//! Control messages (frame type `0x01`, JSON). Adjacently tagged so TypeScript can mirror them 1:1.
//!
//! Flow: client sends `hello` (token first), daemon answers `hello_ok`. Then the client sends
//! `request`s (each with a `req_id`), the daemon answers each with a `response` carrying the same
//! `req_id`, and pushes `event`s to every connected client. Raw terminal input and output travel
//! in PTY frames (`0x02`), not here; `send_input` is for programmatic text (first prompt, Allow keys).

use crate::ids::{ProjectId, SessionId, SubAgentId};
use crate::status::Status;
use crate::types::*;
use serde::{Deserialize, Serialize};

/// Top-level control message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "body", rename_all = "snake_case")]
pub enum Message {
    Hello(Hello),
    HelloOk(HelloOk),
    Request(RequestEnvelope),
    Response(ResponseEnvelope),
    Event(Event),
}

/// First message of every connection. `token` is the content of `…/Lotse/token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub proto_version: u32,
    pub token: String,
    /// `app`, `hook`, `cli`, …
    pub client: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloOk {
    pub proto_version: u32,
    pub daemon_version: String,
}

/// Clients number requests from 1. `req_id: 0` is reserved for errors that answer no request
/// (a rejected `hello`: `unauthorized`, `version_mismatch`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub req_id: u64,
    pub request: Request,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub req_id: u64,
    pub response: Response,
}

/// Client → daemon. Every request gets exactly one response; any request may answer `Response::Error`.
///
/// | request | success response |
/// |---|---|
/// | `ping` | `pong` |
/// | `get_snapshot` | `snapshot` |
/// | `list_projects` | `projects` |
/// | `add_project`, `update_project` | `project` |
/// | `list_sessions` | `sessions` |
/// | `create_session` | `session` |
/// | `attach` | `attached` |
/// | `get_settings`, `update_settings` | `settings` |
/// | `detect_launch_commands` | `launch_commands` |
/// | `get_git_info` | `git_info` |
/// | everything else | `ok` |
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    Ping,
    /// Full state for the first frame → `Response::Snapshot`.
    GetSnapshot,

    // projects
    ListProjects,
    AddProject(NewProject),
    UpdateProject {
        project_id: ProjectId,
        patch: ProjectPatch,
    },
    RemoveProject {
        project_id: ProjectId,
    },

    // sessions
    ListSessions {
        project_id: Option<ProjectId>,
    },
    CreateSession(NewSession),
    RenameSession {
        session_id: SessionId,
        title: String,
    },
    /// Kill the PTY and remove the session.
    CloseSession {
        session_id: SessionId,
    },
    /// Re-run the session's last command (fail card "Retry").
    RetrySession {
        session_id: SessionId,
    },
    /// Mark read (clears the blue dot).
    MarkRead {
        session_id: SessionId,
    },
    /// Allow / Always / Deny on a permission card: the daemon types the matching option into the pane.
    /// `conflict` when the session is not at a prompt it can read, or the prompt has no such option.
    ResolvePermission {
        session_id: SessionId,
        decision: PermDecision,
    },

    // terminal
    /// Start streaming PTY frames for a session → `Response::Attached`; replay first, then live.
    Attach {
        session_id: SessionId,
        cols: u16,
        rows: u16,
    },
    Detach {
        session_id: SessionId,
    },
    /// Programmatic text input (raw keystrokes use PTY frames).
    SendInput {
        session_id: SessionId,
        text: String,
    },
    Resize {
        session_id: SessionId,
        cols: u16,
        rows: u16,
    },

    // settings
    GetSettings,
    /// Replaces the whole settings object.
    UpdateSettings {
        settings: Settings,
    },
    /// Settings › Agents: defaults, found on PATH, versions → `Response::LaunchCommands`.
    DetectLaunchCommands,

    // git
    GetGitInfo {
        project_id: ProjectId,
        session_id: Option<SessionId>,
    },

    /// From `lotse-hook`: an agent hook payload. Forwarded as received, except that `lotse-hook`
    /// strips `tool_response` (tool output can be huge and no adapter reads it).
    HookEvent {
        agent: AgentKind,
        session_id: Option<SessionId>,
        payload: serde_json::Value,
    },

    /// Ask the daemon to exit (sessions die with it in Alpha).
    Shutdown,
}

