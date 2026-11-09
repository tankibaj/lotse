import { describe, expect, it } from 'vitest';
import requests from '../../../../crates/lotse-proto/fixtures/requests.json';
import { DEFAULT_SCRIPT, FIXTURE_NOW, MockClient, SUB_AGENT_SCRIPT } from './MockClient';
import type { DaemonEvent, Status } from './types';
import { DaemonError, PROTO_VERSION } from './types';

const make = (now = FIXTURE_NOW) => new MockClient({ now: () => now, script: false });

const collect = (c: MockClient) => {
  const events: DaemonEvent[] = [];
  c.onEvent((e) => events.push(e));
  return events;
};

describe('MockClient seed (spec/mock.html data model)', () => {
  it('has the mock projects, sessions and sub-agents', async () => {
    const snap = await make().getSnapshot();
    expect(snap.projects.map((p) => p.name)).toEqual(['billing-api', 'praxis-web', 'infra-terraform', 'dotfiles', 'notes']);
    expect(snap.sessions.map((s) => [s.id, s.status])).toEqual([
      ['a1', 'run'],
      ['a2', 'idle'],
      ['a3', 'perm'],
      ['a4', 'fail'],
      ['a5', 'run'],
      ['a6', 'done'],
      ['a7', 'idle'],
    ]);
    const a3 = snap.sessions.find((s) => s.id === 'a3');
    expect(a3?.sub_agents.map((x) => [x.name, x.status])).toEqual([
      ['component-migrator', 'done'],
      ['spec-updater', 'done'],
      ['e2e-runner', 'perm'],
    ]);
    expect(snap.projects.find((p) => p.name === 'notes')?.is_git).toBe(false);
    expect(snap.daemon.proto_version).toBe(PROTO_VERSION);
  });

  it('shifts fixture timestamps so ages are relative to now', async () => {
    const later = FIXTURE_NOW + 3_600_000;
    const a1 = (await make(later).listSessions()).find((s) => s.id === 'a1');
    expect(a1?.running_since).toBe(1793610679000 + 3_600_000);
    expect(later - (a1?.last_activity ?? 0)).toBe(120_000);
  });

  it('returns copies: callers cannot mutate the mock state', async () => {
    const c = make();
    (await c.listSessions())[0]!.title = 'changed';
    expect((await c.listSessions())[0]!.title).toBe('Fix invoice rounding');
  });
});

