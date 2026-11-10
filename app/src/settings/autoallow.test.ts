import { describe, expect, it } from 'vitest';
import { isDestructive, validateConfig, validateRule } from './autoallow';
import type { AutoAllowRule } from './autoallow';

// Adversarial extras for the early checks (the daemon's own tests are `crates/lotsed/src/autoallow.rs`).

const bash = (command: string, extra: Partial<AutoAllowRule> = {}): AutoAllowRule => ({
  id: 'r1',
  project_id: 'p1',
  tool: 'Bash',
  command,
  prefix: false,
  allow_destructive: false,
  ...extra,
});
const path = (tool: string, dir: string, extra: Partial<AutoAllowRule> = {}): AutoAllowRule => ({
  id: 'r1',
  project_id: 'p1',
  tool,
  dir,
  prefix: false,
  allow_destructive: false,
  ...extra,
});

describe('early rule checks', () => {
  it('refuse lookalike and invisible characters, written as escapes', () => {
    for (const bad of [
      'git status', // no-break space
      'git status​', // zero-width space
      'git statuѕ', // Cyrillic ѕ
      'ls； id', // fullwidth semicolon
      'ｇit status', // fullwidth g
      'ls id', // line separator
      'ls\r',
      'ls\0',
    ]) {
      expect(validateRule(bash(bad)), JSON.stringify(bad)).not.toBeNull();
      expect(validateRule(bash(bad, { prefix: true })), JSON.stringify(bad)).not.toBeNull();
    }
  });

  it('refuse shell syntax in a prefix rule and in an exact rule alike', () => {
    for (const bad of ['git status;', 'git status &&', 'git status |', 'git status >', 'git status $X', 'git status `x`', 'git status (x)']) {
      for (const prefix of [false, true]) expect(validateRule(bash(bad, { prefix })), `${bad} ${prefix}`).not.toBeNull();
    }
  });

  it('keep the explicit opt-in from lifting the plain-command rule', () => {
    for (const bad of ['rm -rf build; ls', 'sudo ls && id', 'git push | cat', 'curl x $(id)']) {
      expect(validateRule(bash(bad, { allow_destructive: true })), bad).not.toBeNull();
    }
    for (const ok of ['git status', 'git diff', 'git log --oneline', 'npm test', 'ls -la', 'cargo test -p lotsed']) {
      expect(isDestructive(ok), ok).toBe(false);
    }
    for (const cmd of ['git -C sub clean -fd', 'docker ps', 'tee out.txt', 'ls --force', 'find . -exec ls']) {
      expect(isDestructive(cmd), cmd).toBe(true);
    }
  });

  it('refuse a command that names a secrets file unless explicit', () => {
    for (const cmd of ['cat .env', 'cat .ENV', 'cat src/.env.local', 'cat .git/config', 'ls .ssh', 'cat server.pem', 'cat id_rsa', 'cat .npmrc']) {
      expect(validateRule(bash(cmd)), cmd).toMatch(/secrets/);
      expect(validateRule(bash(cmd, { allow_destructive: true })), cmd).toBeNull();
    }
    for (const cmd of ['cat README.md', 'cat .gitignore', 'ls .github']) expect(validateRule(bash(cmd)), cmd).toBeNull();
  });

  it('are case-sensitive about the tool', () => {
    for (const tool of ['bash', 'BASH', 'read', 'Shell', 'WebFetch', '']) {
      expect(validateRule(bash('ls', { tool })), tool).not.toBeNull();
    }
    expect(validateRule(path('Read', 'src'))).toBeNull();
    expect(validateRule(path('Glob', ''))).toBeNull();
  });

  it('refuse a config with a duplicate id or more than 200 rules', () => {
    expect(validateConfig({ enabled: true, rules: [bash('ls'), bash('pwd')] })).toMatch(/twice/);
    const many = Array.from({ length: 201 }, (_, i) => bash('ls', { id: `r${i}` }));
    expect(validateConfig({ enabled: true, rules: many })).toMatch(/at most/);
    expect(validateConfig({ enabled: false, rules: [] })).toBeNull();
    expect(validateConfig({ enabled: true, rules: [bash('ls'), bash('pwd', { id: 'r2' })] })).toBeNull();
  });
});
