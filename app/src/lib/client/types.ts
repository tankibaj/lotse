// Mirror of crates/lotse-proto (PROTO_VERSION 5). Frozen after WP0: change only with an
// orchestrator-approved proto bump. JSON fixtures in crates/lotse-proto/fixtures are the shared contract.

export const PROTO_VERSION = 5;

// ids (opaque strings minted by the daemon)
export type WorkspaceId = string;
export type ProjectId = string;
export type SessionId = string;
export type SubAgentId = string;
/** Unix time in milliseconds. */
export type UnixMs = number;

export type Status = 'run' | 'perm' | 'fail' | 'done' | 'idle' | 'unknown';
export type AgentKind = 'terminal' | 'claude_code' | 'codex' | 'opencode';
/** `mod` is the Claude Code mod, the strongest source (proto 5). */
export type DetectSource = 'mod' | 'hook' | 'osc7501' | 'screen' | 'process';
/** Which layer feeds a session's data (proto 5). */
export type SourceLayer = 'mod' | 'hooks' | 'screen';
export type Theme = 'light' | 'dark' | 'auto';
export type PermDecision = 'allow' | 'always' | 'deny';
/** What Always picks on the prompt on screen: remember = "don't ask again", auto_mode = "switch to auto mode". */
export type PermAlways = 'remember' | 'auto_mode' | 'unavailable';
/** Where a permission answer came from (`perm_events.via`, proto 4). */
export type PermVia = 'sidebar' | 'notification' | 'palette';
export type NotificationKind = 'perm' | 'fail' | 'done' | 'sub_agent_done';
export type ErrorCode =
  | 'unauthorized'
  | 'version_mismatch'
  | 'invalid_request'
  | 'not_found'
  | 'conflict'
  | 'unavailable'
  | 'internal';

export type NotificationMode =
  | { mode: 'global' }
  | { mode: 'mute' }
  | { mode: 'custom'; perm: boolean; fail: boolean; done: boolean; sub_agent_done: boolean };

/** Groups projects in the sidebar (proto 3). The first workspace is the default one. */
export interface Workspace {
  id: WorkspaceId;
  name: string;
}

export interface Project {
  id: ProjectId;
  workspace_id: WorkspaceId;
  name: string;
  path: string;
  /** Swatch key from the mock palette (`red`, `purple`, …). */
  color: string;
  /** Lucide icon name. */
  icon: string;
  is_git: boolean;
  notifications: NotificationMode;
}

export interface SubAgent {
  id: SubAgentId;
  name: string;
  status: Status;
  // Proto 5. Absent until the mod (or a transcript) reports them.
  /** The sub-agent that started this one. */
  parent?: SubAgentId;
  /** What it is doing now, e.g. `Read src/db/migrations`. */
  activity?: string;
  tokens?: number;
  cost_usd?: number;
  model?: string;
  /** Runs in the background; absent means `false`. */
  background?: boolean;
}

/** Progress of the agent's todo list (proto 5). */
export interface Todo {
  done: number;
  total: number;
}

export type PrState = 'open' | 'draft' | 'merged' | 'closed';
export type PrChecks = 'pass' | 'fail' | 'pending';
export type PrProvider = 'github' | 'gitlab';

/** The pull (GitHub) or merge (GitLab) request of the session's branch (proto 4). */
export interface PrInfo {
  number: number;
  url: string;
  state: PrState;
  /** `null` when the provider reports no checks. */
  checks: PrChecks | null;
  provider: PrProvider;
}

export interface GitInfo {
  /** `false` renders "No git". */
  is_git: boolean;
  branch: string;
  ahead: number;
  dirty: number;
  worktree: boolean;
  worktree_path: string | null;
  // Proto 4. The daemon always sends these; optional here so proto 3 test data still type-checks.
  /** Commits the base has that the branch lacks (↓n). */
  behind?: number;
  /** Lines added / removed by the uncommitted changes. */
  added?: number;
  removed?: number;
  /** Only with `Settings.external.pr_status` on. */
  pr?: PrInfo | null;
}

export interface PermRequest {
  tool: string;
  summary: string;
  /** Absent until the daemon has read the prompt (proto 2). */
  always?: PermAlways;
  /** Identifies this prompt; `resolvePermission` passes it back as `promptId`, so a stale answer cannot hit a newer prompt (proto 4). */
  id?: string;
}

