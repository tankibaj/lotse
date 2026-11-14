import type { FileEntry } from '../../lib/client/types';

export interface TreeNode {
  name: string;
  /** Relative to the session folder, `/` separated. */
  path: string;
  dir: boolean;
  /** Files only. */
  entry?: FileEntry;
  children: TreeNode[];
}

export type Mode = 'all' | 'changed';

export interface Row {
  node: TreeNode;
  depth: number;
  /** Folders only. */
  open: boolean;
  /** A folder holding a changed file. */
  hasChanges: boolean;
}

/** Nest the flat entries; folders the daemon did not list are implied by their children. Folders sort first, otherwise the daemon's order stays. */
export function buildTree(entries: FileEntry[]): TreeNode[] {
  const root: TreeNode = { name: '', path: '', dir: true, children: [] };
  const byPath = new Map<string, TreeNode>([['', root]]);
  const folder = (path: string): TreeNode => {
    const known = byPath.get(path);
    if (known) return known;
    const cut = path.lastIndexOf('/');
    const node: TreeNode = { name: path.slice(cut + 1), path, dir: true, children: [] };
    byPath.set(path, node);
    folder(path.slice(0, Math.max(cut, 0))).children.push(node);
    return node;
  };
  for (const entry of entries) {
    if (entry.dir) {
      folder(entry.path);
      continue;
    }
    const cut = entry.path.lastIndexOf('/');
    const node: TreeNode = { name: entry.path.slice(cut + 1), path: entry.path, dir: false, entry, children: [] };
    folder(cut < 0 ? '' : entry.path.slice(0, cut)).children.push(node);
  }
  const sort = (nodes: TreeNode[]) => {
    nodes.sort((a, b) => Number(b.dir) - Number(a.dir));
    for (const n of nodes) sort(n.children);
  };
  sort(root.children);
  return root.children;
}

export const isChanged = (n: TreeNode): boolean => n.dir ? n.children.some(isChanged) : n.entry?.status != null;

export function countFiles(nodes: TreeNode[]): { files: number; changed: number } {
  let files = 0;
  let changed = 0;
  for (const n of nodes) {
    if (n.dir) {
      const sub = countFiles(n.children);
      files += sub.files;
      changed += sub.changed;
    } else {
      files += 1;
      if (isChanged(n)) changed += 1;
    }
  }
  return { files, changed };
}

/** What the panel opens first: the folders above the first changed file, else the first top-level folder. */
export function defaultOpen(nodes: TreeNode[]): Set<string> {
  const firstChanged = (list: TreeNode[]): TreeNode | undefined => {
    for (const n of list) {
      const hit = n.dir ? firstChanged(n.children) : isChanged(n) ? n : undefined;
      if (hit) return hit;
    }
    return undefined;
  };
  const open = new Set<string>();
  const target = firstChanged(nodes)?.path;
  if (target === undefined) {
    const first = nodes.find((n) => n.dir);
    if (first) open.add(first.path);
    return open;
  }
  for (let cut = target.indexOf('/'); cut >= 0; cut = target.indexOf('/', cut + 1)) open.add(target.slice(0, cut));
  return open;
}

/** The visible rows in display order. `Changed` and a filter expand every folder and drop the ones with nothing to show. */
export function visibleRows(nodes: TreeNode[], mode: Mode, query: string, openSet: Set<string>, depth = 0): Row[] {
  const q = query.trim().toLowerCase();
  const out: Row[] = [];
  for (const node of nodes) {
    if (!node.dir) {
      if (mode === 'changed' && !isChanged(node)) continue;
      if (q && !node.path.toLowerCase().includes(q)) continue;
      out.push({ node, depth, open: false, hasChanges: false });
      continue;
    }
    const open = mode === 'changed' || q !== '' || openSet.has(node.path);
    const below = visibleRows(node.children, mode, query, openSet, depth + 1);
    const filtered = mode === 'changed' || q !== '';
    if (filtered && below.length === 0) continue;
    out.push({ node, depth, open, hasChanges: isChanged(node) });
    if (open) out.push(...below);
  }
  return out;
}
