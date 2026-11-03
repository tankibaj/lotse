import type {
  AgentKind,
  AttachInfo,
  Connector,
  DaemonEvent,
  DiffBase,
  Export,
  ExportFormat,
  FileContent,
  FileTree,
  GitDiff,
  GitInfo,
  HelloOk,
  LaunchCommandInfo,
  NewProject,
  NewSession,
  PermDecision,
  PermVia,
  Project,
  ProjectId,
  ProjectPatch,
  Review,
  Session,
  SessionId,
  SessionInfo,
  Settings,
  Snapshot,
  Transcript,
  Usage,
  Workspace,
  WorkspaceId,
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

  /** `move_project`: also announced as `project_updated`. */
  moveProject(projectId: ProjectId, workspaceId: WorkspaceId): Promise<Project>;

  /** `create_workspace`: rejects with `conflict` when the name is taken. */
  createWorkspace(name: string): Promise<Workspace>;
  /** `rename_workspace` */
  renameWorkspace(workspaceId: WorkspaceId, name: string): Promise<Workspace>;
  /** `delete_workspace`: `conflict` while it has projects, unless `moveProjects` sends them to the default workspace. */
  deleteWorkspace(workspaceId: WorkspaceId, moveProjects: boolean): Promise<void>;

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
  /**
   * `resolve_permission`. `promptId` (the `PermRequest.id` the answer is for) makes a stale answer a `conflict`
   * instead of hitting a newer prompt; `via` is recorded with the decision.
   */
  resolvePermission(sessionId: SessionId, decision: PermDecision, opts?: { via?: PermVia; promptId?: string }): Promise<void>;

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
  /** `get_diff`: Show Changes. `base` defaults to `head` (uncommitted); `merge_base` is everything the branch adds (review). */
  getDiff(sessionId: SessionId, base?: DiffBase): Promise<GitDiff>;
  /** `get_transcript`: Show Transcript. Rejects with `unavailable` ("Not available for OpenCode") for agents without one. */
  getTranscript(sessionId: SessionId, limit?: number): Promise<Transcript>;
  /** `get_review`: ahead, behind and the commits ahead of the base. */
  getReview(sessionId: SessionId): Promise<Review>;
  /** `list_files`: the session folder's files; only the changed ones with `changedOnly`. */
  listFiles(sessionId: SessionId, changedOnly?: boolean): Promise<FileTree>;
  /** `read_file`: at `rev` (a git revision) or as on disk. Paths are confined to the session folder. */
  readFile(sessionId: SessionId, path: string, rev?: string): Promise<FileContent>;
  /** `write_file`: rejects with `conflict` when `expectedSha` is not the file's current sha. */
  writeFile(sessionId: SessionId, path: string, text: string, expectedSha?: string): Promise<FileContent>;
  /** `get_usage`: time, tokens and spend of one session. */
  getUsage(sessionId: SessionId): Promise<Usage>;
  /** `export_transcript`: the transcript as a document, secrets masked. */
  exportTranscript(sessionId: SessionId, format: ExportFormat): Promise<Export>;
  /** `get_session_info`: Session panel data. */
  getSessionInfo(sessionId: SessionId): Promise<SessionInfo>;
  /** `stop_task`: SIGTERM to a background task `getSessionInfo` listed. */
  stopTask(sessionId: SessionId, pid: number): Promise<void>;
  /** `list_connectors`: MCP servers from the agents' config files (status `configured`). */
  listConnectors(sessionId: SessionId): Promise<Connector[]>;
  /** `check_connectors`: like `listConnectors`, with live status where `Settings.external.mcp_live` allows it. */
  checkConnectors(sessionId: SessionId): Promise<Connector[]>;
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
