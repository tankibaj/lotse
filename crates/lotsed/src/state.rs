//! The registry: projects, sessions, settings, and the request dispatcher.

use crate::agent_env::Injection;
use crate::config::Config;
use crate::db::{DEFAULT_WORKSPACE, Db, DbError, SessionRow};
use crate::git;
use crate::git_glue;
use crate::hub::{ClientTx, Hub};
use crate::launch::{self, LaunchEnv};
use crate::session::{Ctx, SessionError, SessionHandle, SpawnSpec, initial_status, now_ms};
use crate::title::{PLACEHOLDER_TITLE, derive_title};
use lotse_proto::{
    AgentKind, DaemonInfo, DetectSource, ErrorCode, Event, Frame, Message, NewProject, NewSession,
    PROTO_VERSION, Project, ProjectId, ProjectPatch, ProtoError, Request, Response,
    ResponseEnvelope, Session, SessionId, Settings, Snapshot, WorkspaceId,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{Notify, OnceCell};

/// Swatch keys of the mock palette (`SW` in `spec/mock.html`); new projects cycle through them.
const PALETTE: &[&str] = &[
    "red", "coral", "orange", "yellow", "green", "teal", "sky", "blue", "indigo", "purple",
    "lavender", "pink", "gray", "sage", "tan",
];
const MIN_SCROLLBACK: u64 = 64 * 1024;
/// Half of a client's queue limit (`hub`), so a full replay always fits.
const MAX_SCROLLBACK: u64 = 32 * 1024 * 1024;
/// A closed session's process gets this long to react to SIGHUP before SIGKILL.
const KILL_GRACE: Duration = Duration::from_secs(2);
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
/// Detectors sample the screen and the foreground process this often.
const DETECT_TICK: Duration = Duration::from_millis(100);
/// How often each session's git line is re-read.
const GIT_REFRESH: Duration = Duration::from_secs(5);

fn err(code: ErrorCode, message: impl Into<String>) -> Response {
    Response::Error(ProtoError {
        code,
        message: message.into(),
    })
}

fn internal(e: impl std::fmt::Display) -> Response {
    tracing::error!("{e}");
    err(ErrorCode::Internal, "internal error")
}

fn db_err(e: DbError) -> Response {
    internal(e)
}

fn git_err(e: &lotse_git::GitError) -> Response {
    use lotse_git::GitError as G;
    let code = match e {
        G::InvalidBranch(_) | G::NotADirectory(_) | G::NotARepo(_) => ErrorCode::InvalidRequest,
        G::PathExists(_) => ErrorCode::Conflict,
        _ => ErrorCode::Unavailable,
    };
    err(code, e.to_string())
}

fn session_err(e: SessionError) -> Response {
    let code = match e {
        SessionError::Launch(_)
        | SessionError::ShuttingDown
        | SessionError::SlowClient
        | SessionError::InputBacklog => ErrorCode::Unavailable,
        SessionError::Exited
        | SessionError::StillRunning
        | SessionError::NotAtPrompt
        | SessionError::NoSuchChoice => ErrorCode::Conflict,
        SessionError::BadSize => ErrorCode::InvalidRequest,
    };
    err(code, e.to_string())
}

pub struct State {
    cfg: Config,
    pub hub: Arc<Hub>,
    db: Arc<Db>,
    settings: Arc<RwLock<Settings>>,
    sessions: RwLock<HashMap<SessionId, Arc<SessionHandle>>>,
    started_at: u64,
    shutdown: Notify,
    /// Set (under the `sessions` write lock) when the daemon starts killing sessions.
    closing: AtomicBool,
    env: OnceCell<Arc<LaunchEnv>>,
    /// Plugins of the agents' status hooks, written once at start and injected into every pane.
    inject: Injection,
}

impl State {
    pub fn new(cfg: Config, db: Db) -> Result<Arc<Self>, DbError> {
        let pruned = db.prune_sessions()?;
        if pruned > 0 {
            tracing::info!("dropped {pruned} session(s) of the previous daemon run");
        }
        let settings = db.load_settings()?;
        let data_dir = cfg.data_dir.clone();
        Ok(Arc::new(Self {
            cfg,
            hub: Arc::new(Hub::default()),
            db: Arc::new(db),
            settings: Arc::new(RwLock::new(settings)),
            sessions: RwLock::new(HashMap::new()),
            started_at: now_ms(),
            shutdown: Notify::new(),
            closing: AtomicBool::new(false),
            env: OnceCell::new(),
            inject: Injection::prepare(&data_dir),
        }))
    }

    /// Resolves once a client asked for `shutdown`.
    pub async fn shutdown_requested(&self) {
        self.shutdown.notified().await;
    }

    /// The login-shell environment, captured once in the background (first use waits for it).
    pub async fn launch_env(&self) -> Arc<LaunchEnv> {
        self.env
            .get_or_init(|| async {
                let login = self.cfg.login_env;
                let env = tokio::task::spawn_blocking(move || {
                    if login {
                        LaunchEnv::from_login_shell()
                    } else {
                        LaunchEnv::from_process()
                    }
                })
                .await
                .unwrap_or_default();
                Arc::new(env)
            })
            .await
            .clone()
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            hub: self.hub.clone(),
            db: self.db.clone(),
            settings: self.settings.clone(),
        }
    }

    fn settings(&self) -> Settings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn session(&self, id: &SessionId) -> Option<Arc<SessionHandle>> {
        self.sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    fn handles(&self) -> Vec<Arc<SessionHandle>> {
        self.sessions
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    fn session_list(&self, project: Option<&ProjectId>) -> Vec<Session> {
        let mut v: Vec<Session> = self
            .handles()
            .iter()
            .map(|h| h.snapshot())
            .filter(|s| project.is_none_or(|p| &s.project_id == p))
            .collect();
        v.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        v
    }

    /// A client connection ended: stop streaming to it.
    pub fn client_gone(&self, client_id: u64) {
        self.hub.remove(client_id);
        for h in self.handles() {
            h.detach(client_id);
        }
    }

    /// Raw keystrokes from a PTY frame (client → daemon).
    pub fn pty_input(&self, session_id: &SessionId, data: &[u8]) {
        if let Some(h) = self.session(session_id) {
            let _ = h.write_input(data);
        }
    }

    /// Graceful shutdown: hang up every session, wait for them (SIGKILL after the grace period).
    pub fn kill_all_sessions(&self) {
        let handles: Vec<_> = {
            let mut map = self.sessions.write().unwrap_or_else(|e| e.into_inner());
            self.closing.store(true, Ordering::Release);
            map.drain().map(|(_, h)| h).collect()
        };
        let tickets: Vec<_> = handles.iter().filter_map(|h| h.kill()).collect();
        std::thread::scope(|s| {
            for t in tickets {
                s.spawn(move || t.finish(KILL_GRACE));
            }
        });
    }

    pub async fn handle(
        self: &Arc<Self>,
        client: &Arc<ClientTx>,
        req_id: u64,
        req: Request,
    ) -> Option<Response> {
        Some(match req {
            Request::Ping => Response::Pong,
            Request::GetSnapshot => self.snapshot(),
            Request::ListProjects => match self.db.list_projects() {
                Ok(p) => Response::Projects(p),
                Err(e) => db_err(e),
            },
            Request::AddProject(np) => self.add_project(np),
            Request::UpdateProject { project_id, patch } => self.update_project(&project_id, patch),
            Request::RemoveProject { project_id } => self.remove_project(&project_id),
            Request::ListSessions { project_id } => {
                Response::Sessions(self.session_list(project_id.as_ref()))
            }
            Request::CreateSession(ns) => self.create_session(ns).await,
            Request::RenameSession { session_id, title } => {
                self.rename_session(&session_id, &title)
            }
            Request::CloseSession { session_id } => self.close_session(&session_id),
            Request::RetrySession { session_id } => self.retry_session(&session_id),
            Request::MarkRead { session_id } => match self.session(&session_id) {
                None => not_found("session"),
                Some(h) => {
                    if h.mark_read() {
                        self.hub.broadcast(Event::SessionUpdated(h.snapshot()));
                    }
                    Response::Ok
                }
            },
            // Types the answer into the pane; Conflict when there is nothing to answer (the UI
            // ignores it). The status moves on by the agent's hook or the screen, never here.
            Request::ResolvePermission {
                session_id,
                decision,
            } => match self.session(&session_id) {
                None => not_found("session"),
                Some(h) => h
                    .resolve_permission(decision)
                    .map_or_else(session_err, |()| Response::Ok),
            },
            Request::Attach {
                session_id,
                cols,
                rows,
            } => {
                let Some(h) = self.session(&session_id) else {
                    return Some(not_found("session"));
                };
                let res = h.attach(client, cols, rows, |info| {
                    let frame = Frame::Control(Message::Response(ResponseEnvelope {
                        req_id,
                        response: Response::Attached(info),
                    }));
                    lotse_proto::encode(&frame).unwrap_or_default().into()
                });
                return match res {
                    Ok(()) => None,
                    Err(e) => Some(session_err(e)),
                };
            }
            Request::Detach { session_id } => match self.session(&session_id) {
                None => not_found("session"),
                Some(h) => {
                    h.detach(client.id);
                    Response::Ok
                }
            },
            Request::SendInput { session_id, text } => match self.session(&session_id) {
                None => not_found("session"),
                Some(h) => h
                    .write_input(text.as_bytes())
                    .map_or_else(session_err, |()| Response::Ok),
            },
            Request::Resize {
                session_id,
                cols,
                rows,
            } => match self.session(&session_id) {
                None => not_found("session"),
                Some(h) => h
                    .resize(cols, rows)
                    .map_or_else(session_err, |()| Response::Ok),
            },
            Request::GetSettings => Response::Settings(self.settings()),
            Request::UpdateSettings { settings } => self.update_settings(settings),
            Request::DetectLaunchCommands => {
                let env = self.launch_env().await;
                let settings = self.settings();
                match tokio::task::spawn_blocking(move || launch::detect_all(&settings, &env)).await
                {
                    Ok(rows) => Response::LaunchCommands(rows),
                    Err(e) => internal(e),
                }
            }
            Request::GetGitInfo {
                project_id,
                session_id,
            } => self.git_info(&project_id, session_id.as_ref()).await,
            Request::HookEvent {
                agent,
                session_id,
                payload,
            } => match session_id {
                None => Response::Ok,
                Some(id) => match self.session(&id) {
                    None => not_found("session"),
                    Some(h) => {
                        h.on_hook(agent, payload);
                        Response::Ok
                    }
                },
            },
            Request::Shutdown => {
                self.shutdown.notify_one();
                Response::Ok
            }
        })
    }

    fn snapshot(&self) -> Response {
        let projects = match self.db.list_projects() {
            Ok(p) => p,
            Err(e) => return db_err(e),
        };
        Response::Snapshot(Snapshot {
            daemon: DaemonInfo {
                version: env!("CARGO_PKG_VERSION").into(),
                proto_version: PROTO_VERSION,
                pid: std::process::id(),
                started_at: self.started_at,
            },
            projects,
            sessions: self.session_list(None),
            settings: self.settings(),
        })
    }

    // projects

    fn add_project(&self, np: NewProject) -> Response {
        let raw = np.path.trim();
        if !Path::new(raw).is_absolute() {
            return err(ErrorCode::InvalidRequest, "project path must be absolute");
        }
        let path = match std::fs::canonicalize(raw) {
            Ok(p) if p.is_dir() => p,
            Ok(_) => return err(ErrorCode::InvalidRequest, "project path is not a folder"),
            Err(e) => return err(ErrorCode::NotFound, format!("project path: {e}")),
        };
        let path_str = path.to_string_lossy().into_owned();
        match self.db.project_by_path(&path_str) {
            Ok(Some(_)) => return err(ErrorCode::Conflict, "project already added"),
            Ok(None) => {}
            Err(e) => return db_err(e),
        }
        let count = self.db.list_projects().map_or(0, |p| p.len());
        let name = match np.name.as_deref().map(str::trim) {
            Some(n) if !n.is_empty() => n.to_owned(),
            _ => path
                .file_name()
                .map_or_else(|| path_str.clone(), |n| n.to_string_lossy().into_owned()),
        };
        let color = np
            .color
            .unwrap_or_else(|| PALETTE[count % PALETTE.len()].into());
        let icon = np.icon.unwrap_or_else(|| "folder".into());
        if let Err(m) = check_project_fields(&name, &color, &icon) {
            return err(ErrorCode::InvalidRequest, m);
        }
        let project = Project {
            id: ProjectId::new(format!("p-{}", short_id())),
            workspace_id: WorkspaceId::new(DEFAULT_WORKSPACE),
            name,
            is_git: git::is_git_repo(&path),
            path: path_str,
            color,
            icon,
            notifications: lotse_proto::NotificationMode::Global,
        };
        if let Err(e) = self.db.insert_project(&project, now_ms()) {
            return db_err(e);
        }
        self.hub.broadcast(Event::ProjectAdded(project.clone()));
        Response::Project(project)
    }

    fn update_project(&self, id: &ProjectId, patch: ProjectPatch) -> Response {
        let mut p = match self.db.get_project(id) {
            Ok(Some(p)) => p,
            Ok(None) => return not_found("project"),
            Err(e) => return db_err(e),
        };
        if let Some(n) = patch.name {
            p.name = n.trim().to_owned();
        }
        if let Some(c) = patch.color {
            p.color = c;
        }
        if let Some(i) = patch.icon {
            p.icon = i;
        }
        if let Some(n) = patch.notifications {
            p.notifications = n;
        }
        if let Err(m) = check_project_fields(&p.name, &p.color, &p.icon) {
            return err(ErrorCode::InvalidRequest, m);
        }
        p.is_git = git::is_git_repo(Path::new(&p.path));
        if let Err(e) = self.db.update_project(&p) {
            return db_err(e);
        }
        self.hub.broadcast(Event::ProjectUpdated(p.clone()));
        Response::Project(p)
    }

    fn remove_project(&self, id: &ProjectId) -> Response {
        match self.db.get_project(id) {
            Ok(Some(_)) => {}
            Ok(None) => return not_found("project"),
            Err(e) => return db_err(e),
        }
        for s in self.session_list(Some(id)) {
            self.close_session(&s.id);
        }
        if let Err(e) = self.db.delete_project(id) {
            return db_err(e);
        }
        self.hub.broadcast(Event::ProjectRemoved {
            project_id: id.clone(),
        });
        Response::Ok
    }

    // sessions

    async fn create_session(self: &Arc<Self>, ns: NewSession) -> Response {
        let project = match self.db.get_project(&ns.project_id) {
            Ok(Some(p)) => p,
            Ok(None) => return not_found("project"),
            Err(e) => return db_err(e),
        };
        let settings = self.settings();
        let command = match ns.command.as_deref().map(str::trim) {
            Some(c) if !c.is_empty() => c.to_owned(),
            _ => launch::configured_command(&settings, ns.agent).to_owned(),
        };
        let env = self.launch_env().await;
        let mut argv = match env.tokenize(&command) {
            Ok(a) => a,
            Err(m) => return err(ErrorCode::InvalidRequest, format!("launch command: {m}")),
        };
        let project_dir = PathBuf::from(&project.path);
        if !project_dir.is_dir() {
            return err(
                ErrorCode::InvalidRequest,
                format!("project folder {} does not exist", project.path),
            );
        }
        match env.resolve(&argv, &project_dir) {
            Some(p) => argv[0] = p.to_string_lossy().into_owned(),
            None => {
                return err(
                    ErrorCode::Unavailable,
                    format!("`{}` not found on PATH", argv[0]),
                );
            }
        }
        // after the command is known to exist: a worktree must not be left behind by a bad launch
        let branch = ns.worktree.as_ref().map(|w| w.branch.clone());
        let repo = project_dir.clone();
        let (cwd, git) = match tokio::task::spawn_blocking(move || {
            let cwd = git_glue::session_dir(&repo, branch.as_deref())?;
            let git = git_glue::info(&cwd);
            Ok::<_, lotse_git::GitError>((cwd, git))
        })
        .await
        {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => return git_err(&e),
            Err(e) => return internal(e),
        };

        let id = SessionId::new(format!("s-{}", short_id()));
        let (title, title_auto) = match ns
            .title
            .as_deref()
            .and_then(derive_title)
            .or_else(|| ns.first_prompt.as_deref().and_then(derive_title))
        {
            Some(t) => (t, false),
            None => (PLACEHOLDER_TITLE.to_owned(), true),
        };
        let now = now_ms();
        let meta = Session {
            id: id.clone(),
            project_id: project.id.clone(),
            title,
            agent: ns.agent,
            command: command.clone(),
            status: initial_status(ns.agent),
            sub_agents: Vec::new(),
            git,
            created_at: now,
            last_activity: now,
            running_since: None,
            unread: false,
            perm: None,
            source: DetectSource::Process,
        };
        let spec = SpawnSpec {
            argv,
            cwd: cwd.clone(),
            env: self.session_env(&env, &id, &project.id),
        };
        let ring_cap = usize::try_from(settings.scrollback_bytes).unwrap_or(usize::MAX);
        let row = SessionRow {
            id: id.clone(),
            project_id: project.id.clone(),
            title: meta.title.clone(),
            title_auto,
            agent: ns.agent,
            command,
            created_at: now,
            last_activity: now,
        };
        if let Err(e) = self.db.insert_session(&row) {
            drop_new_worktree(&project_dir, &cwd);
            return db_err(e);
        }
        // Registered and announced inside `spawn`, before the process can report anything: a
        // fast-failing command still produces `session_created` first, then its exit events.
        let handle = match SessionHandle::spawn(
            self.ctx(),
            meta,
            title_auto,
            spec,
            ring_cap,
            DEFAULT_COLS,
            DEFAULT_ROWS,
            |handle, snapshot| {
                let mut map = self.sessions.write().unwrap_or_else(|e| e.into_inner());
                if self.closing.load(Ordering::Acquire) {
                    return false;
                }
                map.insert(snapshot.id.clone(), handle.clone());
                self.hub.broadcast(Event::SessionCreated(snapshot.clone()));
                true
            },
        ) {
            Ok(h) => h,
            Err(e) => {
                let _ = self.db.delete_session(&id);
                drop_new_worktree(&project_dir, &cwd);
                return session_err(e);
            }
        };
        if let Some(prompt) = ns.first_prompt.filter(|p| !p.trim().is_empty()) {
            send_first_prompt(handle.clone(), prompt);
        }
        // the current state, not the one at spawn: a command that already exited reads as exited
        Response::Session(handle.snapshot())
    }

    /// Login environment plus what the child needs to know about Lotse. The token itself is never
    /// exported, only the path of its file.
    fn session_env(
        &self,
        env: &LaunchEnv,
        id: &SessionId,
        project: &ProjectId,
    ) -> Vec<(String, String)> {
        let mut vars = env.vars().clone();
        vars.insert("TERM".into(), "xterm-256color".into());
        vars.insert("COLORTERM".into(), "truecolor".into());
        vars.insert("TERM_PROGRAM".into(), "Lotse".into());
        vars.insert("LOTSE_SESSION_ID".into(), id.to_string());
        vars.insert("LOTSE_PROJECT_ID".into(), project.to_string());
        vars.insert(
            "LOTSE_SOCKET".into(),
            self.cfg.socket_path().to_string_lossy().into_owned(),
        );
        vars.insert(
            "LOTSE_TOKEN_FILE".into(),
            self.cfg.token_path().to_string_lossy().into_owned(),
        );
        if let Some(hook) = hook_bin() {
            vars.insert("LOTSE_HOOK_BIN".into(), hook.to_string_lossy().into_owned());
        }
        self.inject.apply(&mut vars);
        vars.into_iter().collect()
    }

    fn rename_session(&self, id: &SessionId, title: &str) -> Response {
        let Some(h) = self.session(id) else {
            return not_found("session");
        };
        match h.rename(title) {
            Some(_) => Response::Ok,
            None => err(ErrorCode::InvalidRequest, "title is empty"),
        }
    }

    fn close_session(&self, id: &SessionId) -> Response {
        let removed = self
            .sessions
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        let Some(h) = removed else {
            return not_found("session");
        };
        if let Some(ticket) = h.kill() {
            std::thread::spawn(move || ticket.finish(KILL_GRACE));
        }
        if let Err(e) = self.db.delete_session(id) {
            tracing::warn!("deleting session {id}: {e}");
        }
        self.hub.broadcast(Event::SessionClosed {
            session_id: id.clone(),
        });
        Response::Ok
    }

    fn retry_session(self: &Arc<Self>, id: &SessionId) -> Response {
        let Some(h) = self.session(id) else {
            return not_found("session");
        };
        let initial = initial_status(h.snapshot().agent);
        match h.respawn(initial) {
            Ok(meta) => {
                let at = now_ms();
                self.hub.broadcast(Event::StatusChanged {
                    session_id: id.clone(),
                    status: meta.status,
                    source: DetectSource::Process,
                    perm: None,
                    running_since: None,
                    at,
                });
                self.hub.broadcast(Event::SubAgentsChanged {
                    session_id: id.clone(),
                    sub_agents: Vec::new(),
                });
                self.hub.broadcast(Event::SessionUpdated(meta.clone()));
                Response::Session(meta)
            }
            Err(e) => session_err(e),
        }
    }

    // settings

    fn update_settings(&self, mut s: Settings) -> Response {
        if let Err(m) = normalize_settings(&mut s, &self.launch_env_now()) {
            return err(ErrorCode::InvalidRequest, m);
        }
        if let Err(e) = self.db.save_settings(&s) {
            return db_err(e);
        }
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = s.clone();
        self.hub.broadcast(Event::SettingsChanged(s.clone()));
        Response::Settings(s)
    }

    /// Environment for validating commands: whatever is captured already, else the daemon's own.
    fn launch_env_now(&self) -> LaunchEnv {
        self.env
            .get()
            .map_or_else(LaunchEnv::from_process, |e| (**e).clone())
    }

    async fn git_info(&self, project_id: &ProjectId, session_id: Option<&SessionId>) -> Response {
        let project = match self.db.get_project(project_id) {
            Ok(Some(p)) => p,
            Ok(None) => return not_found("project"),
            Err(e) => return db_err(e),
        };
        // a session may run in its own worktree: ask about the folder it runs in
        let mut dir = PathBuf::from(&project.path);
        if let Some(sid) = session_id {
            match self.session(sid) {
                Some(h) if &h.snapshot().project_id == project_id => dir = h.cwd(),
                _ => return not_found("session"),
            }
        }
        match tokio::task::spawn_blocking(move || git_glue::info(&dir)).await {
            Ok(info) => Response::GitInfo(info),
            Err(e) => internal(e),
        }
    }

    /// Periodic work: every 100 ms the detectors (screen samples, foreground process, tick), every
    /// 5 s the git line of each session. Runs on plain threads: both block (`ps`, `git`).
    pub fn start_background(self: &Arc<Self>) {
        let state = Arc::downgrade(self);
        let spawn = |name: &str, period: Duration, work: fn(&State)| {
            let state = state.clone();
            let built = std::thread::Builder::new()
                .name(name.into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(period);
                        let Some(state) = state.upgrade() else { return };
                        if state.closing.load(Ordering::Acquire) {
                            return;
                        }
                        work(&state);
                    }
                });
            if let Err(e) = built {
                tracing::error!("cannot start thread {name}: {e}");
            }
        };
        spawn("lotse-detect-tick", DETECT_TICK, |s| {
            for h in s.handles() {
                h.tick();
            }
        });
        spawn("lotse-git-refresh", GIT_REFRESH, |s| {
            // Sessions of one project share a folder: ask git once per folder, and only for a
            // folder where a live session saw PTY output or a hook since its last refresh.
            type Due = (Arc<SessionHandle>, Option<u64>);
            let mut folders: HashMap<PathBuf, Vec<Due>> = HashMap::new();
            for h in s.handles() {
                let due = h.git_due();
                folders.entry(h.cwd()).or_default().push((h, due));
            }
            for (dir, sessions) in folders {
                if sessions.iter().all(|(_, due)| due.is_none()) {
                    continue;
                }
                let git = git_glue::info(&dir);
                for (h, due) in sessions {
                    h.apply_git(git.clone());
                    if let Some(token) = due {
                        h.git_refreshed(token);
                    }
                }
            }
        });
    }
}

