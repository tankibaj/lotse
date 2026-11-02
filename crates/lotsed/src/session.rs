//! One PTY session: the child process, its raw-byte ring buffer, the attached clients.
//!
//! Threads (the PTY API is blocking): one reads the master, one writes input, one waits for the
//! child. All shared state lives behind one mutex (`Inner`); the writer thread is fed through a
//! channel so a child that stops reading input can never block the reader (deadlock).

use crate::db::Db;
use crate::hub::{Bytes, ClientTx, Hub};
use crate::ring::Ring;
use crate::title::{LineTracker, derive_title};
use lotse_proto::{AttachInfo, DetectSource, Event, Frame, Session, SessionId, Status, UnixMs};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
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
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("launch failed: {0}")]
    Launch(String),
    #[error("session process has exited")]
    Exited,
    #[error("session is still running")]
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
        let handle = Arc::new(Self {
            id: meta.id.clone(),
            ctx,
            inner: Mutex::new(Inner {
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
        let mut g = self.lock();
        g.got_output = true;
        g.meta.last_activity = now_ms();
        g.ring.push(data);
        g.attached.retain(|_, c| c.send_bytes(frame.clone()));
    }

    fn on_exit(&self, generation: u64, code: Option<i32>) {
        let at = now_ms();
        let status = {
            let mut g = self.lock();
            if g.generation != generation || g.closing {
                return;
            }
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
            status
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
        g.meta.status = initial;
        g.meta.source = DetectSource::Process;
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
