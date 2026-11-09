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
    /// The Claude Code mod (in-process module). Since proto 5.
    Mod,
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

fn is_false(b: &bool) -> bool {
    !*b
}

/// Everything after `status` is new in proto 5 and absent until the mod (or a transcript) reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubAgent {
    pub id: SubAgentId,
    pub name: String,
    pub status: Status,
    /// The sub-agent that started this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<SubAgentId>,
    /// What it is doing now, e.g. `Read src/db/migrations`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    /// Display only: the daemon's totals (`Usage`, `UsageToday`) sum `cost_micros`, never this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Runs in the background (the lead does not wait for it).
    #[serde(default, skip_serializing_if = "is_false")]
    pub background: bool,
}

impl SubAgent {
    /// A sub-agent with only the proto 4 fields.
    pub fn new(id: SubAgentId, name: impl Into<String>, status: Status) -> Self {
        Self {
            id,
            name: name.into(),
            status,
            parent: None,
            activity: None,
            tokens: None,
            cost_usd: None,
            model: None,
            background: false,
        }
    }
}

/// Where a pull or merge request stands. Since proto 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

/// CI result of a pull request. Since proto 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrChecks {
    Pass,
    Fail,
    Pending,
}

/// Which CLI reported the pull request. Since proto 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrProvider {
    Github,
    Gitlab,
}

/// The pull (GitHub) or merge (GitLab) request of the session's branch. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrInfo {
    pub number: u64,
    pub url: String,
    pub state: PrState,
    /// `None` when the provider reports no checks.
    pub checks: Option<PrChecks>,
    pub provider: PrProvider,
}

/// Git line of a session card and the status bar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Commits the base has that the branch lacks (↓n); 0 hides the field. Since proto 4.
    #[serde(default)]
    pub behind: u32,
    /// Lines added / removed by the uncommitted changes (+n −n). Since proto 4.
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub removed: u32,
    /// The branch's pull request; only with `Settings::external.pr_status` on. Since proto 4.
    #[serde(default)]
    pub pr: Option<PrInfo>,
}

/// Which layer of the detection stack feeds a session's data. Since proto 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLayer {
    Mod,
    Hooks,
    Screen,
}

/// Progress of the agent's todo list. Since proto 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Share of the context window in use, 0–100. Since proto 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_pct: Option<u8>,
    /// Which layer feeds this session's data (the card's source tooltip). Since proto 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_layer: Option<SourceLayer>,
    /// The session whose agent started this one. Since proto 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<SessionId>,
    /// Since proto 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo: Option<Todo>,
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
    /// Identifies this prompt (set by the daemon, a new one per prompt). `ResolvePermission`
    /// carries it back as `prompt_id`, so a stale answer cannot hit a newer prompt. Since proto 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Where a permission answer came from (`perm_events.via`). Since proto 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermVia {
    Sidebar,
    Notification,
    Palette,
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
    /// The session whose agent starts this one (a child session). Since proto 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session: Option<SessionId>,
    /// An agent asked for this session (through `lotse-hook mcp`), not the user. Since proto 5.
    #[serde(default, skip_serializing_if = "is_false")]
    pub requested_by_agent: bool,
}

/// What the daemon asks the Claude mod to do (`Event::ModCommand`). Since proto 5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ModCommand {
    /// Submit `text` as the user's prompt.
    Submit { text: String },
    /// Answer the permission prompt `prompt_id` (`PermRequest::id`).
    Answer {
        prompt_id: Option<String>,
        decision: PermDecision,
    },
    /// Liveness check; the mod's heartbeat answers it.
    Ping,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventPrefs {
    pub banner: bool,
    pub sound: bool,
    /// Allow/Deny inside the banner; Beta, kept so the setting round-trips.
    pub allow_deny: bool,
    /// macOS system sound (`Glass`, `Ping`, …); `None` plays the default. Since proto 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound_name: Option<String>,
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
            sound_name: None,
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

/// Settings › External: what the daemon may ask other programs. Both off until the user opts in.
/// Since proto 4.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalSettings {
    /// Run `gh` / `glab` for `GitInfo::pr`.
    #[serde(default)]
    pub pr_status: bool,
    /// Run `claude mcp list` / `codex mcp list` for `check_connectors`.
    #[serde(default)]
    pub mcp_live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub launch_commands: LaunchCommands,
    pub notifications: NotificationSettings,
    pub appearance: AppearanceSettings,
    /// Per-session raw byte ring buffer, replayed on attach. Default 4 MiB.
    pub scrollback_bytes: u64,
    /// Since proto 4.
    #[serde(default)]
    pub external: ExternalSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launch_commands: LaunchCommands::default(),
            notifications: NotificationSettings::default(),
            appearance: AppearanceSettings::default(),
            scrollback_bytes: 4 * 1024 * 1024,
            external: ExternalSettings::default(),
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
    /// Lines added / removed in this file (0 for binary files). Since proto 4.
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub removed: u32,
}

/// What `get_diff` compares the working tree with. Since proto 4.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffBase {
    /// `HEAD`: uncommitted changes (Show Changes).
    #[default]
    Head,
    /// The merge-base with the default branch: everything the branch adds (review).
    MergeBase,
}

/// Answer to `get_diff`: the session folder's changes against the base plus untracked names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitDiff {
    /// `false`: the folder is not in a git repository ("No git").
    pub is_git: bool,
    pub files: Vec<DiffFile>,
    /// The total size cap was hit: files after the last one are left out.
    pub truncated: bool,
}

/// One commit of `Review::commits`. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCommit {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub at: UnixMs,
}