describe('MockClient behaviour', () => {
  it('resolves a permission card like the mock: allow runs, deny idles, sub-agent follows', async () => {
    const c = make();
    const events = collect(c);
    await c.resolvePermission('a3', 'allow');
    let a3 = (await c.listSessions('p2')).find((s) => s.id === 'a3');
    expect(a3?.status).toBe('run');
    expect(a3?.perm).toBeNull();
    expect(a3?.sub_agents.at(-1)?.status).toBe('run');
    expect(a3?.running_since).toBe(FIXTURE_NOW);
    expect(events.map((e) => e.event)).toContain('status_changed');

    const d = make();
    await d.resolvePermission('a3', 'deny');
    a3 = (await d.listSessions('p2')).find((s) => s.id === 'a3');
    expect(a3?.status).toBe('idle');
    expect(a3?.sub_agents.at(-1)?.status).toBe('idle');
  });

  it('notifies per Settings and per-project overrides, never when paused', async () => {
    const c = make();
    const events = collect(c);
    c.setStatus('a2', 'fail');
    c.setSubAgent('a1', 'a1-s2', 'done'); // sub-agent done is off by default
    expect(events.filter((e) => e.event === 'notification').map((e) => e.data)).toMatchObject([
      { kind: 'fail', title: 'billing-api · Codex', subtitle: 'Failed', sound: true },
    ]);

    await c.updateProject('p1', { notifications: { mode: 'mute' } });
    events.length = 0;
    c.setStatus('a1', 'perm', { tool: 'Bash', summary: 'ls' });
    expect(events.some((e) => e.event === 'notification')).toBe(false);

    await c.updateProject('p1', { notifications: { mode: 'global' } });
    const s = await c.getSettings();
    await c.updateSettings({ ...s, notifications: { ...s.notifications, paused: true } });
    events.length = 0;
    c.setStatus('a1', 'fail');
    expect(events.some((e) => e.event === 'notification')).toBe(false);
  });

  it('sends unread and sub-agent changes as their own events, like the daemon', async () => {
    const c = make();
    const events = collect(c);
    c.setStatus('a5', 'done');
    expect(events.map((e) => e.event)).toEqual(['status_changed', 'session_updated', 'notification']);
    expect(events[1]).toMatchObject({ data: { id: 'a5', unread: true } });
    events.length = 0;
    c.setStatus('a5', 'done'); // already unread: no second update
    expect(events.map((e) => e.event)).toEqual(['status_changed', 'notification']);

    events.length = 0;
    c.setStatus('a1', 'perm', { tool: 'Bash', summary: 'ls' }); // running sub-agent test-runner waits too
    expect(events.map((e) => e.event)).toEqual(['status_changed', 'sub_agents_changed', 'notification']);
    const subs = (await c.listSessions('p1')).find((s) => s.id === 'a1')?.sub_agents;
    expect(subs?.map((x) => x.status)).toEqual(['done', 'perm']);

    events.length = 0;
    await c.markRead('a5');
    expect(events.map((e) => e.event)).toEqual(['session_updated']);
  });

  it('reports connection changes', async () => {
    const c = make();
    const seen: string[] = [];
    c.onConnection((s) => seen.push(s));
    await c.connect();
    c.setConnection('disconnected');
    c.setConnection('reconnected');
    expect(seen).toEqual(['connected', 'disconnected', 'reconnected']);
  });

  it('creates and closes sessions with events', async () => {
    const c = make();
    const events = collect(c);
    const s = await c.createSession({
      project_id: 'p1',
      agent: 'claude_code',
      command: null,
      title: null,
      worktree: { branch: 'lotse/new-task' },
      first_prompt: null,
    });
    expect(s).toMatchObject({ id: 'a8', title: 'Claude Code', command: 'claude', status: 'unknown' });
    expect(s.git).toMatchObject({ worktree: true, branch: 'lotse/new-task' });
    await c.closeSession(s.id);
    expect(events.map((e) => e.event)).toEqual(['session_created', 'session_closed']);
    await expect(c.closeSession('a99')).rejects.toBeInstanceOf(DaemonError);
  });

  it('plays the script: status changes at their mock times, then the permission is auto-allowed', async () => {
    const c = new MockClient({ now: () => FIXTURE_NOW, script: DEFAULT_SCRIPT, tickMs: 0 });
    const status = async (id: string): Promise<Status | undefined> => (await c.listSessions()).find((s) => s.id === id)?.status;
    c.tick(3_999);
    expect(await status('a5')).toBe('run');
    c.tick(1);
    expect(await status('a5')).toBe('done');
    c.tick(5_000);
    expect(await status('a2')).toBe('run');
    c.tick(7_000);
    expect(await status('a2')).toBe('fail');
    c.tick(12_000);
    expect(await status('a1')).toBe('perm');
    c.tick(12_000);
    expect(await status('a1')).toBe('run');
  });

  it('streams the mock terminal screen on attach and echoes input', async () => {
    const c = make();
    const chunks: string[] = [];
    c.onPtyData('a4', (d) => chunks.push(new TextDecoder().decode(d)));
    const info = await c.attach('a4', 100, 30);
    await new Promise((r) => setTimeout(r, 5));
    expect(chunks[0]).toContain('ERESOLVE');
    expect(info.replay_len).toBe(new TextEncoder().encode(chunks[0]).length);
    c.writePty('a4', new TextEncoder().encode('x\r'));
    expect(chunks.at(-1)).toBe('x\r\n');
  });

  it('reports launch commands like Settings › Agents', async () => {
    const cmds = await make().detectLaunchCommands();
    expect(cmds.map((x) => [x.agent, x.default_command, x.found])).toEqual([
      ['terminal', '$SHELL -l', true],
      ['claude_code', 'claude', true],
      ['codex', 'codex', true],
      ['opencode', 'opencode', false],
    ]);
  });
});

