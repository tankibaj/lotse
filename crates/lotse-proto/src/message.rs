//! Control messages (frame type `0x01`, JSON). Adjacently tagged so TypeScript can mirror them 1:1.
//!
//! Flow: client sends `hello` (token first), daemon answers `hello_ok`. Then the client sends
//! `request`s (each with a `req_id`), the daemon answers each with a `response` carrying the same
//! `req_id`, and pushes `event`s to every connected client. Raw terminal input and output travel
//! in PTY frames (`0x02`), not here; `send_input` is for programmatic text (first prompt, Allow keys).

use crate::ids::{ProjectId, SessionId, SubAgentId, WorkspaceId};
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
/// | `add_project`, `update_project`, `move_project` | `project` |
/// | `create_workspace`, `rename_workspace` | `workspace` |
/// | `list_sessions` | `sessions` |
/// | `create_session` | `session` |
/// | `attach` | `attached` |
/// | `get_settings`, `update_settings` | `settings` |
/// | `detect_launch_commands` | `launch_commands` |
/// | `get_git_info` | `git_info` |
/// | `get_diff` | `diff` |
/// | `get_transcript` | `transcript` |
/// | `get_review` | `review` |
/// | `list_files` | `file_tree` |
/// | `read_file`, `write_file` | `file` |
/// | `get_usage` | `usage` |
/// | `export_transcript` | `export` |
/// | `get_session_info` | `session_info` |
/// | `list_connectors`, `check_connectors` | `connectors` |
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
    /// Move a project to another workspace → `Response::Project`; also announced as `project_updated`.
    MoveProject {
        project_id: ProjectId,
        workspace_id: WorkspaceId,
    },

    // workspaces (since proto 3)
    /// `conflict` when the name is taken (names are unique, case-insensitive).
    CreateWorkspace {
        name: String,
    },
    RenameWorkspace {
        workspace_id: WorkspaceId,
        name: String,
    },
    /// Delete a workspace. The default workspace cannot be deleted. A workspace with projects is
    /// refused (`conflict`) unless `move_projects` moves them to the default workspace first.
    DeleteWorkspace {
        workspace_id: WorkspaceId,
        move_projects: bool,
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
    /// `conflict` when the session is not at a prompt it can read, or the prompt has no such option,
    /// or `prompt_id` is not the open prompt's `PermRequest::id` (a stale banner). `via` is recorded
    /// with the decision; both are optional and new in proto 4.
    ResolvePermission {
        session_id: SessionId,
        decision: PermDecision,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        via: Option<PermVia>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_id: Option<String>,
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
    /// Show Changes: `git diff` of the session's folder against `base` (default `HEAD`; staged and
    /// unstaged) plus the names of untracked files, capped in size (since proto 3; `base` since 4).
    GetDiff {
        session_id: SessionId,
        #[serde(default)]
        base: DiffBase,
    },
    /// Show Transcript: the last `limit` turns (default 50, at most 200) of the session's agent
    /// transcript. `unavailable` for agents without one (since proto 3).
    GetTranscript {
        session_id: SessionId,
        limit: Option<u32>,
    },

    // beta (since proto 4); every handler is `unavailable` until its work package lands
    /// Branch against its base: ahead, behind, the commits ahead → `Response::Review`.
    GetReview {
        session_id: SessionId,
    },
    /// Files of the session's folder (only the changed ones with `changed_only`) → `Response::FileTree`.
    ListFiles {
        session_id: SessionId,
        changed_only: bool,
    },
    /// A file of the session's folder, at `rev` (a git revision) or as it is on disk → `Response::File`.
    ReadFile {
        session_id: SessionId,
        path: String,
        rev: Option<String>,
    },
    /// Replace a file's text; `conflict` when `expected_sha` is not the file's current sha → `Response::File`.
    WriteFile {
        session_id: SessionId,
        path: String,
        text: String,
        expected_sha: Option<String>,
    },
    /// Time, tokens and spend of the session → `Response::Usage`.
    GetUsage {
        session_id: SessionId,
    },
    /// The session's transcript as a document, secrets masked → `Response::Export`.
    ExportTranscript {
        session_id: SessionId,
        format: ExportFormat,
    },
    /// Session panel data → `Response::SessionInfo`.
    GetSessionInfo {
        session_id: SessionId,
    },
    /// SIGTERM to a background task of the session (the pid must be one `get_session_info` listed).
    StopTask {
        session_id: SessionId,
        pid: u32,
    },
    /// MCP servers from the agents' config files → `Response::Connectors`.
    ListConnectors {
        session_id: SessionId,
    },
    /// Like `list_connectors`, with live status where `Settings::external.mcp_live` allows it.
    CheckConnectors {
        session_id: SessionId,
    },

    // mod and child sessions (since proto 5); every handler is `unavailable` until its work package lands
    /// The Claude mod's bridge (`lotse-hook mod-bridge`) attaches to the session: the daemon then
    /// pushes `Event::ModCommand`s for it on this connection → `Response::Ok`.
    ModAttach {
        session_id: SessionId,
    },
    /// Answer an `Event::SessionStartRequested` card: `approve` starts the child session → `Response::Ok`.
    /// `not_found` when `id` is not an open request.
    ResolveSessionStart {
        id: String,
        approve: bool,
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
        "move_project",
        "create_workspace",
        "rename_workspace",
        "delete_workspace",
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
        "get_diff",
        "get_transcript",
        "get_review",
        "list_files",
        "read_file",
        "write_file",
        "get_usage",
        "export_transcript",
        "get_session_info",
        "stop_task",
        "list_connectors",
        "check_connectors",
        "mod_attach",
        "resolve_session_start",
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
            Request::MoveProject { .. } => "move_project",
            Request::CreateWorkspace { .. } => "create_workspace",
            Request::RenameWorkspace { .. } => "rename_workspace",
            Request::DeleteWorkspace { .. } => "delete_workspace",
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
            Request::GetDiff { .. } => "get_diff",
            Request::GetTranscript { .. } => "get_transcript",
            Request::GetReview { .. } => "get_review",
            Request::ListFiles { .. } => "list_files",
            Request::ReadFile { .. } => "read_file",
            Request::WriteFile { .. } => "write_file",
            Request::GetUsage { .. } => "get_usage",
            Request::ExportTranscript { .. } => "export_transcript",
            Request::GetSessionInfo { .. } => "get_session_info",
            Request::StopTask { .. } => "stop_task",
            Request::ListConnectors { .. } => "list_connectors",
            Request::CheckConnectors { .. } => "check_connectors",
            Request::ModAttach { .. } => "mod_attach",
            Request::ResolveSessionStart { .. } => "resolve_session_start",
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
    Workspace(Workspace),
    Diff(GitDiff),
    Transcript(Transcript),
    Review(Review),
    FileTree(FileTree),
    File(File),
    Usage(Usage),
    Export(Export),
    SessionInfo(SessionInfo),
    Connectors(Vec<Connector>),
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
        "workspace",
        "diff",
        "transcript",
        "review",
        "file_tree",
        "file",
        "usage",
        "export",
        "session_info",
        "connectors",
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
            Response::Workspace(_) => "workspace",
            Response::Diff(_) => "diff",
            Response::Transcript(_) => "transcript",
            Response::Review(_) => "review",
            Response::FileTree(_) => "file_tree",
            Response::File(_) => "file",
            Response::Usage(_) => "usage",
            Response::Export(_) => "export",
            Response::SessionInfo(_) => "session_info",
            Response::Connectors(_) => "connectors",
            Response::Error(_) => "error",
        }
    }
}

