import { describe, expect, it } from 'vitest';
import {
  joinPath, repoNameFromUrl, validateBranch, validateCloneUrl, validateNewPath, validateNewSession, validatePath,
  validateProjectName,
} from './validation';

describe('paths', () => {
  it('requires an absolute or ~ path', () => {
    expect(validatePath('')).toMatch(/required/);
    expect(validatePath('   ')).toMatch(/required/);
    expect(validatePath('repos/x')).toMatch(/start with/);
    expect(validatePath('/Users/naim/x')).toBeNull();
    expect(validatePath('~/Repos/x')).toBeNull();
    expect(validatePath('~')).toBeNull();
    expect(validatePath('/a\u0000b')).toMatch(/control/);
  });
  it('rejects a folder that is already a project, ignoring trailing slashes', () => {
    const projects = [{ path: '/Users/naim/Repos/billing-api' }];
    expect(validateNewPath('/Users/naim/Repos/billing-api/', projects)).toMatch(/already/);
    expect(validateNewPath('/Users/naim/Repos/other', projects)).toBeNull();
  });
  it('joins without doubling slashes', () => {
    expect(joinPath('~/Repos/', ' app ')).toBe('~/Repos/app');
  });
});

describe('clone url', () => {
  it.each([
    'https://github.com/owner/repo.git',
    'http://host.example/o/r',
    'ssh://git@host/o/r.git',
    'git@github.com:owner/repo.git',
  ])('accepts %s', (u) => expect(validateCloneUrl(u)).toBeNull());
  it.each(['', 'github.com/owner/repo', 'https://github.com', 'not a url', 'ftp://h/r', 'https://h/r with space'])(
    'rejects %j',
    (u) => expect(validateCloneUrl(u)).not.toBeNull(),
  );
  it('derives the repo name', () => {
    expect(repoNameFromUrl('https://github.com/owner/repo.git')).toBe('repo');
    expect(repoNameFromUrl('git@github.com:owner/repo')).toBe('repo');
    expect(repoNameFromUrl('https://h/o/r/')).toBe('r');
  });
});

describe('project name', () => {
  it('accepts one folder name', () => expect(validateProjectName('my-project')).toBeNull());
  it.each(['', '  ', '.', '..', 'a/b', 'a\\b'])('rejects %j', (n) => expect(validateProjectName(n)).not.toBeNull());
});

describe('branch', () => {
  it.each(['lotse/new-task', 'feature/x-1', 'fix_rounding', 'a.b'])('accepts %s', (b) => expect(validateBranch(b)).toBeNull());
  it.each(['', ' ', 'a b', '-x', '/x', 'x/', 'x.', 'x.lock', 'a..b', 'a//b', 'a@{b', '@', 'a~b', 'a^b', 'a:b', 'a?b', 'a*b', 'a[b', '.hidden/x', 'x/.y'])(
    'rejects %j',
    (b) => expect(validateBranch(b)).not.toBeNull(),
  );
});

describe('new session form', () => {
  const projects = [
    { id: 'p1', is_git: true },
    { id: 'p5', is_git: false },
  ];
  const base = { projectId: 'p1', agent: 'terminal' as const, worktree: false, branch: '' };
  it('needs a known project', () => {
    expect(validateNewSession({ ...base, projectId: '' }, projects).project).toBeDefined();
    expect(validateNewSession(base, projects)).toEqual({});
  });
  it('checks the branch only when a worktree is asked for', () => {
    expect(validateNewSession(base, projects)).toEqual({});
    expect(validateNewSession({ ...base, worktree: true, branch: 'bad name' }, projects).branch).toBeDefined();
    expect(validateNewSession({ ...base, worktree: true, branch: 'lotse/ok' }, projects)).toEqual({});
  });
  it('refuses a worktree in a folder without git', () => {
    expect(validateNewSession({ ...base, projectId: 'p5', worktree: true, branch: 'lotse/ok' }, projects).branch).toMatch(/git/);
  });
});
