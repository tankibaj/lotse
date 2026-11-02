//! One PTY session: the child process, its raw-byte ring buffer, the attached clients.
//!
//! Threads (the PTY API is blocking): one reads the master, one writes input, one waits for the
//! child. All shared state lives behind one mutex (`Inner`); the writer thread is fed through a
//! channel so a child that stops reading input can never block the reader (deadlock).

use crate::db::Db;
use crate::detect_glue::{Detector, Trigger, notification, process_name};
use crate::hub::{Bytes, ClientTx, Hub};
use crate::ring::Ring;
use crate::title::{LineTracker, derive_title};
use lotse_detect::engine::StatusUpdate;
use lotse_detect::osc7501::QUERY_REPLY;
use lotse_proto::{
    AgentKind, AttachInfo, DetectSource, Event, Frame, GitInfo, Session, SessionId, Settings,
    Status, UnixMs,
};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Replay is sent in frames of this size.
const REPLAY_CHUNK: usize = 32 * 1024;
/// After the child exits, wait this long for the reader to drain the last output.
const DRAIN_GRACE: Duration = Duration::from_millis(300);

pub fn now_ms() -> UnixMs {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Everything needed to (re)start the process.
#[derive(Debug, Clone)]
pub struct SpawnSpec {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

/// Shared services of a session.
#[derive(Clone)]
pub struct Ctx {
    pub hub: Arc<Hub>,
    pub db: Arc<Db>,
    /// Shared with the registry: notification toggles are read when a status changes.
    pub settings: Arc<RwLock<Settings>>,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("launch failed: {0}")]
    Launch(String),
    #[error("session process has exited")]
    Exited,
    #[error("the process is still running; Retry re-runs a command that has exited")]
    StillRunning,
    #[error("invalid size")]
    BadSize,
    #[error("the daemon is shutting down")]
    ShuttingDown,
    #[error("client is too slow to receive the replay")]
    SlowClient,
    #[error("too much unread input is queued for the session")]
    InputBacklog,
}

struct Launched {
    master: Box<dyn MasterPty + Send>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

fn launch(spec: &SpawnSpec, cols: u16, rows: u16) -> Result<Launched, SessionError> {
    let err = |e: anyhow::Error| SessionError::Launch(format!("{e:#}"));
    let pair = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(err)?;
    let (program, args) = spec
        .argv
        .split_first()
        .ok_or_else(|| SessionError::Launch("empty command".into()))?;
    let mut cmd = CommandBuilder::new(program);
    cmd.args(args);
    cmd.env_clear();
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    cmd.cwd(&spec.cwd);
    let child = pair.slave.spawn_command(cmd).map_err(err)?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(err)?;
    let writer = pair.master.take_writer().map_err(err)?;
    Ok(Launched {
        master: pair.master,
        reader,
        writer,
        child,
    })
}

struct Live {
    input: mpsc::Sender<Vec<u8>>,
    /// Bytes queued for the writer thread (bounded: a child that never reads must not grow us).
    pending: Arc<AtomicUsize>,
    master: Box<dyn MasterPty + Send>,
}

/// Most input bytes waiting for a child that is not reading.
const MAX_PENDING_INPUT: usize = 8 * 1024 * 1024;

struct Inner {
    meta: Session,
    title_auto: bool,
    tracker: LineTracker,
    ring: Ring,
    attached: HashMap<u64, Arc<ClientTx>>,
    cols: u16,
    rows: u16,
    live: Option<Live>,
    /// Process group of the session leader (kept after exit: survivors may still be in it).
    pgid: Option<u32>,
    /// Bumped on every (re)start so a late exit report of an older process is ignored.
    generation: u64,
    closing: bool,
    got_output: bool,
    spec: SpawnSpec,
    /// Feeds the status engine (hooks, OSC 7501, screen, process).
    detector: Detector,
}

pub struct SessionHandle {
    id: SessionId,
    ctx: Ctx,
    inner: Mutex<Inner>,
}

/// Returned by [`SessionHandle::kill`]; [`KillTicket::finish`] escalates to SIGKILL.
pub struct KillTicket {
    /// Process groups that got SIGHUP: the session leader's and the PTY's foreground group
    /// (a shell with job control puts the running agent in a group of its own).
    pgids: Vec<u32>,
    /// The PTY master stays open until the process groups are gone: closing it early makes the
    /// kernel hang up the terminal first, which races with the shell forwarding SIGHUP to its jobs.
    _live: Option<Live>,
}

impl KillTicket {
    /// Give the groups `grace` to exit after SIGHUP (agents may flush state), then SIGKILL what is
    /// left: a child that ignores SIGHUP (`nohup`, `trap '' HUP`) must not outlive its session.
    ///
    /// Pid-reuse hazard: a group that vanished and whose id is taken by an unrelated new group
    /// within `grace` would be hit; a pid is not recycled that fast.
    pub fn finish(self, grace: Duration) {
        let alive = |pgids: &[u32]| pgids.iter().any(|&p| group_alive(p));
        let end = Instant::now() + grace;
        while Instant::now() < end && alive(&self.pgids) {
            std::thread::sleep(Duration::from_millis(20));
        }
        for &pgid in &self.pgids {
            if group_alive(pgid) {
                signal_group(pgid, Signal::SIGKILL);
            }
        }
        let end = Instant::now() + Duration::from_secs(1);
        while Instant::now() < end && alive(&self.pgids) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// `kill(-pgid, 0)`: any member left (a zombie counts until reaped).
fn group_alive(pgid: u32) -> bool {
    !matches!(
        killpg(Pid::from_raw(pgid as i32), None),
        Err(nix::errno::Errno::ESRCH)
    )
}

fn signal_group(pid: u32, sig: Signal) {
    // portable-pty makes the child a session leader (setsid), so its pid is the process group id.
    let _ = killpg(Pid::from_raw(pid as i32), sig);
}

pub fn initial_status(agent: lotse_proto::AgentKind) -> Status {
    match agent {
        lotse_proto::AgentKind::Terminal => Status::Idle,
        _ => Status::Unknown,
    }
}

impl SessionHandle {
    /// Start the process and its threads. `meta` carries id, project, title, command, git.
    ///
    /// `announce` runs while the session lock is held, after the process started and before any
    /// of its events can be reported: register the handle and broadcast `session_created` there, so
    /// no client sees an exit before the creation. Returning `false` aborts (daemon shutting down).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        ctx: Ctx,
        meta: Session,
        title_auto: bool,
        spec: SpawnSpec,
        ring_cap: usize,
        cols: u16,
        rows: u16,
        announce: impl FnOnce(&Arc<Self>, &Session) -> bool,
    ) -> Result<Arc<Self>, SessionError> {
        let launched = launch(&spec, cols, rows)?;
        let pgid = launched.child.process_id();
        let detector = Detector::new(meta.id.clone(), meta.agent, cols, rows);
        let handle = Arc::new(Self {
            id: meta.id.clone(),
            ctx,
            inner: Mutex::new(Inner {
                detector,
                meta,
                title_auto,
                tracker: LineTracker::new(),
                ring: Ring::new(ring_cap),
                attached: HashMap::new(),
                cols,
                rows,
                live: None,
                pgid,
                generation: 0,
                closing: false,
                got_output: false,
                spec,
            }),
        });
        let ticket = {
            let mut g = handle.lock();
            g.live = Some(handle.start_io(launched, 0));
            let snapshot = g.meta.clone();
            if announce(&handle, &snapshot) {
                None
            } else {
                drop(g);
                handle.kill()
            }
        };
        if let Some(ticket) = ticket {
            std::thread::spawn(move || ticket.finish(Duration::from_secs(2)));
            return Err(SessionError::ShuttingDown);
        }
        Ok(handle)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Session {
        self.lock().meta.clone()
    }

    pub fn is_alive(&self) -> bool {
        self.lock().live.is_some()
    }

    pub fn has_output(&self) -> bool {
        self.lock().got_output
    }

    fn start_io(self: &Arc<Self>, launched: Launched, generation: u64) -> Live {
        let Launched {
            master,
            mut reader,
            mut writer,
            mut child,
        } = launched;
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        let pending = Arc::new(AtomicUsize::new(0));
        let queued = pending.clone();
        spawn_thread("lotse-pty-in", move || {
            for chunk in input_rx {
                let ok = writer
                    .write_all(&chunk)
                    .and_then(|()| writer.flush())
                    .is_ok();
                queued.fetch_sub(chunk.len(), Ordering::AcqRel);
                if !ok {
                    break;
                }
            }
        });

        let (done_tx, done_rx) = mpsc::channel::<()>();
        let this = self.clone();
        spawn_thread("lotse-pty-out", move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => this.on_output(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let _ = done_tx.send(());
        });

        let this = self.clone();
        spawn_thread("lotse-pty-wait", move || {
            let code = child.wait().ok().map(|s| s.exit_code() as i32);
            // let the reader flush what the child wrote last (a grandchild may keep the PTY open)
            let _ = done_rx.recv_timeout(DRAIN_GRACE);
            this.on_exit(generation, code);
        });

        Live {
            input: input_tx,
            pending,
            master,
        }
    }

    fn on_output(&self, data: &[u8]) {
        let frame: Bytes = match lotse_proto::encode(&Frame::Pty {
            session_id: self.id.clone(),
            data: data.to_vec(),
        }) {
            Ok(b) => b.into(),
            Err(_) => return,
        };
        let at = now_ms();
        let triggers = {
            let mut g = self.lock();
            g.got_output = true;
            g.meta.last_activity = at;
            g.ring.push(data);
            g.attached.retain(|_, c| c.send_bytes(frame.clone()));
            let fed = g.detector.on_output(data, Instant::now(), at);
            if fed.reply {
                // the agent asked whether this terminal speaks OSC 7501: it does
                let _ = queue_input(g.live.as_ref(), QUERY_REPLY);
            }
            self.apply(&mut g, fed.updates)
        };
        self.notify(triggers);
    }

    /// An agent hook payload (`Request::HookEvent`).
    pub fn on_hook(&self, agent: AgentKind, payload: serde_json::Value) {
        let at = now_ms();
        let triggers = {
            let mut g = self.lock();
            if g.closing || g.live.is_none() {
                return;
            }
            let update = g.detector.on_hook(agent, payload, at);
            self.apply(&mut g, update.into_iter().collect())
        };
        self.notify(triggers);
    }

    /// About every 100 ms: foreground process, screen sample, clock tick.
    pub fn tick(&self) {
        let now = Instant::now();
        let at = now_ms();
        // the foreground process group is cheap to read; its name needs a `ps`, so only on change
        let changed = {
            let g = self.lock();
            if g.closing || g.live.is_none() {
                return;
            }
            let pgid = g
                .live
                .as_ref()
                .and_then(|l| l.master.process_group_leader());
            (pgid != g.detector.fg_pgid).then_some(pgid).flatten()
        };
        let name = changed.map(|pgid| (pgid, process_name(pgid)));
        let triggers = {
            let mut g = self.lock();
            if g.closing || g.live.is_none() {
                return;
            }
            let mut updates = Vec::new();
            if let Some((pgid, name)) = name {
                g.detector.fg_pgid = Some(pgid);
                if let Some(name) = name {
                    updates.extend(g.detector.on_foreground(name, at));
                }
            }
            updates.extend(g.detector.tick(now, at));
            self.apply(&mut g, updates)
        };
        self.notify(triggers);
    }

    /// Take a fresh git line (branch, ahead, dirty) for this session's folder; broadcast
    /// `git_info_changed` when it moved. The caller reads it with `git_glue::info(&self.cwd())` (off
    /// every lock: it runs `git`), once per folder when several sessions share one.
    pub fn apply_git(&self, git: GitInfo) {
        let changed = {
            let mut g = self.lock();
            if g.closing || g.meta.git == git {
                false
            } else {
                g.meta.git = git.clone();
                true
            }
        };
        if changed {
            self.ctx.hub.broadcast(Event::GitInfoChanged {
                session_id: self.id.clone(),
                git,
            });
        }
    }

    /// Folder the session runs in (the project, or its worktree).
    pub fn cwd(&self) -> PathBuf {
        self.lock().spec.cwd.clone()
    }

    /// Merge engine updates into the session and broadcast what changed. Called with the session
    /// lock held so events keep their order; returns what may deserve a notification.
    fn apply(&self, g: &mut Inner, updates: Vec<StatusUpdate>) -> Vec<(Session, Trigger)> {
        let mut triggers = Vec::new();
        let notify =
            g.detector.agent() != AgentKind::Terminal || g.meta.agent != AgentKind::Terminal;
        for update in updates {
            let prev = g.meta.status;
            let prev_perm = g.meta.perm.clone();
            let status_moved = prev != update.status || prev_perm != update.perm;
            g.meta.status = update.status;
            g.meta.source = update.source;
            g.meta.perm = update.perm.clone();
            g.meta.running_since = update.running_since;
            g.meta.last_activity = update.at;
            let finished = matches!(update.status, Status::Done | Status::Fail)
                && !matches!(prev, Status::Done | Status::Fail);
            if finished && !g.meta.unread {
                g.meta.unread = true;
                self.ctx
                    .hub
                    .broadcast(Event::SessionUpdated(g.meta.clone()));
            }
            if status_moved {
                self.ctx.hub.broadcast(Event::StatusChanged {
                    session_id: self.id.clone(),
                    status: update.status,
                    source: update.source,
                    perm: update.perm.clone(),
                    running_since: update.running_since,
                    at: update.at,
                });
                if notify {
                    let trigger = match update.status {
                        // a new request while one is open is a new reason to look up
                        Status::Perm if prev != Status::Perm || update.perm != prev_perm => {
                            Some(Trigger::Perm(update.perm.clone()))
                        }
                        Status::Fail if prev != Status::Fail => Some(Trigger::Fail),
                        // a finished turn, not an agent that merely settled at its prompt
                        Status::Done if matches!(prev, Status::Run | Status::Perm) => {
                            Some(Trigger::Done)
                        }
                        _ => None,
                    };
                    triggers.extend(trigger.map(|t| (g.meta.clone(), t)));
                }
            }
            if let Some(subs) = update.sub_agents.filter(|s| *s != g.meta.sub_agents) {
                for sub in &subs {
                    let before = g.meta.sub_agents.iter().find(|s| s.id == sub.id);
                    if before.map(|s| s.status) != Some(sub.status) {
                        self.ctx.hub.broadcast(Event::SubAgentStatus {
                            session_id: self.id.clone(),
                            sub_agent_id: sub.id.clone(),
                            status: sub.status,
                        });
                        if notify && sub.status == Status::Done {
                            triggers
                                .push((g.meta.clone(), Trigger::SubAgentDone(sub.name.clone())));
                        }
                    }
                }
                g.meta.sub_agents.clone_from(&subs);
                self.ctx.hub.broadcast(Event::SubAgentsChanged {
                    session_id: self.id.clone(),
                    sub_agents: subs,
                });
            }
        }
        triggers
    }

    /// Turn triggers into `Event::Notification` (settings and the project's override applied).
    fn notify(&self, triggers: Vec<(Session, Trigger)>) {
        for (session, trigger) in triggers {
            let project = match self.ctx.db.get_project(&session.project_id) {
                Ok(Some(p)) => p,
                _ => continue,
            };
            let settings = self
                .ctx
                .settings
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .notifications
                .clone();
            if let Some(event) = notification(
                &settings,
                &project.notifications,
                &project.name,
                &session,
                &trigger,
                now_ms(),
            ) {
                self.ctx.hub.broadcast(event);
            }
        }
    }

    fn on_exit(&self, generation: u64, code: Option<i32>) {
        let at = now_ms();
        let (status, trigger) = {
            let mut g = self.lock();
            if g.generation != generation || g.closing {
                return;
            }
            g.detector.on_exit(code, at);
            g.live = None;
            // keep the group id only while survivors hold it (a freed id may be reused by anyone)
            if g.pgid.is_some_and(|p| !group_alive(p)) {
                g.pgid = None;
            }
            // process-group identity: the weakest detection source, a better one may refine it
            let status = if code == Some(0) {
                Status::Done
            } else {
                Status::Fail
            };
            g.meta.status = status;
            g.meta.source = DetectSource::Process;
            g.meta.running_since = None;
            g.meta.perm = None;
            g.meta.last_activity = at;
            let agent_session =
                g.meta.agent != AgentKind::Terminal || g.detector.agent() != AgentKind::Terminal;
            let failed = status == Status::Fail && !matches!(code, Some(130 | 143));
            let trigger = (agent_session && failed).then(|| (g.meta.clone(), Trigger::Fail));
            (status, trigger)
        };
        self.ctx.hub.broadcast(Event::SessionExited {
            session_id: self.id.clone(),
            exit_code: code,
        });
        self.ctx.hub.broadcast(Event::StatusChanged {
            session_id: self.id.clone(),
            status,
            source: DetectSource::Process,
            perm: None,
            running_since: None,
            at,
        });
        self.notify(trigger.into_iter().collect());
    }

    /// Answer the request and start streaming, atomically: `respond` builds the `attached`
    /// response, which is queued before the replay and before any live byte.
    pub fn attach(
        &self,
        client: &Arc<ClientTx>,
        cols: u16,
        rows: u16,
        respond: impl FnOnce(AttachInfo) -> Bytes,
    ) -> Result<(), SessionError> {
        check_size(cols, rows)?;
        let mut g = self.lock();
        g.cols = cols;
        g.rows = rows;
        g.detector.resize(cols, rows);
        if let Some(live) = &g.live {
            let _ = live.master.resize(pty_size(cols, rows));
        }
        let replay = g.ring.snapshot();
        let mut queued = client.send_bytes(respond(AttachInfo {
            session_id: self.id.clone(),
            cols,
            rows,
            replay_len: replay.len() as u64,
        }));
        for chunk in replay.chunks(REPLAY_CHUNK) {
            queued &= client.send_frame(&Frame::Pty {
                session_id: self.id.clone(),
                data: chunk.to_vec(),
            });
        }
        if !queued {
            return Err(SessionError::SlowClient);
        }
        g.attached.insert(client.id, client.clone());
        Ok(())
    }

    pub fn detach(&self, client_id: u64) {
        self.lock().attached.remove(&client_id);
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), SessionError> {
        check_size(cols, rows)?;
        let mut g = self.lock();
        g.cols = cols;
        g.rows = rows;
        g.detector.resize(cols, rows);
        if let Some(live) = &g.live {
            let _ = live.master.resize(pty_size(cols, rows));
        }
        Ok(())
    }

    /// Keystrokes from a client. Also feeds the first-prompt title tracker.
    pub fn write_input(&self, data: &[u8]) -> Result<(), SessionError> {
        let renamed = {
            let mut g = self.lock();
            queue_input(g.live.as_ref(), data)?;
            if g.title_auto {
                g.tracker.feed(data).inspect(|title| {
                    g.meta.title.clone_from(title);
                    g.title_auto = false;
                })
            } else {
                None
            }
        };
        if let Some(title) = renamed {
            self.persist_title(&title, false);
        }
        Ok(())
    }

    /// Programmatic text (first prompt): written like input but never used as a title source.
    pub fn inject(&self, data: &[u8]) -> Result<(), SessionError> {
        queue_input(self.lock().live.as_ref(), data)
    }

    /// User edit of the title; switches the automatic title off. `None` when the title is empty.
    pub fn rename(&self, title: &str) -> Option<Session> {
        let title = derive_title(title)?;
        {
            let mut g = self.lock();
            g.meta.title.clone_from(&title);
            g.title_auto = false;
        }
        self.persist_title(&title, false);
        Some(self.snapshot())
    }

    fn persist_title(&self, title: &str, title_auto: bool) {
        if let Err(e) = self
            .ctx
            .db
            .update_session_title(&self.id, title, title_auto)
        {
            tracing::warn!("persisting title of {}: {e}", self.id);
        }
        self.ctx
            .hub
            .broadcast(Event::SessionUpdated(self.snapshot()));
    }

    pub fn mark_read(&self) -> bool {
        let mut g = self.lock();
        std::mem::replace(&mut g.meta.unread, false)
    }

    /// Restart the process of an exited session (the "Retry" action).
    pub fn respawn(self: &Arc<Self>, initial: Status) -> Result<Session, SessionError> {
        let mut g = self.lock();
        if g.closing || g.live.is_some() {
            return Err(SessionError::StillRunning);
        }
        let launched = launch(&g.spec, g.cols, g.rows)?;
        g.pgid = launched.child.process_id();
        g.generation += 1;
        let generation = g.generation;
        g.live = Some(self.start_io(launched, generation));
        let (cols, rows) = (g.cols, g.rows);
        g.detector.reset(cols, rows);
        g.meta.status = initial;
        g.meta.source = DetectSource::Process;
        g.meta.sub_agents.clear();
        g.meta.perm = None;
        g.meta.running_since = None;
        g.meta.last_activity = now_ms();
        g.got_output = false;
        Ok(g.meta.clone())
    }

    /// Hang up the process groups and forget the clients. Idempotent. The ticket escalates to
    /// SIGKILL. Also works after the leader exited on its own: survivors may remain in its group.
    pub fn kill(&self) -> Option<KillTicket> {
        let mut g = self.lock();
        g.closing = true;
        g.attached.clear();
        let leader = g.pgid?;
        let live = g.live.take();
        if live.is_none() && !group_alive(leader) {
            // exited long ago, nobody left in the group: the number may belong to someone else now
            g.pgid = None;
            return None;
        }
        let mut pgids = vec![leader];
        if let Some(live) = &live {
            // the foreground group is where the running agent lives under job control
            pgids.extend(
                live.master
                    .process_group_leader()
                    .and_then(|p| u32::try_from(p).ok())
                    .filter(|p| *p != leader && *p > 1),
            );
        }
        drop(g);
        for &pgid in &pgids {
            signal_group(pgid, Signal::SIGHUP);
        }
        Some(KillTicket { pgids, _live: live })
    }
}

fn queue_input(live: Option<&Live>, data: &[u8]) -> Result<(), SessionError> {
    let live = live.ok_or(SessionError::Exited)?;
    if live.pending.fetch_add(data.len(), Ordering::AcqRel) + data.len() > MAX_PENDING_INPUT {
        live.pending.fetch_sub(data.len(), Ordering::AcqRel);
        return Err(SessionError::InputBacklog);
    }
    if live.input.send(data.to_vec()).is_err() {
        live.pending.fetch_sub(data.len(), Ordering::AcqRel);
        return Err(SessionError::Exited);
    }
    Ok(())
}

fn check_size(cols: u16, rows: u16) -> Result<(), SessionError> {
    if cols == 0 || rows == 0 || cols > 1000 || rows > 1000 {
        return Err(SessionError::BadSize);
    }
    Ok(())
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn spawn_thread(name: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.into()).spawn(f) {
        tracing::error!("cannot start thread {name}: {e}");
    }
}
