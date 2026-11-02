import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockClient } from '../client';
import { onUiCommand } from '../ui/commands';
import { getSelectedSessionId, selectSession } from '../ui/selection';
import { startSession } from './actions';

afterEach(() => selectSession(null));

const workspace = async (client: MockClient) => ({ projects: await client.listProjects(), sessions: await client.listSessions() });

describe('startSession', () => {
  it("creates the session in the selected session's project with no worktree and selects it", async () => {
    const client = new MockClient({ script: false });
    const spy = vi.spyOn(client, 'createSession');
    selectSession('a3'); // praxis-web
    const created = await startSession(client, await workspace(client), 'claude_code');
    expect(spy).toHaveBeenCalledWith({ project_id: 'p2', agent: 'claude_code', command: null, title: null, worktree: null, first_prompt: null });
    expect(getSelectedSessionId()).toBe(created?.id);
    client.close();
  });

  it('falls back to the first project when nothing is selected', async () => {
    const client = new MockClient({ script: false });
    const ws = await workspace(client);
    await startSession(client, ws, 'terminal');
    expect((await client.listSessions()).at(-1)).toMatchObject({ project_id: ws.projects[0]?.id, agent: 'terminal' });
    client.close();
  });

  it('opens Add project when there is no project', async () => {
    const client = new MockClient({ script: false });
    const spy = vi.spyOn(client, 'createSession');
    const add = vi.fn();
    const off = onUiCommand('openAddProject', add);
    expect(await startSession(client, { projects: [], sessions: [] }, 'terminal')).toBeNull();
    expect(add).toHaveBeenCalledTimes(1);
    expect(spy).not.toHaveBeenCalled();
    off();
    client.close();
  });
});
