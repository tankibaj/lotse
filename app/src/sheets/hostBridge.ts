import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

// What the Add project sheet and the sidebar need from the host (file system, git, native dialog,
// Finder). The frozen DaemonClient has no such calls, so they go through Tauri commands in the app
// (`app/src-tauri/src/host.rs`, WP8) and are simulated in the browser (dev server / MockClient).
// Tests inject their own bridge with `setHostBridge`.

export interface RepoScan {
  /** The folder itself is a git repository. */
  is_repo: boolean;
  /** Git repositories found (the folder itself, or its direct children). */
  repos: string[];
  /** More child folders than the scan limit: only the first ones by name were looked at. */
  truncated?: boolean;
}

export interface HostBridge {
  /** Native folder picker; `null` when cancelled; `undefined` when there is none (browser). */
  pickFolder(): Promise<string | null> | undefined;
  scanRepos(path: string): Promise<RepoScan>;
  cloneRepo(url: string, dest: string): Promise<void>;
  /** Create `path` (and `git init` when asked). */
  createProject(path: string, gitInit: boolean): Promise<void>;
  /** Show a folder in Finder. `undefined` in the browser (there is no Finder to ask). */
  revealInFinder?(path: string): Promise<void>;
  /**
   * Folder drops onto `region()` (looked up per event: the sidebar can be hidden and shown again).
   * `onHover` fires while a file drag is over it. Returns the cleanup.
   * Browser drops only expose folder names, so paths are `~/…/<name>` like the mock.
   */
  watchDrops(region: () => HTMLElement | null, h: { onHover(over: boolean): void; onDrop(paths: string[]): void }): () => void;
}

export const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const baseName = (p: string) => p.replace(/\/+$/, '').split('/').pop() ?? p;

const browserDrops: HostBridge['watchDrops'] = (region, { onHover, onDrop }) => {
  // Only file drags: session drags (text/lotse-session) belong to the panes.
  const isFiles = (e: DragEvent) => !!e.dataTransfer?.types.includes('Files');
  const inRegion = (e: DragEvent) => {
    const r = region();
    return !!r && e.target instanceof Node && r.contains(e.target);
  };
  const over = (e: DragEvent) => {
    if (!isFiles(e) || !inRegion(e)) return onHover(false);
    e.preventDefault();
    onHover(true);
  };
  const leave = () => onHover(false);
  const drop = (e: DragEvent) => {
    onHover(false);
    if (!isFiles(e) || !inRegion(e)) return;
    e.preventDefault();
    const dirs = [...(e.dataTransfer?.items ?? [])]
      .map((i) => i.webkitGetAsEntry?.())
      .filter((x): x is FileSystemEntry => !!x && x.isDirectory);
    onDrop(dirs.map((d) => `~/…/${d.name}`));
  };
  document.addEventListener('dragover', over);
  document.addEventListener('dragleave', leave);
  document.addEventListener('drop', drop);
  return () => {
    document.removeEventListener('dragover', over);
    document.removeEventListener('dragleave', leave);
    document.removeEventListener('drop', drop);
  };
};

const tauriDrops: HostBridge['watchDrops'] = (region, { onHover, onDrop }) => {
  let off: (() => void) | undefined;
  let stopped = false;
  const inside = (p: { x: number; y: number }) => {
    const r = region()?.getBoundingClientRect();
    if (!r) return false;
    const x = p.x / devicePixelRatio;
    const y = p.y / devicePixelRatio;
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  };
  void import('@tauri-apps/api/webview').then(async ({ getCurrentWebview }) => {
    const un = await getCurrentWebview().onDragDropEvent((ev) => {
      const p = ev.payload;
      if (p.type === 'leave') onHover(false);
      else if (p.type === 'over') onHover(inside(p.position));
      else {
        onHover(false);
        if (p.type === 'drop' && inside(p.position)) onDrop(p.paths);
      }
    });
    if (stopped) un();
    else off = un;
  }).catch(() => {}); // no webview API (tests, odd hosts): no drops
  return () => {
    stopped = true;
    off?.();
  };
};

// Simulated host for the browser: every path is a repo, except `multi*` folders (3 child repos), so
// the multi-repo flow can be walked against the MockClient.
const browserBridge: HostBridge = {
  pickFolder: () => undefined,
  async scanRepos(path) {
    await sleep(80);
    if (baseName(path).startsWith('multi')) {
      return { is_repo: false, repos: ['api', 'web', 'docs'].map((n) => `${path.replace(/\/+$/, '')}/${n}`) };
    }
    return { is_repo: true, repos: [path] };
  },
  cloneRepo: () => sleep(400),
  createProject: () => sleep(150),
  watchDrops: browserDrops,
};

const call = <T>(cmd: string, args: Record<string, unknown>): Promise<T> => invoke<T>(cmd, args);

const tauriBridge: HostBridge = {
  pickFolder: async () => {
    const picked = await open({ directory: true, multiple: false, title: 'Add a project' });
    return typeof picked === 'string' ? picked : null;
  },
  scanRepos: (path) => call<RepoScan>('scan_repos', { path }),
  cloneRepo: (url, dest) => call<void>('clone_repo', { url, dest }),
  createProject: (path, gitInit) => call<void>('create_project_dir', { path, gitInit }),
  revealInFinder: (path) => call<void>('reveal_in_finder', { path }),
  watchDrops: tauriDrops,
};

let override: HostBridge | null = null;

/** Replace the bridge (tests, or WP8 once the real commands exist). `null` restores the default. */
export function setHostBridge(b: HostBridge | null): void {
  override = b;
}

export const hostBridge = (): HostBridge => override ?? (inTauri() ? tauriBridge : browserBridge);