describe('MockClient workspaces, diff, transcript and usage (proto 3)', () => {
  it('starts with one default workspace holding every project, and the mock usage', async () => {
    const snap = await make().getSnapshot();
    expect(snap.workspaces).toEqual([{ id: 'w1', name: 'Default' }]);
    expect(snap.projects.every((p) => p.workspace_id === 'w1')).toBe(true);
    expect(snap.usage).toMatchObject({ cost_micros: 4_180_000 });
    expect(snap.usage.input_tokens + snap.usage.output_tokens + snap.usage.cache_read_tokens + snap.usage.cache_write_tokens).toBe(412_000);
  });

  it('creates, renames, moves into and deletes workspaces, emitting the daemon events', async () => {
    const c = make();
    const events = collect(c);
    const ws = await c.createWorkspace('  Client work ');
    expect(ws.name).toBe('Client work');
    await expect(c.createWorkspace('client WORK')).rejects.toMatchObject({ code: 'conflict' });
    await expect(c.createWorkspace('  ')).rejects.toMatchObject({ code: 'invalid_request' });
    expect(await c.renameWorkspace(ws.id, 'Clients')).toEqual({ id: ws.id, name: 'Clients' });
    await expect(c.renameWorkspace('nope', 'x')).rejects.toBeInstanceOf(DaemonError);

    const moved = await c.moveProject('p2', ws.id);
    expect(moved.workspace_id).toBe(ws.id);
    await expect(c.moveProject('p2', 'nope')).rejects.toMatchObject({ code: 'not_found' });

    // not while it has projects, unless they move to the default workspace
    await expect(c.deleteWorkspace(ws.id, false)).rejects.toMatchObject({ code: 'conflict' });
    await c.deleteWorkspace(ws.id, true);
    const snap = await c.getSnapshot();
    expect(snap.workspaces).toEqual([{ id: 'w1', name: 'Default' }]);
    expect(snap.projects.find((p) => p.id === 'p2')?.workspace_id).toBe('w1');
    await expect(c.deleteWorkspace('w1', true)).rejects.toMatchObject({ code: 'conflict' });
    expect(events.map((e) => e.event)).toEqual([
      'workspace_added',
      'workspace_updated',
      'project_updated',
      'project_updated',
      'workspace_removed',
    ]);
  });

  it('answers Show Changes: files for a dirty git session, none for a clean one, no git for notes', async () => {
    const c = make();
    const dirty = await c.getDiff('a1');
    expect(dirty.is_git).toBe(true);
    expect(dirty.files.map((f) => f.status)).toEqual(['modified', 'added', 'modified', 'untracked']);
    expect(dirty.files.some((f) => f.binary)).toBe(true);
    const a7 = (await c.listSessions()).find((s) => s.id === 'a7')!;
    expect(a7.git.is_git).toBe(false);
    expect((await c.getDiff('a7')).is_git).toBe(false);
    await expect(c.getDiff('nope')).rejects.toMatchObject({ code: 'not_found' });
  });

  it('answers Show Transcript for Claude Code and Codex, and refuses OpenCode and Terminal', async () => {
    const c = make();
    const snap = await c.getSnapshot();
    const of = (agent: string) => snap.sessions.find((s) => s.agent === agent)!.id;
    const t = await c.getTranscript(of('claude_code'));
    expect(t.available).toBe(true);
    expect(t.turns.map((x) => x.role)).toContain('tool');
    expect((await c.getTranscript(of('claude_code'), 2)).turns).toHaveLength(2);
    expect((await c.getTranscript(of('codex'))).available).toBe(true);
    await expect(c.getTranscript(of('opencode'))).rejects.toMatchObject({ code: 'unavailable', message: 'Not available for OpenCode' });
    await expect(c.getTranscript(of('terminal'))).rejects.toMatchObject({ code: 'unavailable' });
  });

  it('pushes usage_changed when the usage moves', async () => {
    const c = make();
    const events = collect(c);
    const usage = { input_tokens: 1, output_tokens: 2, cache_read_tokens: 3, cache_write_tokens: 4, cost_micros: null };
    c.setUsage(usage);
    expect(events).toEqual([{ event: 'usage_changed', data: usage }]);
    expect((await c.getSnapshot()).usage).toEqual(usage);
  });
});

