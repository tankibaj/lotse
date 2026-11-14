import { inTauri } from '../sheets/hostBridge';

// Settings › Agents › Auto-allow rules (Beta feature 14, off by default). The daemon owns the rules
// (`<data dir>/autoallow.json`) and does the matching; this file only edits them. `validateRule` mirrors
// `crates/lotsed/src/autoallow.rs` to give early feedback: the daemon re-validates everything and is the
// authority, so a rule this accepts may still be refused.
//
// The frozen client has no call that carries the rules, so the UI goes through a bridge. Until the proto
// gets `GetAutoAllow` / `SetAutoAllow` (a Request in the work package report) the app has none and the
// section says so; the browser (dev server, MockClient) gets an in-memory preview that saves nothing.

export interface AutoAllowRule {
  id: string;
  project_id: string;
  /** Exact, case-sensitive: `Bash` or one of `PATH_TOOLS`. */
  tool: string;
  command?: string;
  /** Bash: the command may continue with plain arguments after a space. */
  prefix: boolean;
  /** Path tools: project-relative directory (`''` = the whole session folder). */
  dir?: string;
  /** Lets the rule match a destructive command or a secrets path. */
  allow_destructive: boolean;
}

export interface AutoAllowConfig {
  enabled: boolean;
  rules: AutoAllowRule[];
}

export interface AutoAllowBridge {
  load(): Promise<AutoAllowConfig>;
  /** Rejects with the daemon's reason when it refuses the config; the rules in force then stay. */
  save(config: AutoAllowConfig): Promise<void>;
}

/** Tools whose rule is a directory of the project. */
export const PATH_TOOLS = ['Read', 'Edit', 'MultiEdit', 'Write', 'NotebookEdit', 'Glob', 'Grep'] as const;
export const RULE_TOOLS: readonly string[] = ['Bash', ...PATH_TOOLS];

const MAX_FIELD = 1000;
const DESTRUCTIVE_WORDS = new Set([
  'rm', 'rmdir', 'mv', 'dd', 'mkfs', 'sudo', 'su', 'doas', 'chmod', 'chown', 'chgrp', 'kill', 'killall', 'pkill',
  'shutdown', 'reboot', 'curl', 'wget', 'ssh', 'scp', 'rsync', 'nc', 'eval', 'exec', 'sh', 'bash', 'zsh', 'xargs',
  'tee', 'truncate', 'shred', 'ln', 'open', 'osascript', 'launchctl', 'defaults', 'docker', 'kubectl', 'publish',
  'unpublish', 'dlx',
]);
const DESTRUCTIVE_GIT = new Set([
  'push', 'reset', 'clean', 'rebase', 'checkout', 'restore', 'switch', 'stash', 'gc', 'prune', 'filter-branch',
  'update-ref', 'config', 'remote', 'worktree', 'branch', 'tag',
]);
const DESTRUCTIVE_FLAGS = new Set(['-delete', '-exec', '--force', '--hard', '--no-verify', '--force-with-lease']);

const basename = (word: string) => word.slice(word.lastIndexOf('/') + 1);

/** One-space-separated words of `[A-Za-z0-9_./:=@+,-]`, no `..` segment. */
const isPlain = (text: string) =>
  text.length > 0 &&
  text.length <= MAX_FIELD &&
  /^[A-Za-z0-9_./:=@+,-]+( [A-Za-z0-9_./:=@+,-]+)*$/.test(text) &&
  !text.split(' ').some((word) => word.split('/').includes('..'));

export function isDestructive(command: string): boolean {
  const words = command.split(' ');
  const git = words.some((w) => basename(w) === 'git');
  return words.some((w) => DESTRUCTIVE_WORDS.has(basename(w)) || DESTRUCTIVE_FLAGS.has(w) || (git && DESTRUCTIVE_GIT.has(basename(w))));
}

/** Secrets and agent config, compared case-insensitively (the volume may be). */
const isSensitive = (name: string) => {
  const n = name.toLowerCase();
  return (
    ['.git', '.ssh', '.aws', '.gnupg', '.netrc', '.npmrc', '.claude', '.lotse', '.git-credentials'].includes(n) ||
    n.startsWith('.env') ||
    n.startsWith('id_rsa') ||
    n.startsWith('id_ed25519') ||
    n.endsWith('.pem') ||
    n.endsWith('.key')
  );
};

/** A word of the command names a secrets file or folder (`cat .env`). */
const touchesSensitive = (command: string) => command.split(' ').some((word) => word.split('/').some(isSensitive));

/** Why the rule cannot be stored, or `null`. Same wording as the daemon where it has the same check. */
export function validateRule(rule: AutoAllowRule): string | null {
  if (!/^[A-Za-z0-9_-]{1,64}$/.test(rule.id)) return 'a rule id is 1-64 letters, digits, - or _';
  if (rule.tool === 'Bash') {
    if (rule.command === undefined) return 'a Bash rule needs a command';
    if (rule.dir !== undefined) return 'a Bash rule has no directory';
    if (!isPlain(rule.command)) return 'a command is plain words of letters, digits and _ . / : = @ + , - (no ; & | $ quotes or newlines)';
    if (rule.prefix && rule.command.includes('=')) return 'a prefix rule cannot contain =';
    if (!rule.allow_destructive && isDestructive(rule.command)) return 'that command is destructive: allow it explicitly or drop it';
    if (!rule.allow_destructive && touchesSensitive(rule.command)) return 'that command names a secrets file or folder: allow it explicitly or drop it';
    return null;
  }
  if (!(PATH_TOOLS as readonly string[]).includes(rule.tool)) return `tool "${rule.tool}" cannot be auto-allowed`;
  if (rule.command !== undefined || rule.prefix) return 'a path rule has a directory, not a command';
  const dir = rule.dir ?? '';
  if (dir.length > MAX_FIELD || dir.startsWith('/') || dir.includes('\0') || dir.split('/').includes('..')) {
    return 'a directory is relative to the project, without ..';
  }
  return null;
}

/** The first reason the config cannot be stored, or `null`. */
export function validateConfig(config: AutoAllowConfig): string | null {
  if (config.rules.length > 200) return 'at most 200 rules';
  const seen = new Set<string>();
  for (const rule of config.rules) {
    const why = validateRule(rule);
    if (why) return `rule "${rule.id}": ${why}`;
    if (seen.has(rule.id)) return `rule id "${rule.id}" is used twice`;
    seen.add(rule.id);
  }
  return null;
}

/** Browser preview: edits live until the page reloads and reach no daemon. */
function previewBridge(): AutoAllowBridge {
  let config: AutoAllowConfig = { enabled: false, rules: [] };
  return {
    load: async () => structuredClone(config),
    save: async (next) => {
      const why = validateConfig(next);
      if (why) throw new Error(why);
      config = structuredClone(next);
    },
  };
}

let preview: AutoAllowBridge | undefined;
let override: AutoAllowBridge | null | undefined;

/** Tests inject a bridge (`null`: none, as in the app today); `undefined` restores the default. */
export function setAutoAllowBridge(b: AutoAllowBridge | null | undefined): void {
  override = b;
  preview = undefined;
}

/** `true` when edits only live in this page (nothing is persisted). */
export const isPreviewBridge = () => override === undefined && !inTauri();

export function autoAllowBridge(): AutoAllowBridge | null {
  if (override !== undefined) return override;
  if (inTauri()) return null; // TODO(beta-m5): bind to the daemon once the proto carries GetAutoAllow/SetAutoAllow
  return (preview ??= previewBridge());
}
