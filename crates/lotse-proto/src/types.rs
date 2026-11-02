//! Domain types carried by messages. Shapes follow the data model in `spec/mock.html` (`P`).

use crate::ids::{ProjectId, SessionId, SubAgentId, WorkspaceId};
use crate::status::Status;
use serde::{Deserialize, Serialize};

/// Unix time in milliseconds.
pub type UnixMs = u64;

/// What runs in a session. `label()` is the text the UI shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Terminal,
    ClaudeCode,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
}

impl AgentKind {
    pub const ALL: [AgentKind; 4] = [
        AgentKind::Terminal,
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::OpenCode,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Terminal => "Terminal",
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::OpenCode => "OpenCode",
        }
    }

    /// Default launch command (mock: Settings › Agents placeholders).
    pub fn default_command(self) -> &'static str {
        match self {
            AgentKind::Terminal => "$SHELL -l",
            AgentKind::ClaudeCode => "claude",
            AgentKind::Codex => "codex",
            AgentKind::OpenCode => "opencode",
        }
    }
}

/// Which detection source produced a status (order of precedence, strongest first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectSource {
    Hook,
    Osc7501,
    Screen,
    Process,
}

/// Per-project notification override (mock: project context menu › Notifications).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum NotificationMode {
    /// Follow Settings › Notifications.
    Global,
    /// Never notify for this project.
    Mute,
    /// Per-event choice.
    Custom {
        perm: bool,
        fail: bool,
        done: bool,
        sub_agent_done: bool,
    },
}

/// Groups projects in the sidebar (Workspace › Project › Agent › Sub-agent). Since proto 3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub path: String,
    /// Swatch key from the mock palette (`red`, `purple`, …).
    pub color: String,
    /// Lucide icon name (SF Symbols helper may map it later).
    pub icon: String,
    pub is_git: bool,
    pub notifications: NotificationMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubAgent {
    pub id: SubAgentId,
    pub name: String,
    pub status: Status,
}

/// Git line of a session card and the status bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInfo {
    /// `false` renders "No git".
    pub is_git: bool,
    /// Branch name, empty when not a git repo or detached.
    pub branch: String,
    /// Commits ahead (↑n); 0 hides the field.
    pub ahead: u32,
    /// Uncommitted changes (±n); 0 hides the field.
    pub dirty: u32,
    /// Isolated worktree (true) or main checkout (false).
    pub worktree: bool,
    /// Absolute worktree path when `worktree` is true.
    pub worktree_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub project_id: ProjectId,
    pub title: String,
    pub agent: AgentKind,
    /// The command line the session was started with.
    pub command: String,
    pub status: Status,
    pub sub_agents: Vec<SubAgent>,
    pub git: GitInfo,
    pub created_at: UnixMs,
    /// Last PTY or status activity; the card shows it as "2m", "1h".
    pub last_activity: UnixMs,
    /// Set while `status == Run`; the card shows the elapsed time.
    pub running_since: Option<UnixMs>,
    /// Finished while not focused: blue dot on the card.
    pub unread: bool,
    /// Pending permission request when `status == Perm`.
    pub perm: Option<PermRequest>,
    /// Which source set the current status.
    pub source: DetectSource,
}

/// What a permission prompt asks for (banner and card text).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermRequest {
    pub tool: String,
    /// e.g. `npx playwright test e2e/patients`.
    pub summary: String,
    /// What the card's Always button does on the prompt on screen; absent until the daemon has
    /// read the prompt (the card then shows a plain Always). Since proto 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always: Option<PermAlways>,
}

/// The option the Always button picks on the prompt on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermAlways {
    /// "Yes, and don't ask again …" (or "… this project / this session").
    Remember,
    /// "Yes, and switch to auto mode": the session stops asking (card label "Auto").
    AutoMode,
    /// No such option: the card hides Always.
    Unavailable,
}

/// Sidebar buttons on a permission card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermDecision {
    Allow,
    Always,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeSpec {
    /// New branch, e.g. `lotse/new-task`.
    pub branch: String,
}

/// "New session" sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSession {
    pub project_id: ProjectId,
    pub agent: AgentKind,
    /// Overrides the configured launch command.
    pub command: Option<String>,
    pub title: Option<String>,
    pub worktree: Option<WorktreeSpec>,
    pub first_prompt: Option<String>,
}

/// "Add a project" sheet. Browse and drag-drop use `path`; the other tabs need git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewProject {
    pub path: String,
    pub name: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
}

/// Fields that can change on a project (`None` = keep).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPatch {
    pub name: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub notifications: Option<NotificationMode>,
}

/// Row of Settings › Agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchCommandInfo {
    pub agent: AgentKind,
    /// The built-in default (the input's placeholder and Reset target).
    pub default_command: String,
    /// The configured command.
    pub command: String,
    /// `false` renders "not found on PATH".
    pub found: bool,
    pub resolved_path: Option<String>,
    /// e.g. `zsh 5.9`, `v2.1.309`.
    pub version: Option<String>,
    /// e.g. `hooks installed`, `OSC 7501`.
    pub integration: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchCommands {
    pub terminal: String,
    pub claude_code: String,
    pub codex: String,
    pub opencode: String,
}