/// Answer to `get_review`: how the session's branch stands against its base. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    /// The ref the branch is compared with (`origin/main`, `main`, `HEAD`).
    pub base: String,
    pub ahead: u32,
    pub behind: u32,
    /// Newest first, the commits ahead of the base.
    pub commits: Vec<ReviewCommit>,
    /// More commits than listed.
    pub truncated: bool,
}

/// One entry of `FileTree::entries`. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative to `FileTree::root`, `/` separated.
    pub path: String,
    pub dir: bool,
    /// `None`: unchanged (or not a git folder).
    pub status: Option<DiffStatus>,
    pub added: u32,
    pub removed: u32,
}

/// Answer to `list_files`. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTree {
    /// Absolute folder of the session; every path is confined to it.
    pub root: String,
    pub is_git: bool,
    pub entries: Vec<FileEntry>,
    /// More entries than listed.
    pub truncated: bool,
}

/// Answer to `read_file` and `write_file`. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub path: String,
    /// Empty for a binary file.
    pub text: String,
    /// SHA-256 (hex) of the file's bytes: `write_file` passes it back as `expected_sha`.
    pub sha: String,
    /// Not UTF-8 text: shown read-only.
    pub binary: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    User,
    Assistant,
    /// A tool call the agent made (`tool` is set, `text` is empty).
    Tool,
    /// A permission decision (`approval` is set, `text` is empty). Since proto 4.
    Approval,
}

/// How a tool call ended. Since proto 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Run,
    Done,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptTool {
    /// `Bash`, `Edit`, …
    pub name: String,
    /// Short, e.g. the command or file path.
    pub summary: String,
    /// Since proto 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ToolStatus>,
    /// Short excerpt of the tool's output. Since proto 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// A permission decision inside the transcript. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptApproval {
    pub decision: PermDecision,
    /// `None` when the decision was made in the terminal.
    pub via: Option<PermVia>,
    /// What was asked, e.g. `Bash(npx playwright test)`.
    pub command: String,
}

/// One turn of the agent's conversation. Since proto 3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptTurn {
    pub role: TranscriptRole,
    pub text: String,
    pub tool: Option<TranscriptTool>,
    pub at: Option<UnixMs>,
    /// Since proto 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<TranscriptApproval>,
    /// Name of the sub-agent that took this turn. Since proto 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_agent: Option<String>,
}

/// Where a transcript was read from. Since proto 4.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSource {
    #[default]
    Claude,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    /// The session's terminal scrollback (no agent transcript).
    Scrollback,
}

/// Answer to `get_transcript`: the last turns of the agent's own transcript file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    /// `false`: the agent has not reported a transcript file yet (no hook has fired).
    pub available: bool,
    pub turns: Vec<TranscriptTurn>,
    /// Older turns were left out (count or size cap).
    pub truncated: bool,
    /// Since proto 4.
    #[serde(default)]
    pub source: TranscriptSource,
}

/// Format of `export_transcript`. Since proto 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Markdown,
    JsonLines,
    Text,
}

/// Answer to `export_transcript`. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Export {
    pub file_name: String,
    pub content: String,
    /// Secrets masked in `content`.
    pub redactions: u32,
}

/// One stretch of a lane in `Usage` (the session's status over time).
/// Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageSpan {
    pub status: Status,
    pub from: UnixMs,
    pub to: UnixMs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneKind {
    /// The session's own agent.
    Lead,
    SubAgent,
}

/// One row of the usage timeline. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageLane {
    pub name: String,
    pub kind: LaneKind,
    pub spans: Vec<UsageSpan>,
    /// Millionths of a US dollar.
    pub cost_micros: u64,
}

/// Token counts of a session. Since proto 4.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageTokens {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}

/// Spend in millionths of a US dollar: this session, its project, everything. Since proto 4.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageSpend {
    pub session: u64,
    pub project: u64,
    pub all: u64,
}

/// Answer to `get_usage`: time and tokens of one session. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub started_at: UnixMs,
    pub total_ms: u64,
    /// Time in `run`.
    pub working_ms: u64,
    /// Time in `perm`.
    pub waiting_ms: u64,
    pub lanes: Vec<UsageLane>,
    pub tokens: UsageTokens,
    pub spend: UsageSpend,
    pub model: Option<String>,
    /// `false`: a model without a known price took part, the spend covers the priced part only.
    pub priced: bool,
}

/// A background process of a session (Session panel › Tasks). Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub pid: u32,
    pub command: String,
    pub started_at: UnixMs,
}

/// Answer to `get_session_info`. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub agent_version: Option<String>,
    pub model: Option<String>,
    /// `scutil --get ComputerName`.
    pub machine: String,
    pub cwd: String,
    pub worktree: Option<String>,
    /// Which source set the session's status.
    pub source: DetectSource,
    pub tasks: Vec<BackgroundTask>,
}

/// State of one MCP connector. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectorStatus {
    /// Found in a config file; not checked live.
    Configured,
    Connected {
        tools: u32,
    },
    NeedsAuth,
    Failed {
        reason: String,
    },
    Unknown,
}

/// One MCP server of an agent's configuration. Since proto 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connector {
    pub agent: AgentKind,
    pub name: String,
    /// `stdio`, `http`, `sse`, …
    pub transport: String,
    /// The file that declares it.
    pub config_path: String,
    pub status: ConnectorStatus,
}

/// One rate-limit window of the agent's plan (the status bar meter). Since proto 5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimit {
    /// `five_hour`, `seven_day`, …
    pub kind: String,
    /// Share of the window used, 0–100.
    pub pct: u8,
    pub resets_at: UnixMs,
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
    /// Empty until the mod reports them. Since proto 5.
    ///
    /// The transcript tracker rebuilds the token and cost fields on every refresh; it must carry
    /// the last reported `rate_limits` over into each new `UsageToday`, or every refresh wipes them
    /// (obligation for M2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rate_limits: Vec<RateLimit>,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