fn banner_default() -> bool {
    true
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
    WorkspaceAdded(Workspace),
    WorkspaceUpdated(Workspace),
    /// Its projects (if any) were moved to the default workspace first, each announced as `project_updated`.
    WorkspaceRemoved {
        workspace_id: WorkspaceId,
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
    /// Today's tokens and cost changed (at most every 5 s).
    UsageChanged(UsageToday),
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
        /// Show a banner. `false` with `sound: true` is sound only. Since proto 4; a v3 event is a banner.
        #[serde(default = "banner_default")]
        banner: bool,
        sound: bool,
        /// macOS system sound to play (`EventPrefs::sound_name`). Since proto 4.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound_name: Option<String>,
        /// The `PermRequest::id` a `perm` notification is about; its Allow/Deny answers with it.
        /// Since proto 4.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_id: Option<String>,
        at: UnixMs,
    },
    /// To the connection that sent `mod_attach` for `session_id`: something for the mod to do.
    /// Since proto 5.
    ModCommand {
        session_id: SessionId,
        command: ModCommand,
    },
    /// An agent asked to start a child session (`lotse-hook mcp`); the app shows the full `prompt`
    /// on an approval card and answers with `resolve_session_start`. Since proto 5.
    SessionStartRequested {
        /// Identifies the request for `resolve_session_start`.
        id: String,
        /// The session whose agent asked.
        parent_session: SessionId,
        project_id: ProjectId,
        agent: AgentKind,
        title: Option<String>,
        prompt: String,
        worktree: Option<WorktreeSpec>,
        at: UnixMs,
    },
}

impl Event {
    pub const NAMES: &'static [&'static str] = &[
        "project_added",
        "project_updated",
        "project_removed",
        "workspace_added",
        "workspace_updated",
        "workspace_removed",
        "session_created",
        "session_updated",
        "session_closed",
        "session_exited",
        "status_changed",
        "sub_agents_changed",
        "sub_agent_status",
        "git_info_changed",
        "settings_changed",
        "usage_changed",
        "notification",
        "mod_command",
        "session_start_requested",
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Event::ProjectAdded(_) => "project_added",
            Event::ProjectUpdated(_) => "project_updated",
            Event::ProjectRemoved { .. } => "project_removed",
            Event::WorkspaceAdded(_) => "workspace_added",
            Event::WorkspaceUpdated(_) => "workspace_updated",
            Event::WorkspaceRemoved { .. } => "workspace_removed",
            Event::SessionCreated(_) => "session_created",
            Event::SessionUpdated(_) => "session_updated",
            Event::SessionClosed { .. } => "session_closed",
            Event::SessionExited { .. } => "session_exited",
            Event::StatusChanged { .. } => "status_changed",
            Event::SubAgentsChanged { .. } => "sub_agents_changed",
            Event::SubAgentStatus { .. } => "sub_agent_status",
            Event::GitInfoChanged { .. } => "git_info_changed",
            Event::SettingsChanged(_) => "settings_changed",
            Event::UsageChanged(_) => "usage_changed",
            Event::Notification { .. } => "notification",
            Event::ModCommand { .. } => "mod_command",
            Event::SessionStartRequested { .. } => "session_start_requested",
        }
    }
}