impl Request {
    /// Every method name, for fixture coverage.
    pub const METHODS: &'static [&'static str] = &[
        "ping",
        "get_snapshot",
        "list_projects",
        "add_project",
        "update_project",
        "remove_project",
        "list_sessions",
        "create_session",
        "rename_session",
        "close_session",
        "retry_session",
        "mark_read",
        "resolve_permission",
        "attach",
        "detach",
        "send_input",
        "resize",
        "get_settings",
        "update_settings",
        "detect_launch_commands",
        "get_git_info",
        "hook_event",
        "shutdown",
    ];

    /// Wire name of the method. The match is exhaustive: a new variant fails to compile here.
    pub fn method(&self) -> &'static str {
        match self {
            Request::Ping => "ping",
            Request::GetSnapshot => "get_snapshot",
            Request::ListProjects => "list_projects",
            Request::AddProject(_) => "add_project",
            Request::UpdateProject { .. } => "update_project",
            Request::RemoveProject { .. } => "remove_project",
            Request::ListSessions { .. } => "list_sessions",
            Request::CreateSession(_) => "create_session",
            Request::RenameSession { .. } => "rename_session",
            Request::CloseSession { .. } => "close_session",
            Request::RetrySession { .. } => "retry_session",
            Request::MarkRead { .. } => "mark_read",
            Request::ResolvePermission { .. } => "resolve_permission",
            Request::Attach { .. } => "attach",
            Request::Detach { .. } => "detach",
            Request::SendInput { .. } => "send_input",
            Request::Resize { .. } => "resize",
            Request::GetSettings => "get_settings",
            Request::UpdateSettings { .. } => "update_settings",
            Request::DetectLaunchCommands => "detect_launch_commands",
            Request::GetGitInfo { .. } => "get_git_info",
            Request::HookEvent { .. } => "hook_event",
            Request::Shutdown => "shutdown",
        }
    }
}

/// Daemon → client, answering one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Pong,
    Snapshot(Snapshot),
    Projects(Vec<Project>),
    Project(Project),
    Sessions(Vec<Session>),
    Session(Session),
    Attached(AttachInfo),
    Settings(Settings),
    LaunchCommands(Vec<LaunchCommandInfo>),
    GitInfo(GitInfo),
    Error(ProtoError),
}

impl Response {
    pub const KINDS: &'static [&'static str] = &[
        "ok",
        "pong",
        "snapshot",
        "projects",
        "project",
        "sessions",
        "session",
        "attached",
        "settings",
        "launch_commands",
        "git_info",
        "error",
    ];

    pub fn kind(&self) -> &'static str {
        match self {
            Response::Ok => "ok",
            Response::Pong => "pong",
            Response::Snapshot(_) => "snapshot",
            Response::Projects(_) => "projects",
            Response::Project(_) => "project",
            Response::Sessions(_) => "sessions",
            Response::Session(_) => "session",
            Response::Attached(_) => "attached",
            Response::Settings(_) => "settings",
            Response::LaunchCommands(_) => "launch_commands",
            Response::GitInfo(_) => "git_info",
            Response::Error(_) => "error",
        }
    }
}

/// Daemon → every client. Pushed state changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    ProjectAdded(Project),
    ProjectUpdated(Project),
    ProjectRemoved {
        project_id: ProjectId,
    },
    SessionCreated(Session),
    /// Title, unread, git or other metadata changed. Status has its own events.
    SessionUpdated(Session),
    SessionClosed {
        session_id: SessionId,
    },
    SessionExited {
        session_id: SessionId,
        exit_code: Option<i32>,
    },
    /// Session status changed. `perm` is set when `status == Perm`.
    StatusChanged {
        session_id: SessionId,
        status: Status,
        source: DetectSource,
        perm: Option<PermRequest>,
        running_since: Option<UnixMs>,
        at: UnixMs,
    },
    /// Full sub-agent list of a session (replaces the previous list).
    SubAgentsChanged {
        session_id: SessionId,
        sub_agents: Vec<SubAgent>,
    },
    /// One sub-agent changed (convenience for sub-agent done notifications).
    SubAgentStatus {
        session_id: SessionId,
        sub_agent_id: SubAgentId,
        status: Status,
    },
    GitInfoChanged {
        session_id: SessionId,
        git: GitInfo,
    },
    SettingsChanged(Settings),
    /// The daemon decided a notification is due (settings and per-project overrides applied).
    Notification {
        session_id: SessionId,
        kind: NotificationKind,
        /// `praxis-web · Claude Code`
        title: String,
        /// `Needs permission`
        subtitle: String,
        /// `Bash(npx playwright test e2e/patients)`
        body: Option<String>,
        sound: bool,
        at: UnixMs,
    },
}

impl Event {
    pub const NAMES: &'static [&'static str] = &[
        "project_added",
        "project_updated",
        "project_removed",
        "session_created",
        "session_updated",
        "session_closed",
        "session_exited",
        "status_changed",
        "sub_agents_changed",
        "sub_agent_status",
        "git_info_changed",
        "settings_changed",
        "notification",
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Event::ProjectAdded(_) => "project_added",
            Event::ProjectUpdated(_) => "project_updated",
            Event::ProjectRemoved { .. } => "project_removed",
            Event::SessionCreated(_) => "session_created",
            Event::SessionUpdated(_) => "session_updated",
            Event::SessionClosed { .. } => "session_closed",
            Event::SessionExited { .. } => "session_exited",
            Event::StatusChanged { .. } => "status_changed",
            Event::SubAgentsChanged { .. } => "sub_agents_changed",
            Event::SubAgentStatus { .. } => "sub_agent_status",
            Event::GitInfoChanged { .. } => "git_info_changed",
            Event::SettingsChanged(_) => "settings_changed",
            Event::Notification { .. } => "notification",
        }
    }
}
