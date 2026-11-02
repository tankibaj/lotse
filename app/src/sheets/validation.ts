import type { AgentKind, Project } from '../lib/client/types';

// Pure form validation for the sheets. Each validator returns an error message or `null`.
// Alpha rule: the app never touches the disk itself, so these only reject input the host could not use.

export const AGENTS: { kind: AgentKind; label: string }[] = [
  { kind: 'terminal', label: 'Terminal' },
  { kind: 'claude_code', label: 'Claude Code' },
  { kind: 'codex', label: 'Codex' },
  { kind: 'opencode', label: 'OpenCode' },
];

const hasControl = (s: string) => [...s].some((c) => c.charCodeAt(0) < 32 || c.charCodeAt(0) === 127);
const trimSlash = (p: string) => (p.length > 1 ? p.replace(/\/+$/, '') : p);

/** Absolute (`/…`) or home-relative (`~/…`, `~`) path, no control characters. */
export function validatePath(raw: string, what = 'Folder path'): string | null {
  const p = raw.trim();
  if (!p) return `${what} is required`;
  if (hasControl(p)) return `${what} contains control characters`;
  if (!p.startsWith('/') && p !== '~' && !p.startsWith('~/')) return `${what} must start with / or ~/`;
  return null;
}

/** Same folder already added (compared without trailing slashes). */
export function validateNewPath(raw: string, projects: Pick<Project, 'path'>[]): string | null {
  const bad = validatePath(raw);
  if (bad) return bad;
  const p = trimSlash(raw.trim());
  return projects.some((x) => trimSlash(x.path) === p) ? 'This folder is already a project' : null;
}

const SCP_URL = /^[\w.-]+@[\w.-]+:[\w./~-]+$/; // git@host:owner/repo.git
const SCHEME_URL = /^(https?|ssh|git):\/\/[^\s/]+\/\S+$/i;

export function validateCloneUrl(raw: string): string | null {
  const u = raw.trim();
  if (!u) return 'Repository URL is required';
  if (/\s/.test(u)) return 'URL must not contain spaces';
  if (u.startsWith('-')) return 'URL must not start with -'; // would read as a git option
  if (!SCP_URL.test(u) && !SCHEME_URL.test(u)) return 'Use https://…, ssh://… or git@host:owner/repo.git';
  return null;
}

/** `https://github.com/o/r.git` → `r`; `git@h:o/r` → `r`. Empty when the URL has no usable tail. */
export function repoNameFromUrl(raw: string): string {
  return raw.trim().replace(/\/+$/, '').replace(/\.git$/, '').split(/[/:]/).pop() ?? '';
}

/** A single path segment usable as a folder name. */
export function validateProjectName(raw: string): string | null {
  const n = raw.trim();
  if (!n) return 'Project name is required';
  if (n === '.' || n === '..') return 'Pick a different name';
  if (/[/\\]/.test(n) || hasControl(n)) return 'Name must not contain / or \\';
  if (n.length > 120) return 'Name is too long';
  return null;
}

export const joinPath = (dir: string, name: string) => `${trimSlash(dir.trim())}/${name.trim()}`;

/** Subset of `git check-ref-format --branch` rules. */
export function validateBranch(raw: string): string | null {
  const b = raw.trim();
  if (!b) return 'Branch name is required';
  if (/[\s~^:?*[\\]/.test(b) || hasControl(b)) return 'Branch name has invalid characters';
  if (b.startsWith('-') || b.startsWith('/') || b.endsWith('/') || b.endsWith('.') || b.endsWith('.lock')) {
    return 'Branch name is not allowed';
  }
  if (b.includes('..') || b.includes('//') || b.includes('@{') || b === '@') return 'Branch name is not allowed';
  if (b.split('/').some((s) => s.startsWith('.') || s.endsWith('.lock'))) return 'Branch name is not allowed';
  return null;
}

export interface NewSessionForm {
  projectId: string;
  agent: AgentKind;
  worktree: boolean;
  branch: string;
}

export interface NewSessionErrors {
  project?: string;
  branch?: string;
}

export function validateNewSession(f: NewSessionForm, projects: Pick<Project, 'id' | 'is_git'>[]): NewSessionErrors {
  const errors: NewSessionErrors = {};
  const project = projects.find((p) => p.id === f.projectId);
  if (!project) errors.project = 'Pick a project';
  if (f.worktree) {
    if (project && !project.is_git) errors.branch = 'A worktree needs a git repository';
    else errors.branch = validateBranch(f.branch) ?? undefined;
  }
  if (!errors.branch) delete errors.branch;
  return errors;
}