describe('MockClient Beta data (proto 4, spec/mock.html panels)', () => {
  const TOTALS = 'src/invoice/totals.ts';

  it('seeds the v4 fields: git behind, +/− and PR, a perm prompt id', async () => {
    const snap = await make().getSnapshot();
    expect(snap.daemon.proto_version).toBe(5); // the proto 5 bump
    const a1 = snap.sessions.find((s) => s.id === 'a1')!;
    expect(a1.git).toMatchObject({ ahead: 2, behind: 1, added: 42, removed: 7, pr: { number: 184, state: 'draft', provider: 'github' } });
    expect(snap.settings.external).toEqual({ pr_status: false, mcp_live: false });
    expect(snap.sessions.find((s) => s.id === 'a3')?.perm?.id).toBeTruthy();
  });

  it('getReview: commits ahead of main, newest first, as the Changes panel lists them', async () => {
    const c = make();
    const r = await c.getReview('a1');
    expect(r).toMatchObject({ base: 'main', ahead: 2, behind: 1, truncated: false });
    expect(r.commits.map((x) => [x.sha, x.subject])).toEqual([
      ['a41c9e2', "Use banker's rounding for invoice totals"],
      ['7be03d1', 'Add roundHalfEven helper'],
    ]);
    expect(FIXTURE_NOW - r.commits[0]!.at).toBe(4 * 60_000);
    expect((await c.getReview('a2')).commits).toEqual([]);
    expect((await c.getReview('a3')).commits).toHaveLength(5);
    await expect(c.getReview('nope')).rejects.toMatchObject({ code: 'not_found' });
  });

  it('getDiff: per-file +/− counts, and the merge-base view includes committed work', async () => {
    const c = make();
    const head = await c.getDiff('a1');
    expect(head.files.map((f) => [f.path, f.added, f.removed])).toEqual([
      ['src/invoice.ts', 2, 1],
      ['src/rounding.ts', 3, 0],
      ['docs/logo.png', 0, 0],
      ['notes.md', 0, 0],
    ]);
    expect(await c.getDiff('a1', 'head')).toEqual(head);
    expect((await c.getDiff('a1', 'merge_base')).files).toHaveLength(4);
    // a6 has a commit ahead but nothing uncommitted
    expect((await c.getDiff('a6')).files).toEqual([]);
    expect((await c.getDiff('a6', 'merge_base')).files.length).toBeGreaterThan(0);
  });

  it('listFiles: the mock file tree, the changed subset, no status without git', async () => {
    const c = make();
    const all = await c.listFiles('a1');
    expect(all).toMatchObject({ root: '/Users/naim/.lotse/worktrees/billing-api/fix-rounding', is_git: true, truncated: false });
    expect(all.entries.find((e) => e.path === 'src/invoice')).toEqual({ path: 'src/invoice', dir: true, status: null, added: 0, removed: 0 });
    expect(all.entries.filter((e) => !e.dir)).toHaveLength(10);
    const changed = await c.listFiles('a1', true);
    expect(changed.entries.map((e) => [e.path, e.status, e.added, e.removed])).toEqual([
      [TOTALS, 'modified', 2, 1],
      ['src/invoice/round.ts', 'added', 7, 0],
      ['test/totals.spec.ts', 'modified', 4, 1],
    ]);
    expect((await c.listFiles('a2', true)).entries).toEqual([]); // clean
    expect((await c.listFiles('a5')).root).toBe('/Users/naim/Repos/infra-terraform'); // main checkout
    const notes = await c.listFiles('a7');
    expect(notes.is_git).toBe(false);
    expect(notes.entries.every((e) => e.status === null)).toBe(true);
    await expect(c.listFiles('nope')).rejects.toMatchObject({ code: 'not_found' });
  });

  it('readFile: text with a sha, the file at a revision, confined to the session folder', async () => {
    const c = make();
    const f = await c.readFile('a1', TOTALS);
    expect(f).toMatchObject({ path: TOTALS, binary: false });
    expect(f.text).toContain('roundHalfEven(sum, 2)');
    expect(f.sha).toMatch(/^[0-9a-f]{64}$/);
    expect(f.size).toBe(new TextEncoder().encode(f.text).length);
    expect((await c.readFile('a1', TOTALS)).sha).toBe(f.sha);
    const old = await c.readFile('a1', TOTALS, 'HEAD');
    expect(old.text).toContain('Math.round(sum * 100) / 100');
    expect(old.sha).not.toBe(f.sha);
    await expect(c.readFile('a1', 'src/invoice/round.ts', 'HEAD')).rejects.toMatchObject({ code: 'not_found' }); // added since
    await expect(c.readFile('a1', 'nope.ts')).rejects.toMatchObject({ code: 'not_found' });
    await expect(c.readFile('a1', '../secrets')).rejects.toMatchObject({ code: 'invalid_request' });
    await expect(c.readFile('a1', '/etc/passwd')).rejects.toMatchObject({ code: 'invalid_request' });
  });

  it('writeFile edits in memory; a stale expectedSha is a conflict', async () => {
    const c = make();
    const before = await c.readFile('a1', TOTALS);
    const saved = await c.writeFile('a1', TOTALS, 'export {};\n', before.sha);
    expect(saved).toMatchObject({ path: TOTALS, text: 'export {};\n', size: 11 });
    expect(saved.sha).not.toBe(before.sha);
    expect(await c.readFile('a1', TOTALS)).toEqual(saved);
    // the sha the editor still holds is stale now
    await expect(c.writeFile('a1', TOTALS, 'lost\n', before.sha)).rejects.toMatchObject({ code: 'conflict' });
    expect((await c.readFile('a1', TOTALS)).text).toBe('export {};\n');
    // no expectedSha: unguarded
    expect((await c.writeFile('a1', TOTALS, 'again\n')).text).toBe('again\n');
    // other sessions keep their own folder
    expect((await c.readFile('a3', TOTALS)).text).toContain('roundHalfEven(sum, 2)');
    await expect(c.writeFile('a1', 'nope.ts', 'x')).rejects.toMatchObject({ code: 'not_found' });
    await expect(c.writeFile('a1', '../x', 'x')).rejects.toMatchObject({ code: 'invalid_request' });
    await expect(c.writeFile('a1', TOTALS, 'x'.repeat(2 * 1024 * 1024 + 1))).rejects.toMatchObject({ code: 'invalid_request' });
  });

  it('getUsage: the Usage & time panel (41 m, lanes, tokens, spend today)', async () => {
    const u = await make().getUsage('a1');
    expect([u.total_ms, u.working_ms, u.waiting_ms]).toEqual([41 * 60_000, 37 * 60_000, 4 * 60_000]);
    expect(u.started_at).toBe(FIXTURE_NOW - 41 * 60_000);
    expect(u.lanes.map((l) => [l.name, l.kind, l.cost_micros])).toEqual([
      ['lead', 'lead', 820_000],
      ['explore-repo', 'sub_agent', 190_000],
      ['test-runner', 'sub_agent', 130_000],
    ]);
    const lead = u.lanes[0]!;
    expect(lead.spans.map((s) => [s.status, (s.from - u.started_at) / 60_000, (s.to - u.started_at) / 60_000])).toEqual([
      ['run', 0, 3],
      ['perm', 3, 7],
      ['run', 7, 41],
    ]);
    expect(u.tokens).toEqual({ input: 61_000, cache_read: 26_000, cache_write: 0, output: 9_000 });
    expect(u.spend).toEqual({ session: 1_140_000, project: 2_310_000, all: 4_180_000 });
    expect(u).toMatchObject({ model: 'Opus 5.5', priced: true });
    await expect(make().getUsage('nope')).rejects.toMatchObject({ code: 'not_found' });
  });

  it('getTranscript carries the source and the tool status and result', async () => {
    const t = await make().getTranscript('a1');
    expect(t.source).toBe('claude');
    expect(t.turns.filter((x) => x.tool).map((x) => [x.tool?.name, x.tool?.status, x.tool?.result])).toEqual([
      ['Grep', 'done', '3 call sites in totals.ts'],
      ['Edit', 'done', '+2 −1'],
      ['Bash', 'run', undefined],
    ]);
    expect((await make().getTranscript('a2')).source).toBe('codex');
  });

  it('exportTranscript: Markdown, JSON Lines or text; refused where there is no transcript', async () => {
    const c = make();
    const md = await c.exportTranscript('a1', 'markdown');
    expect(md.file_name).toBe('fix-invoice-rounding.md');
    expect(md.content).toContain('# Fix invoice rounding');
    expect(md.content).toContain('**You:** Fix the invoice rounding so totals match the PDF.');
    expect(md.content).toContain('- `Edit src/invoice.ts → +2 −1`');
    expect(md.redactions).toBe(0);
    const jsonl = await c.exportTranscript('a1', 'json_lines');
    expect(jsonl.file_name).toBe('fix-invoice-rounding.jsonl');
    const lines = jsonl.content.trim().split('\n');
    expect(lines).toHaveLength(6);
    expect(JSON.parse(lines[0]!)).toMatchObject({ role: 'user' });
    const txt = await c.exportTranscript('a1', 'text');
    expect(txt.file_name).toBe('fix-invoice-rounding.txt');
    expect(txt.content).toContain('You: ');
    await expect(c.exportTranscript('a7', 'text')).rejects.toMatchObject({ code: 'unavailable' });
    await expect(c.exportTranscript('nope', 'text')).rejects.toMatchObject({ code: 'not_found' });
  });

  it('getSessionInfo and stopTask: the Session panel and its background tasks', async () => {
    const c = make();
    const info = await c.getSessionInfo('a1');
    expect(info).toMatchObject({
      agent_version: '2.1.309',
      model: 'Opus 5.5',
      cwd: '/Users/naim/Repos/billing-api',
      worktree: '/Users/naim/.lotse/worktrees/billing-api/fix-rounding',
      source: 'hook',
    });
    expect(info.machine).not.toBe('');
    expect(info.tasks).toEqual([{ pid: 4711, command: 'npm run dev', started_at: FIXTURE_NOW - 12 * 60_000 }]);
    await c.stopTask('a1', 4711);
    expect((await c.getSessionInfo('a1')).tasks).toEqual([]);
    await expect(c.stopTask('a1', 4711)).rejects.toMatchObject({ code: 'not_found' }); // only pids the daemon listed
    expect(await c.getSessionInfo('a7')).toMatchObject({ agent_version: null, model: null, worktree: null, tasks: [] });
    await expect(c.getSessionInfo('nope')).rejects.toMatchObject({ code: 'not_found' });
  });

  it('connectors: configured from the files, live status only when Settings › External allows it', async () => {
    const c = make();
    const listed = await c.listConnectors('a1');
    expect(listed.map((x) => x.name)).toEqual(['Agent browser', 'Jfrog', 'Mcp atlassian', 'Playwright', 'Mermaid Chart']);
    expect(listed.every((x) => x.agent === 'claude_code' && x.status.state === 'configured')).toBe(true);
    expect((await c.checkConnectors('a1')).every((x) => x.status.state === 'configured')).toBe(true); // mcp_live is off

    const s = await c.getSettings();
    await c.updateSettings({ ...s, external: { ...s.external, mcp_live: true } });
    expect((await c.listConnectors('a1')).every((x) => x.status.state === 'configured')).toBe(true);
    const live = await c.checkConnectors('a1');
    expect(live.map((x) => x.status)).toEqual([
      { state: 'connected', tools: 14 },
      { state: 'connected', tools: 22 },
      { state: 'connected', tools: 61 },
      { state: 'connected', tools: 21 },
      { state: 'needs_auth' },
    ]);
    expect((await c.checkConnectors('a2')).map((x) => [x.name, x.status.state])).toEqual([
      ['Playwright', 'connected'],
      ['GitHub', 'failed'],
    ]);
    expect((await c.checkConnectors('a6')).map((x) => x.name)).toEqual(['Context7']);
    expect(await c.checkConnectors('a7')).toEqual([]);
    await expect(c.listConnectors('nope')).rejects.toMatchObject({ code: 'not_found' });
  });

  it('resolvePermission: a stale prompt id is a conflict and changes nothing', async () => {
    const c = make();
    c.setStatus('a1', 'perm', { tool: 'Bash', summary: 'ls' });
    const id = (await c.listSessions()).find((s) => s.id === 'a1')!.perm!.id!;
    expect(id).toBeTruthy();
    await expect(c.resolvePermission('a1', 'allow', { via: 'notification', promptId: 'stale' })).rejects.toMatchObject({ code: 'conflict' });
    expect((await c.listSessions()).find((s) => s.id === 'a1')?.status).toBe('perm');
    await c.resolvePermission('a1', 'allow', { via: 'notification', promptId: id });
    expect((await c.listSessions()).find((s) => s.id === 'a1')?.status).toBe('run');
    await c.resolvePermission('a3', 'allow'); // no opts: as before
  });

  it('notifications carry banner, sound name and the prompt id; sound-only has no banner', async () => {
    const c = make();
    const events = collect(c);
    const s = await c.getSettings();
    await c.updateSettings({
      ...s,
      notifications: { ...s.notifications, perm: { banner: false, sound: true, allow_deny: true, sound_name: 'Glass' } },
    });
    events.length = 0;
    c.setStatus('a1', 'perm', { tool: 'Bash', summary: 'ls' });
    const id = (await c.listSessions()).find((x) => x.id === 'a1')!.perm!.id;
    expect(events.filter((e) => e.event === 'notification').map((e) => e.data)).toMatchObject([
      { kind: 'perm', banner: false, sound: true, sound_name: 'Glass', prompt_id: id },
    ]);
    events.length = 0;
    c.setStatus('a2', 'fail');
    expect(events.find((e) => e.event === 'notification')?.data).toMatchObject({ kind: 'fail', banner: true, sound: true });
  });

  // The daemon's resolve(): a custom "off" silences the whole event, sound included; a custom "on" takes the sound from the global row.
  it('a custom project mode with the event off notifies nothing, even when the global row has sound on', async () => {
    const c = make();
    const events = collect(c);
    const s = await c.getSettings();
    expect(s.notifications.perm.sound).toBe(true); // the seed: sound on
    await c.updateProject('p1', { notifications: { mode: 'custom', perm: false, fail: true, done: true, sub_agent_done: true } });
    events.length = 0;
    c.setStatus('a1', 'perm', { tool: 'Bash', summary: 'ls' });
    expect(events.some((e) => e.event === 'notification')).toBe(false);
    c.setStatus('a2', 'fail'); // fail is on for this project: it still notifies, sound from the global row
    expect(events.find((e) => e.event === 'notification')?.data).toMatchObject({ kind: 'fail', banner: true, sound: true });
  });

  it('a custom "on" plays the global row\'s sound name only while that row has sound on', async () => {
    const c = make();
    const events = collect(c);
    const s = await c.getSettings();
    await c.updateSettings({
      ...s,
      notifications: { ...s.notifications, done: { banner: false, sound: false, allow_deny: false, sound_name: 'Glass' } },
    });
    await c.updateProject('p1', { notifications: { mode: 'custom', perm: true, fail: true, done: true, sub_agent_done: true } });
    events.length = 0;
    c.setStatus('a1', 'done');
    const note = events.find((e) => e.event === 'notification')?.data as Record<string, unknown>;
    expect(note).toMatchObject({ kind: 'done', banner: true, sound: false });
    expect(note).not.toHaveProperty('sound_name');
  });
});