/// A launch that failed after its worktree was made must not leave the folder behind (the branch
/// stays, so a retry with the same name reuses it).
fn drop_new_worktree(project_dir: &Path, cwd: &Path) {
    if cwd != project_dir {
        git_glue::remove_worktree(project_dir, cwd);
    }
}

/// `lotse-hook` next to the daemon binary (how both ship), unless `LOTSE_HOOK_BIN` points elsewhere.
fn hook_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LOTSE_HOOK_BIN").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let sibling = std::env::current_exe().ok()?.with_file_name("lotse-hook");
    sibling.is_file().then_some(sibling)
}

fn not_found(what: &str) -> Response {
    err(ErrorCode::NotFound, format!("{what} not found"))
}

fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..10].to_owned()
}

fn check_project_fields(name: &str, color: &str, icon: &str) -> Result<(), String> {
    if name.is_empty() || name.chars().count() > 100 {
        return Err("project name must be 1-100 characters".into());
    }
    if !PALETTE.contains(&color) {
        return Err(format!("unknown color {color:?}"));
    }
    if icon.is_empty() || icon.len() > 40 {
        return Err("icon must be 1-40 characters".into());
    }
    Ok(())
}

/// Empty launch command = reset to the default; every command must tokenize; limits on the ring.
pub fn normalize_settings(s: &mut Settings, env: &LaunchEnv) -> Result<(), String> {
    let lc = &mut s.launch_commands;
    for (agent, cmd) in [
        (AgentKind::Terminal, &mut lc.terminal),
        (AgentKind::ClaudeCode, &mut lc.claude_code),
        (AgentKind::Codex, &mut lc.codex),
        (AgentKind::OpenCode, &mut lc.opencode),
    ] {
        let trimmed = cmd.trim();
        *cmd = if trimmed.is_empty() {
            agent.default_command().to_owned()
        } else {
            trimmed.to_owned()
        };
        env.tokenize(cmd)
            .map_err(|m| format!("{} launch command: {m}", agent.label()))?;
    }
    if !(MIN_SCROLLBACK..=MAX_SCROLLBACK).contains(&s.scrollback_bytes) {
        return Err(format!(
            "scrollback_bytes must be between {MIN_SCROLLBACK} and {MAX_SCROLLBACK}"
        ));
    }
    Ok(())
}