export interface Session {
  id: SessionId;
  project_id: ProjectId;
  title: string;
  agent: AgentKind;
  command: string;
  status: Status;
  sub_agents: SubAgent[];
  git: GitInfo;
  created_at: UnixMs;
  last_activity: UnixMs;
  running_since: UnixMs | null;
  unread: boolean;
  perm: PermRequest | null;
  source: DetectSource;
  // Proto 5. Absent until the mod (or a hook) reports them.
  /** Share of the context window in use, 0–100. */
  context_pct?: number;
  /** Which layer feeds this session's data (the card's source tooltip). */
  source_layer?: SourceLayer;
  /** The session whose agent started this one. */
  parent_session?: SessionId;
  todo?: Todo;
}

export interface WorktreeSpec {
  branch: string;
}

export interface NewSession {
  project_id: ProjectId;
  agent: AgentKind;
  command: string | null;
  title: string | null;
  worktree: WorktreeSpec | null;
  first_prompt: string | null;
  /** The session whose agent starts this one (a child session; proto 5). */
  parent_session?: SessionId;
  /** An agent asked for this session, not the user (proto 5). */
  requested_by_agent?: boolean;
}

/** What the daemon asks the Claude mod to do (`mod_command`, proto 5). */
export type ModCommand =
  | { cmd: 'submit'; text: string }
  | { cmd: 'answer'; prompt_id: string | null; decision: PermDecision }
  | { cmd: 'ping' };

export interface NewProject {
  path: string;
  name: string | null;
  color: string | null;
  icon: string | null;
}

export interface ProjectPatch {
  name: string | null;
  color: string | null;
  icon: string | null;
  notifications: NotificationMode | null;
}

export interface LaunchCommandInfo {
  agent: AgentKind;
  default_command: string;
  command: string;
  found: boolean;
  resolved_path: string | null;
  version: string | null;
  integration: string | null;
}

export interface LaunchCommands {
  terminal: string;
  claude_code: string;
  codex: string;
  opencode: string;
}

export interface EventPrefs {
  banner: boolean;
  sound: boolean;
  allow_deny: boolean;
}

/**
 * `EventPrefs` plus the macOS system sound (proto 4). A separate type so `keyof EventPrefs` stays the three
 * toggles Settings › Notifications iterates over.
 */
export interface EventSettings extends EventPrefs {
  /** `Glass`, `Ping`, …; absent plays the default. */
  sound_name?: string;
}

export interface NotificationSettings {
  perm: EventSettings;
  fail: EventSettings;
  done: EventSettings;
  sub_agent_done: EventSettings;
  only_when_unfocused: boolean;
  paused: boolean;
}

export interface AppearanceSettings {
  theme: Theme;
  interface_font: string;
  interface_font_size: number;
  terminal_font: string;
  terminal_font_size: number;
}

/** Settings › External: what the daemon may ask other programs. Both off until the user opts in (proto 4). */
export interface ExternalSettings {
  /** Run `gh` / `glab` for `GitInfo.pr`. */
  pr_status: boolean;
  /** Run `claude mcp list` / `codex mcp list` for `checkConnectors`. */
  mcp_live: boolean;
}

export interface Settings {
  launch_commands: LaunchCommands;
  notifications: NotificationSettings;
  appearance: AppearanceSettings;
  scrollback_bytes: number;
  external: ExternalSettings;
}

export interface DaemonInfo {
  version: string;
  proto_version: number;
  pid: number;
  started_at: UnixMs;
}

export type DiffStatus = 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked';

/** One file of a session's `git diff` (proto 3). */
export interface DiffFile {
  path: string;
  /** Previous path of a rename. */
  old_path: string | null;
  status: DiffStatus;
  /** "Binary files … differ": noted, no diff text. */
  binary: boolean;
  /** The text was cut at the per-file or total size cap. */
  truncated: boolean;
  /** Unified diff of this file; empty for untracked and binary files. */
  diff: string;
  // Proto 4. The daemon always sends these; optional here so proto 3 test data still type-checks.
  /** Lines added / removed in this file; 0 for binary files. */
  added?: number;
  removed?: number;
}

/** What `getDiff` compares the working tree with (proto 4). */
export type DiffBase = 'head' | 'merge_base';

/** Answer to `get_diff`: changes against the base (`head` by default; staged and unstaged) plus untracked names. */
export interface GitDiff {
  /** `false`: not a git repository. */
  is_git: boolean;
  files: DiffFile[];
  /** The total size cap was hit: later files are left out. */
  truncated: boolean;
}

/** One commit of `Review.commits` (proto 4). */
export interface ReviewCommit {
  sha: string;
  subject: string;
  author: string;
  at: UnixMs;
}