describe('DaemonClient mirrors the proto', () => {
  // request method (wire name) -> DaemonClient method
  const CLIENT_METHOD: Record<string, string> = {
    ping: 'ping',
    get_snapshot: 'getSnapshot',
    list_projects: 'listProjects',
    add_project: 'addProject',
    update_project: 'updateProject',
    remove_project: 'removeProject',
    move_project: 'moveProject',
    create_workspace: 'createWorkspace',
    rename_workspace: 'renameWorkspace',
    delete_workspace: 'deleteWorkspace',
    list_sessions: 'listSessions',
    create_session: 'createSession',
    rename_session: 'renameSession',
    close_session: 'closeSession',
    retry_session: 'retrySession',
    mark_read: 'markRead',
    resolve_permission: 'resolvePermission',
    attach: 'attach',
    detach: 'detach',
    send_input: 'sendInput',
    resize: 'resize',
    get_settings: 'getSettings',
    update_settings: 'updateSettings',
    detect_launch_commands: 'detectLaunchCommands',
    get_git_info: 'getGitInfo',
    get_diff: 'getDiff',
    get_transcript: 'getTranscript',
    get_review: 'getReview',
    list_files: 'listFiles',
    read_file: 'readFile',
    write_file: 'writeFile',
    get_usage: 'getUsage',
    export_transcript: 'exportTranscript',
    get_session_info: 'getSessionInfo',
    stop_task: 'stopTask',
    list_connectors: 'listConnectors',
    check_connectors: 'checkConnectors',
    mod_attach: 'modAttach',
    resolve_session_start: 'resolveSessionStart',
    hook_event: 'hookEvent',
    shutdown: 'shutdown',
  };

  it('has a client method for every request method in the Rust fixtures', () => {
    const c = make() as unknown as Record<string, unknown>;
    const methods = new Set(requests.map((m) => m.body.request.method));
    expect(methods.size).toBe(Object.keys(CLIENT_METHOD).length);
    for (const m of methods) {
      const name = CLIENT_METHOD[m];
      expect(name, `no mapping for ${m}`).toBeDefined();
      expect(typeof c[name as string], `DaemonClient.${name}`).toBe('function');
    }
  });
});

