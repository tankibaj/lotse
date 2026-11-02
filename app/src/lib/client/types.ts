// Mirror of crates/lotse-proto (PROTO_VERSION 1). Frozen after WP0: change only with an
// orchestrator-approved proto bump. JSON fixtures in crates/lotse-proto/fixtures are the shared contract.

export const PROTO_VERSION = 1;

// ids (opaque strings minted by the daemon)
export type WorkspaceId = string;
export type ProjectId = string;
export type SessionId = string;
export type SubAgentId = string;
/** Unix time in milliseconds. */
export type UnixMs = number;

export type Status = 'run' | 'perm' | 'fail' | 'done' | 'idle' | 'unknown';
export type AgentKind = 'terminal' | 'claude_code' | 'codex' | 'opencode';
export type DetectSource = 'hook' | 'osc7501' | 'screen' | 'process';
export type Theme = 'light' | 'dark' | 'auto';
export type PermDecision = 'allow' | 'always' | 'deny';
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
}

export interface GitInfo {
  /** `false` renders "No git". */
  is_git: boolean;
  branch: string;
  ahead: number;
  dirty: number;
  worktree: boolean;
  worktree_path: string | null;
}

export interface PermRequest {
  tool: string;
  summary: string;
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
}

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

export interface NotificationSettings {
  perm: EventPrefs;
  fail: EventPrefs;
  done: EventPrefs;
  sub_agent_done: EventPrefs;
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

export interface Settings {
  launch_commands: LaunchCommands;
  notifications: NotificationSettings;
  appearance: AppearanceSettings;
  scrollback_bytes: number;
}

export interface DaemonInfo {
  version: string;
  proto_version: number;
  pid: number;
  started_at: UnixMs;
}

export interface Snapshot {
  daemon: DaemonInfo;
  projects: Project[];
  sessions: Session[];
  settings: Settings;
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
  | { method: 'list_sessions'; params: { project_id: ProjectId | null } }
  | { method: 'create_session'; params: NewSession }
  | { method: 'rename_session'; params: { session_id: SessionId; title: string } }
  | { method: 'close_session'; params: { session_id: SessionId } }
  | { method: 'retry_session'; params: { session_id: SessionId } }
  | { method: 'mark_read'; params: { session_id: SessionId } }
  | { method: 'resolve_permission'; params: { session_id: SessionId; decision: PermDecision } }
  | { method: 'attach'; params: { session_id: SessionId; cols: number; rows: number } }
  | { method: 'detach'; params: { session_id: SessionId } }
  | { method: 'send_input'; params: { session_id: SessionId; text: string } }
  | { method: 'resize'; params: { session_id: SessionId; cols: number; rows: number } }
  | { method: 'get_settings' }
  | { method: 'update_settings'; params: { settings: Settings } }
  | { method: 'detect_launch_commands' }
  | { method: 'get_git_info'; params: { project_id: ProjectId; session_id: SessionId | null } }
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
  | { kind: 'error'; data: ProtoError };

export type DaemonEvent =
  | { event: 'project_added'; data: Project }
  | { event: 'project_updated'; data: Project }
  | { event: 'project_removed'; data: { project_id: ProjectId } }
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
  | {
      event: 'notification';
      data: {
        session_id: SessionId;
        kind: NotificationKind;
        title: string;
        subtitle: string;
        body: string | null;
        sound: boolean;
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