/// Best effort: wait for the agent's first output (it is ready for input), then type the prompt.
/// Unverified: Claude Code / Codex / OpenCode accept input this early; check in WP8 with the real CLIs.
fn send_first_prompt(handle: Arc<SessionHandle>, prompt: String) {
    let spawned = std::thread::Builder::new()
        .name("lotse-first-prompt".into())
        .spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !handle.has_output() && handle.is_alive() && std::time::Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            std::thread::sleep(Duration::from_millis(700));
            let text = prompt.trim_end_matches(['\r', '\n']);
            if handle.inject(text.as_bytes()).is_ok() {
                std::thread::sleep(Duration::from_millis(150));
                let _ = handle.inject(b"\r");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("first prompt not sent: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::Status;

    #[test]
    fn settings_normalization_resets_empty_and_rejects_bad() {
        let env = LaunchEnv::from_process();
        let mut s = Settings::default();
        s.launch_commands.claude_code = "   ".into();
        s.launch_commands.codex = "  codex --fast ".into();
        normalize_settings(&mut s, &env).unwrap();
        assert_eq!(s.launch_commands.claude_code, "claude");
        assert_eq!(s.launch_commands.codex, "codex --fast");

        let mut bad = Settings::default();
        bad.launch_commands.opencode = "open 'code".into();
        assert!(
            normalize_settings(&mut bad, &env)
                .unwrap_err()
                .contains("OpenCode")
        );

        let mut small = Settings {
            scrollback_bytes: 10,
            ..Settings::default()
        };
        assert!(normalize_settings(&mut small, &env).is_err());
    }

    #[test]
    fn project_field_checks() {
        assert!(check_project_fields("x", "teal", "folder").is_ok());
        assert!(check_project_fields("", "teal", "folder").is_err());
        assert!(check_project_fields("x", "mauve", "folder").is_err());
        assert!(check_project_fields("x", "teal", "").is_err());
    }

    #[test]
    fn session_status_defaults_follow_the_mock() {
        assert_eq!(initial_status(AgentKind::Terminal), Status::Idle);
        assert_eq!(initial_status(AgentKind::ClaudeCode), Status::Unknown);
    }
}