describe('proto 5: mod data and child sessions', () => {
  it('seed stays the mock data: no proto 5 field on any session or sub-agent', async () => {
    const sessions = await make().listSessions();
    for (const s of sessions) {
      expect(s).not.toHaveProperty('parent_session');
      expect(s).not.toHaveProperty('context_pct');
      for (const sub of s.sub_agents) expect(Object.keys(sub).sort()).toEqual(['id', 'name', 'status']);
    }
    expect(await make().getSnapshot().then((x) => x.usage)).not.toHaveProperty('rate_limits');
  });

  it('carries sub-agent mod data and session mod data through events', async () => {
    const c = make();
    const events = collect(c);
    c.setSubAgents('a1', [
      { id: 'a1-s1', name: 'explore-repo', status: 'run', activity: 'Read src/db', tokens: 18_400, cost_usd: 0.042, model: 'claude-sonnet-5-5', background: true },
      { id: 'a1-s2', name: 'test-runner', status: 'perm', parent: 'a1-s1' },
    ]);
    c.setSessionMeta('a1', { context_pct: 62, source_layer: 'mod', todo: { done: 2, total: 5 } });
    c.setRateLimits([{ kind: 'five_hour', pct: 42, resets_at: FIXTURE_NOW + 3_600_000 }]);
    const a1 = (await c.listSessions()).find((s) => s.id === 'a1');
    expect(a1).toMatchObject({ context_pct: 62, source_layer: 'mod', todo: { done: 2, total: 5 } });
    expect(a1?.sub_agents[0]).toMatchObject({ activity: 'Read src/db', tokens: 18_400, background: true });
    expect(a1?.sub_agents[1]?.parent).toBe('a1-s1');
    expect((await c.getSnapshot()).usage.rate_limits).toEqual([{ kind: 'five_hour', pct: 42, resets_at: FIXTURE_NOW + 3_600_000 }]);
    expect(events.map((e) => e.event)).toEqual(['sub_agents_changed', 'session_updated', 'usage_changed']);
  });

  it('modAttach: mod_command reaches only an attached session', async () => {
    const c = make();
    const events = collect(c);
    expect(c.sendModCommand('a1', { cmd: 'ping' })).toBe(false);
    await c.modAttach('a1');
    expect(c.sendModCommand('a1', { cmd: 'submit', text: 'go' })).toBe(true);
    expect(c.sendModCommand('a2', { cmd: 'ping' })).toBe(false);
    expect(events).toEqual([{ event: 'mod_command', data: { session_id: 'a1', command: { cmd: 'submit', text: 'go' } } }]);
    await expect(c.modAttach('nope')).rejects.toMatchObject({ code: 'not_found' });
  });

  it('a start request asks first; approve starts the child under its parent, deny starts nothing', async () => {
    const c = make();
    const events = collect(c);
    const ask = { parent_session: 'a1', project_id: 'p1', agent: 'claude_code' as const, title: 'Write the migration', prompt: 'Add V43', worktree: null };
    const id = c.requestSessionStart(ask);
    expect(events.at(-1)).toMatchObject({ event: 'session_start_requested', data: { id, ...ask } });
    expect(await c.listSessions()).toHaveLength(7);

    await c.resolveSessionStart(id, true);
    const child = (await c.listSessions()).at(-1);
    expect(child).toMatchObject({ title: 'Write the migration', agent: 'claude_code', parent_session: 'a1' });
    expect(events.at(-1)?.event).toBe('session_created');
    await expect(c.resolveSessionStart(id, true)).rejects.toMatchObject({ code: 'not_found' }); // answered once

    const id2 = c.requestSessionStart(ask);
    await c.resolveSessionStart(id2, false);
    expect(await c.listSessions()).toHaveLength(8);
    await expect(c.resolveSessionStart('nope', false)).rejects.toMatchObject({ code: 'not_found' });
  });

  it('modAttached is cleared when the session closes and when the link drops or returns', async () => {
    const c = make();
    const events = collect(c);
    await c.modAttach('a1');
    await c.closeSession('a1');
    expect(c.sendModCommand('a1', { cmd: 'ping' })).toBe(false);

    await c.modAttach('a2');
    c.setConnection('disconnected');
    expect(c.sendModCommand('a2', { cmd: 'ping' })).toBe(false);
    await c.modAttach('a2');
    c.setConnection('reconnected');
    expect(c.sendModCommand('a2', { cmd: 'ping' })).toBe(false);
    await c.modAttach('a2');
    expect(c.sendModCommand('a2', { cmd: 'ping' })).toBe(true);
    expect(events.filter((e) => e.event === 'mod_command')).toHaveLength(1);
  });

  it('a failed approve keeps the request open: it can be answered again', async () => {
    const c = make();
    const id = c.requestSessionStart({ parent_session: 'a1', project_id: 'p1', agent: 'codex', title: null, prompt: 'x', worktree: null });
    await c.closeSession('a1'); // the parent goes away: the start fails
    await expect(c.resolveSessionStart(id, true)).rejects.toMatchObject({ code: 'not_found' });
    await c.resolveSessionStart(id, false); // still open, so a denial is accepted
    await expect(c.resolveSessionStart(id, false)).rejects.toMatchObject({ code: 'not_found' });
  });

  it('setSessionMeta with undefined removes the key instead of storing undefined', async () => {
    const c = make();
    c.setSessionMeta('a1', { context_pct: 50, todo: { done: 1, total: 2 } });
    const events = collect(c);
    c.setSessionMeta('a1', { context_pct: undefined });
    const data = (events[0] as { data: Record<string, unknown> }).data;
    expect('context_pct' in data).toBe(false);
    expect(data.todo).toEqual({ done: 1, total: 2 });
    expect('context_pct' in ((await c.listSessions()).find((s) => s.id === 'a1') ?? {})).toBe(false);
  });

  it('createSession keeps parent_session; an unknown parent is not_found', async () => {
    const c = make();
    const base = { project_id: 'p1', agent: 'codex' as const, command: null, title: null, worktree: null, first_prompt: null };
    expect(await c.createSession({ ...base, parent_session: 'a1', requested_by_agent: true })).toMatchObject({ parent_session: 'a1' });
    expect(await c.createSession(base)).not.toHaveProperty('parent_session');
    await expect(c.createSession({ ...base, parent_session: 'nope' })).rejects.toMatchObject({ code: 'not_found' });
  });
});

describe('SUB_AGENT_SCRIPT', () => {
  it('starts and stops sub-agents on a Claude, a Codex and an OpenCode session', async () => {
    const c = new MockClient({ now: () => FIXTURE_NOW, script: SUB_AGENT_SCRIPT, tickMs: 0 });
    const events = collect(c);
    c.tick(10_000);
    const names = events.map((e) => e.event);
    expect(names.filter((n) => n === 'sub_agents_changed')).toHaveLength(3);
    expect(names.filter((n) => n === 'sub_agent_status')).toHaveLength(3);
    const byId = new Map((await c.listSessions()).map((s) => [s.id, s]));
    expect(['a1', 'a2', 'a6'].map((id) => byId.get(id)?.agent)).toEqual(['claude_code', 'codex', 'opencode']);
    expect(byId.get('a2')?.sub_agents).toEqual([{ id: 'a2-s1', name: 'schema-check', status: 'done' }]);
    expect(byId.get('a6')?.sub_agents).toEqual([{ id: 'a6-s1', name: 'docs-scan', status: 'done' }]);
  });
});