/** Answer to `get_review`: how the session's branch stands against its base (proto 4). */
export interface Review {
  /** The ref the branch is compared with (`origin/main`, `main`, `HEAD`). */
  base: string;
  ahead: number;
  behind: number;
  /** Newest first, the commits ahead of the base. */
  commits: ReviewCommit[];
  /** More commits than listed. */
  truncated: boolean;
}

/** One entry of `FileTree.entries` (proto 4). */
export interface FileEntry {
  /** Relative to `FileTree.root`, `/` separated. */
  path: string;
  dir: boolean;
  /** `null`: unchanged (or not a git folder). */
  status: DiffStatus | null;
  added: number;
  removed: number;
}

/** Answer to `list_files` (proto 4). */
export interface FileTree {
  /** Absolute folder of the session; every path is confined to it. */
  root: string;
  is_git: boolean;
  entries: FileEntry[];
  /** More entries than listed. */
  truncated: boolean;
}

/** Answer to `read_file` and `write_file` (proto 4). Rust: `File`, renamed to stay clear of the DOM `File`. */
export interface FileContent {
  path: string;
  /** Empty for a binary file. */
  text: string;
  /** SHA-256 (hex) of the file's bytes: `writeFile` passes it back as `expectedSha`. */
  sha: string;
  /** Not UTF-8 text: shown read-only. */
  binary: boolean;
  size: number;
}

export type TranscriptRole = 'user' | 'assistant' | 'tool' | 'approval';
export type ToolStatus = 'run' | 'done' | 'fail';

export interface TranscriptTool {
  name: string;
  summary: string;
  status?: ToolStatus;
  /** Short excerpt of the tool's output. */
  result?: string;
}

/** A permission decision inside the transcript (proto 4). */
export interface TranscriptApproval {
  decision: PermDecision;
  /** `null` when the decision was made in the terminal. */
  via: PermVia | null;
  /** What was asked, e.g. `Bash(npx playwright test)`. */
  command: string;
}

export interface TranscriptTurn {
  role: TranscriptRole;
  text: string;
  tool: TranscriptTool | null;
  at: UnixMs | null;
  /** Set when `role` is `approval` (proto 4). */
  approval?: TranscriptApproval;
  /** Name of the sub-agent that took this turn (proto 4). */
  sub_agent?: string;
}

/** Where a transcript was read from (proto 4). */
export type TranscriptSource = 'claude' | 'codex' | 'opencode' | 'scrollback';

/** Answer to `get_transcript`: the last turns of the agent's transcript. */
export interface Transcript {
  /** `false`: the agent has not reported a transcript file yet. */
  available: boolean;
  turns: TranscriptTurn[];
  /** Older turns were left out. */
  truncated: boolean;
  /** Proto 4. The daemon always sends it; optional here so proto 3 test data still type-checks. */
  source?: TranscriptSource;
}

/** Format of `exportTranscript` (proto 4). */
export type ExportFormat = 'markdown' | 'json_lines' | 'text';

/** Answer to `export_transcript` (proto 4). */
export interface Export {
  file_name: string;
  content: string;
  /** Secrets masked in `content`. */
  redactions: number;
}

/** One stretch of a lane in `Usage`: the session's status over time (proto 4). */
export interface UsageSpan {
  status: Status;
  from: UnixMs;
  to: UnixMs;
}

export type LaneKind = 'lead' | 'sub_agent';

/** One row of the usage timeline (proto 4). */
export interface UsageLane {
  name: string;
  kind: LaneKind;
  spans: UsageSpan[];
  /** Millionths of a US dollar. */
  cost_micros: number;
}

export interface UsageTokens {
  input: number;
  cache_read: number;
  cache_write: number;
  output: number;
}

/** Spend in millionths of a US dollar: this session, its project, everything. */
export interface UsageSpend {
  session: number;
  project: number;
  all: number;
}

/** Answer to `get_usage`: time and tokens of one session (proto 4). */
export interface Usage {
  started_at: UnixMs;
  total_ms: number;
  /** Time in `run`. */
  working_ms: number;
  /** Time in `perm`. */
  waiting_ms: number;
  lanes: UsageLane[];
  tokens: UsageTokens;
  spend: UsageSpend;
  model: string | null;
  /** `false`: a model without a known price took part, the spend covers the priced part only. */
  priced: boolean;
}

/** A background process of a session (Session panel › Tasks, proto 4). */
export interface BackgroundTask {
  pid: number;
  command: string;
  started_at: UnixMs;
}

