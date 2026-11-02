import type {
  AgentKind,
  AttachInfo,
  DaemonEvent,
  GitInfo,
  HelloOk,
  LaunchCommandInfo,
  NewProject,
  NewSession,
  PermDecision,
  Project,
  ProjectId,
  ProjectPatch,
  Session,
  SessionId,
  Settings,
  Snapshot,
} from './types';

export type Unsubscribe = () => void;

/**
 * Link to the daemon. `connected`: first handshake done. `disconnected`: socket dropped (status bar
 * shows lotsed down; the app may restart it). `reconnected`: link is back, so refetch `getSnapshot()`
 * and reattach terminals (events were missed meanwhile).
 */
export type ConnectionState = 'connected' | 'disconnected' | 'reconnected';

/**
 * The app's only view of the daemon. Mirrors `lotse-proto` 1:1: each method is one `Request`
 * (named in the comment) and resolves with its success `Response`, or rejects with `DaemonError`.
 * `MockClient` implements it in memory; WP8 adds the real client over Tauri commands/events.
 * Frozen after WP0.
 */
export interface DaemonClient {
  /** `hello` / `hello_ok` handshake. Call once before anything else. */
  connect(): Promise<HelloOk>;
  close(): void;

  /** `ping` */
  ping(): Promise<void>;
  /** `get_snapshot` */
  getSnapshot(): Promise<Snapshot>;

  /** `list_projects` */
  listProjects(): Promise<Project[]>;
  /** `add_project` */
  addProject(project: NewProject): Promise<Project>;
  /** `update_project` */
  updateProject(projectId: ProjectId, patch: Partial<ProjectPatch>): Promise<Project>;
  /** `remove_project` */
  removeProject(projectId: ProjectId): Promise<void>;

  /** `list_sessions` */
  listSessions(projectId?: ProjectId): Promise<Session[]>;
  /** `create_session` */
  createSession(session: NewSession): Promise<Session>;
  /** `rename_session` */
  renameSession(sessionId: SessionId, title: string): Promise<void>;
  /** `close_session` */
  closeSession(sessionId: SessionId): Promise<void>;
  /** `retry_session` */
  retrySession(sessionId: SessionId): Promise<void>;
  /** `mark_read` */
  markRead(sessionId: SessionId): Promise<void>;
  /** `resolve_permission` */
  resolvePermission(sessionId: SessionId, decision: PermDecision): Promise<void>;

  /** `attach`: replay arrives through `onPtyData` first, then live output. */
  attach(sessionId: SessionId, cols: number, rows: number): Promise<AttachInfo>;
  /** `detach` */
  detach(sessionId: SessionId): Promise<void>;
  /** `send_input`: programmatic text. Keystrokes go through `writePty`. */
  sendInput(sessionId: SessionId, text: string): Promise<void>;
  /** `resize` */
  resize(sessionId: SessionId, cols: number, rows: number): Promise<void>;

  /** `get_settings` */
  getSettings(): Promise<Settings>;
  /** `update_settings` (replaces the whole object) */
  updateSettings(settings: Settings): Promise<Settings>;
  /** `detect_launch_commands` */
  detectLaunchCommands(): Promise<LaunchCommandInfo[]>;

  /** `get_git_info` */
  getGitInfo(projectId: ProjectId, sessionId?: SessionId): Promise<GitInfo>;
  /** `hook_event` (sent by `lotse-hook`; exposed for completeness and tests) */
  hookEvent(agent: AgentKind, sessionId: SessionId | null, payload: unknown): Promise<void>;
  /** `shutdown` */
  shutdown(): Promise<void>;

  /** Connection state changes (see `ConnectionState`). Not a proto message: the client's own transport state. */
  onConnection(handler: (state: ConnectionState) => void): Unsubscribe;
  /** Pushed `event`s: project/session changes, status, sub-agents, git, settings, notifications. */
  onEvent(handler: (event: DaemonEvent) => void): Unsubscribe;
  /** PTY frames daemon → client (terminal output) for one attached session. */
  onPtyData(sessionId: SessionId, handler: (data: Uint8Array) => void): Unsubscribe;
  /** PTY frame client → daemon (keyboard input). */
  writePty(sessionId: SessionId, data: Uint8Array): void;
}