impl Default for LaunchCommands {
    fn default() -> Self {
        Self {
            terminal: AgentKind::Terminal.default_command().into(),
            claude_code: AgentKind::ClaudeCode.default_command().into(),
            codex: AgentKind::Codex.default_command().into(),
            opencode: AgentKind::OpenCode.default_command().into(),
        }
    }
}

/// One row of Settings › Notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventPrefs {
    pub banner: bool,
    pub sound: bool,
    /// Allow/Deny inside the banner; Beta, kept so the setting round-trips.
    pub allow_deny: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationSettings {
    pub perm: EventPrefs,
    pub fail: EventPrefs,
    pub done: EventPrefs,
    pub sub_agent_done: EventPrefs,
    /// "Only when Lotse is not focused on that session".
    pub only_when_unfocused: bool,
    /// Footer bell: pause all notifications.
    pub paused: bool,
}

impl Default for NotificationSettings {
    /// Defaults from the mock's Settings › Notifications table.
    fn default() -> Self {
        let on = |banner, sound, allow_deny| EventPrefs {
            banner,
            sound,
            allow_deny,
        };
        Self {
            perm: on(true, true, true),
            fail: on(true, true, false),
            done: on(true, false, false),
            sub_agent_done: on(false, false, false),
            only_when_unfocused: true,
            paused: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceSettings {
    pub theme: Theme,
    pub interface_font: String,
    pub interface_font_size: u32,
    pub terminal_font: String,
    pub terminal_font_size: u32,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: Theme::Auto,
            interface_font: "System (SF Pro)".into(),
            interface_font_size: 13,
            terminal_font: "SF Mono".into(),
            terminal_font_size: 13,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub launch_commands: LaunchCommands,
    pub notifications: NotificationSettings,
    pub appearance: AppearanceSettings,
    /// Per-session raw byte ring buffer, replayed on attach. Default 4 MiB.
    pub scrollback_bytes: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launch_commands: LaunchCommands::default(),
            notifications: NotificationSettings::default(),
            appearance: AppearanceSettings::default(),
            scrollback_bytes: 4 * 1024 * 1024,
        }
    }
}

/// Which toggle of Settings › Notifications fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    Perm,
    Fail,
    Done,
    SubAgentDone,
}

/// Daemon facts for the status bar ("● lotsed running · 6 sessions").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub version: String,
    pub proto_version: u32,
    pub pid: u32,
    pub started_at: UnixMs,
}

/// How a file differs from `HEAD` in `GetDiff` (staged and unstaged together).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    /// Not tracked: listed by name, no diff text.
    Untracked,
}

/// One file of a session's `git diff`. Since proto 3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFile {
    pub path: String,
    /// Previous path of a rename.
    pub old_path: Option<String>,
    pub status: DiffStatus,
    /// Git reports "Binary files … differ": noted, no diff text.
    pub binary: bool,
    /// The text below was cut at the per-file or total size cap.
    pub truncated: bool,
    /// Unified diff of this file (headers and hunks); empty for untracked and binary files.
    pub diff: String,
}

/// Answer to `get_diff`: the session folder's changes against `HEAD` plus untracked names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitDiff {
    /// `false`: the folder is not in a git repository ("No git").
    pub is_git: bool,
    pub files: Vec<DiffFile>,
    /// The total size cap was hit: files after the last one are left out.
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    User,
    Assistant,
    /// A tool call the agent made (`tool` is set, `text` is empty).
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptTool {
    /// `Bash`, `Edit`, …
    pub name: String,
    /// Short, e.g. the command or file path.
    pub summary: String,
}

/// One turn of the agent's conversation. Since proto 3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptTurn {
    pub role: TranscriptRole,
    pub text: String,
    pub tool: Option<TranscriptTool>,
    pub at: Option<UnixMs>,
}

/// Answer to `get_transcript`: the last turns of the agent's own transcript file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    /// `false`: the agent has not reported a transcript file yet (no hook has fired).
    pub available: bool,
    pub turns: Vec<TranscriptTurn>,
    /// Older turns were left out (count or size cap).
    pub truncated: bool,
}

/// Tokens and cost of today (local time), summed over the Claude Code transcripts the daemon knows.
/// Since proto 3.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageToday {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// Millionths of a US dollar; `None` when a model without a known price took part ("tokens only").
    pub cost_micros: Option<u64>,
}

impl UsageToday {
    /// All four kinds of tokens: the number the status bar shows.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }
}

/// Everything a client needs to draw the first frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub daemon: DaemonInfo,
    /// In creation order; the first is the default workspace.
    pub workspaces: Vec<Workspace>,
    pub projects: Vec<Project>,
    pub sessions: Vec<Session>,
    pub settings: Settings,
    pub usage: UsageToday,
}

/// Answer to `attach`. PTY frames follow: first the replay of `replay_len` bytes, then live output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachInfo {
    pub session_id: SessionId,
    pub cols: u16,
    pub rows: u16,
    pub replay_len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    VersionMismatch,
    InvalidRequest,
    NotFound,
    Conflict,
    Unavailable,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtoError {
    pub code: ErrorCode,
    pub message: String,
}