/** Answer to `get_session_info` (proto 4). */
export interface SessionInfo {
  agent_version: string | null;
  model: string | null;
  /** `scutil --get ComputerName`. */
  machine: string;
  cwd: string;
  worktree: string | null;
  /** Which source set the session's status. */
  source: DetectSource;
  tasks: BackgroundTask[];
}

/** State of one MCP connector (proto 4). `configured`: found in a config file, not checked live. */
export type ConnectorStatus =
  | { state: 'configured' }
  | { state: 'connected'; tools: number }
  | { state: 'needs_auth' }
  | { state: 'failed'; reason: string }
  | { state: 'unknown' };

/** One MCP server of an agent's configuration (proto 4). */
export interface Connector {
  agent: AgentKind;
  name: string;
  /** `stdio`, `http`, `sse`, … */
  transport: string;
  /** The file that declares it. */
  config_path: string;
  status: ConnectorStatus;
}

/** One rate-limit window of the agent's plan (proto 5). */
export interface RateLimit {
  /** `five_hour`, `seven_day`, … */
  kind: string;
  /** Share of the window used, 0–100. */
  pct: number;
  resets_at: UnixMs;
}

/** Today's tokens and cost (local day), from the Claude Code transcripts the daemon knows. */
export interface UsageToday {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  /** Millionths of a US dollar; `null` when a model without a known price took part ("tokens only"). */
  cost_micros: number | null;
  /** Absent until the mod reports them (proto 5). */
  rate_limits?: RateLimit[];
}

export interface Snapshot {
  daemon: DaemonInfo;
  workspaces: Workspace[];
  projects: Project[];
  sessions: Session[];
  settings: Settings;
  usage: UsageToday;
}

export interface AttachInfo {
  session_id: SessionId;
  cols: number;
  rows: number;
  replay_len: number;
}

export interface ProtoError {
  code: ErrorCode;
  message: string;
}

// messages ------------------------------------------------------------------------------------

export interface Hello {
  proto_version: number;
  token: string;
  client: string;
}

export interface HelloOk {
  proto_version: number;
  daemon_version: string;
}

export type Request =
  | { method: 'ping' }
  | { method: 'get_snapshot' }
  | { method: 'list_projects' }
  | { method: 'add_project'; params: NewProject }
  | { method: 'update_project'; params: { project_id: ProjectId; patch: ProjectPatch } }
  | { method: 'remove_project'; params: { project_id: ProjectId } }
  | { method: 'move_project'; params: { project_id: ProjectId; workspace_id: WorkspaceId } }
  | { method: 'create_workspace'; params: { name: string } }
  | { method: 'rename_workspace'; params: { workspace_id: WorkspaceId; name: string } }
  | { method: 'delete_workspace'; params: { workspace_id: WorkspaceId; move_projects: boolean } }
  | { method: 'list_sessions'; params: { project_id: ProjectId | null } }
  | { method: 'create_session'; params: NewSession }
  | { method: 'rename_session'; params: { session_id: SessionId; title: string } }
  | { method: 'close_session'; params: { session_id: SessionId } }
  | { method: 'retry_session'; params: { session_id: SessionId } }
  | { method: 'mark_read'; params: { session_id: SessionId } }
  | {
      method: 'resolve_permission';
      params: { session_id: SessionId; decision: PermDecision; via?: PermVia; prompt_id?: string };
    }
  | { method: 'attach'; params: { session_id: SessionId; cols: number; rows: number } }
  | { method: 'detach'; params: { session_id: SessionId } }
  | { method: 'send_input'; params: { session_id: SessionId; text: string } }
  | { method: 'resize'; params: { session_id: SessionId; cols: number; rows: number } }
  | { method: 'get_settings' }
  | { method: 'update_settings'; params: { settings: Settings } }
  | { method: 'detect_launch_commands' }
  | { method: 'get_git_info'; params: { project_id: ProjectId; session_id: SessionId | null } }
  | { method: 'get_diff'; params: { session_id: SessionId; base?: DiffBase } }
  | { method: 'get_transcript'; params: { session_id: SessionId; limit: number | null } }
  | { method: 'get_review'; params: { session_id: SessionId } }
  | { method: 'list_files'; params: { session_id: SessionId; changed_only: boolean } }
  | { method: 'read_file'; params: { session_id: SessionId; path: string; rev: string | null } }
  | { method: 'write_file'; params: { session_id: SessionId; path: string; text: string; expected_sha: string | null } }
  | { method: 'get_usage'; params: { session_id: SessionId } }
  | { method: 'export_transcript'; params: { session_id: SessionId; format: ExportFormat } }
  | { method: 'get_session_info'; params: { session_id: SessionId } }
  | { method: 'stop_task'; params: { session_id: SessionId; pid: number } }
  | { method: 'list_connectors'; params: { session_id: SessionId } }
  | { method: 'check_connectors'; params: { session_id: SessionId } }
  | { method: 'mod_attach'; params: { session_id: SessionId } }
  | { method: 'resolve_session_start'; params: { id: string; approve: boolean } }
  | { method: 'hook_event'; params: { agent: AgentKind; session_id: SessionId | null; payload: unknown } }
  | { method: 'shutdown' };

export type Response =
  | { kind: 'ok' }
  | { kind: 'pong' }
  | { kind: 'snapshot'; data: Snapshot }
  | { kind: 'projects'; data: Project[] }
  | { kind: 'project'; data: Project }
  | { kind: 'sessions'; data: Session[] }
  | { kind: 'session'; data: Session }
  | { kind: 'attached'; data: AttachInfo }
  | { kind: 'settings'; data: Settings }
  | { kind: 'launch_commands'; data: LaunchCommandInfo[] }
  | { kind: 'git_info'; data: GitInfo }
  | { kind: 'workspace'; data: Workspace }
  | { kind: 'diff'; data: GitDiff }
  | { kind: 'transcript'; data: Transcript }
  | { kind: 'review'; data: Review }
  | { kind: 'file_tree'; data: FileTree }
  | { kind: 'file'; data: FileContent }
  | { kind: 'usage'; data: Usage }
  | { kind: 'export'; data: Export }
  | { kind: 'session_info'; data: SessionInfo }
  | { kind: 'connectors'; data: Connector[] }
  | { kind: 'error'; data: ProtoError };

export type DaemonEvent =
  | { event: 'project_added'; data: Project }
  | { event: 'project_updated'; data: Project }
  | { event: 'project_removed'; data: { project_id: ProjectId } }
  | { event: 'workspace_added'; data: Workspace }
  | { event: 'workspace_updated'; data: Workspace }
  | { event: 'workspace_removed'; data: { workspace_id: WorkspaceId } }
  | { event: 'session_created'; data: Session }
  | { event: 'session_updated'; data: Session }
  | { event: 'session_closed'; data: { session_id: SessionId } }
  | { event: 'session_exited'; data: { session_id: SessionId; exit_code: number | null } }
  | {
      event: 'status_changed';
      data: {
        session_id: SessionId;
        status: Status;
        source: DetectSource;
        perm: PermRequest | null;
        running_since: UnixMs | null;
        at: UnixMs;
      };
    }
  | { event: 'sub_agents_changed'; data: { session_id: SessionId; sub_agents: SubAgent[] } }
  | { event: 'sub_agent_status'; data: { session_id: SessionId; sub_agent_id: SubAgentId; status: Status } }
  | { event: 'git_info_changed'; data: { session_id: SessionId; git: GitInfo } }
  | { event: 'settings_changed'; data: Settings }
  | { event: 'usage_changed'; data: UsageToday }
  | {
      event: 'notification';
      data: {
        session_id: SessionId;
        kind: NotificationKind;
        title: string;
        subtitle: string;
        body: string | null;
        /** Show a banner; `false` with `sound: true` is sound only (proto 4). */
        banner: boolean;
        sound: boolean;
        /** macOS system sound to play (proto 4). */
        sound_name?: string;
        /** The `PermRequest.id` a `perm` notification is about; its Allow/Deny answers with it (proto 4). */
        prompt_id?: string;
        at: UnixMs;
      };
    }
  | { event: 'mod_command'; data: { session_id: SessionId; command: ModCommand } }
  | {
      event: 'session_start_requested';
      data: {
        /** Identifies the request for `resolve_session_start`. */
        id: string;
        /** The session whose agent asked. */
        parent_session: SessionId;
        project_id: ProjectId;
        agent: AgentKind;
        title: string | null;
        prompt: string;
        worktree: WorktreeSpec | null;
        at: UnixMs;
      };
    };

export type Message =
  | { type: 'hello'; body: Hello }
  | { type: 'hello_ok'; body: HelloOk }
  | { type: 'request'; body: { req_id: number; request: Request } }
  | { type: 'response'; body: { req_id: number; response: Response } }
  | { type: 'event'; body: DaemonEvent };

/** Error answered by the daemon (`Response::Error`), thrown by client methods. */
export class DaemonError extends Error {
  readonly code: ErrorCode;
  constructor(err: ProtoError) {
    super(err.message);
    this.name = 'DaemonError';
    this.code = err.code;
  }
}
