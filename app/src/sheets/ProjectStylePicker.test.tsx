import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { mountWp5 } from './harness';
import { getOverlay, openProjectStylePicker } from './overlay';
import { ICON_GROUPS, SWATCHES, filterIcons } from './symbols';

describe('symbols', () => {
  it('has 15 swatches and the symbol groups', () => {
    expect(Object.keys(SWATCHES)).toHaveLength(15);
    expect(Object.keys(ICON_GROUPS)).toEqual(['Development', 'Objects', 'Nature', 'Symbols']);
  });
  it('filters by name or group and drops empty groups', () => {
    expect(filterIcons('rock')).toEqual([['Development', ['rocket']]]);
    expect(filterIcons('objects')[0]?.[0]).toBe('Objects');
    expect(filterIcons('  STAR ')).toContainEqual(['Symbols', expect.arrayContaining(['star'])]);
    expect(filterIcons('zzz')).toEqual([]);
  });
  it('also matches keywords, SF Symbol names and several words', () => {
    expect(filterIcons('hosting')).toContainEqual(['Development', ['cloud']]);
    expect(filterIcons('ladybug')).toEqual([['Development', ['bug']]]);
    expect(filterIcons('arrow.triangle.branch')).toEqual([['Development', ['git-branch']]]);
    expect(filterIcons('git merge')).toEqual([['Development', ['git-merge']]]);
  });
});

describe('Project colour and icon picker', () => {
  const open = async () => {
    act(() => openProjectStylePicker('p1', { left: 40, bottom: 100 }));
    return screen.findByRole('dialog', { name: 'Project icon and color' });
  };

  it('shows 15 swatches with the current one checked, and the symbol grid', async () => {
    mountWp5();
    const dlg = await open();
    const swatches = within(screen.getByRole('group', { name: 'Colour' })).getAllByRole('button');
    expect(swatches).toHaveLength(15);
    await waitFor(() => expect(within(dlg).getByRole('button', { name: 'purple' })).toHaveAttribute('aria-pressed', 'true'));
    expect(within(dlg).getByRole('button', { name: 'rocket' })).toBeInTheDocument();
    expect(within(dlg).getAllByRole('button', { name: /./ }).length).toBeGreaterThan(50);
  });

  it('persists a colour and an icon through updateProject and stays open', async () => {
    const { client } = mountWp5();
    const update = vi.spyOn(client, 'updateProject');
    const dlg = await open();
    await userEvent.click(within(dlg).getByRole('button', { name: 'pink' }));
    await userEvent.click(within(dlg).getByRole('button', { name: 'rocket' }));
    expect(update).toHaveBeenNthCalledWith(1, 'p1', { color: 'pink' });
    expect(update).toHaveBeenNthCalledWith(2, 'p1', { icon: 'rocket' });
    const saved = (await client.listProjects()).find((p) => p.id === 'p1');
    expect(saved).toMatchObject({ color: 'pink', icon: 'rocket' });
    await waitFor(() => expect(within(dlg).getByRole('button', { name: 'pink' })).toHaveAttribute('aria-pressed', 'true'));
    expect(within(dlg).getByRole('button', { name: 'rocket' })).toHaveAttribute('aria-pressed', 'true');
    expect(getOverlay()?.name).toBe('projectStyle');
  });

  it('searches symbols and says when nothing matches', async () => {
    mountWp5();
    const dlg = await open();
    await userEvent.type(within(dlg).getByLabelText('Search symbols'), 'rock');
    expect(within(dlg).getByRole('button', { name: 'rocket' })).toBeInTheDocument();
    expect(within(dlg).queryByRole('button', { name: 'bug' })).toBeNull();
    await userEvent.clear(within(dlg).getByLabelText('Search symbols'));
    await userEvent.type(within(dlg).getByLabelText('Search symbols'), 'zzzz');
    expect(within(dlg).getByText('No symbols')).toBeInTheDocument();
  });

  it('the search matches keywords, not just names', async () => {
    mountWp5();
    const dlg = await open();
    await userEvent.type(within(dlg).getByLabelText('Search symbols'), 'hosting');
    expect(within(dlg).getByRole('button', { name: 'cloud' })).toBeInTheDocument();
    expect(within(dlg).queryByRole('button', { name: 'rocket' })).toBeNull();
  });

  it('saves an icon picked from the new symbols under its catalogue id', async () => {
    const { client } = mountWp5();
    const dlg = await open();
    await userEvent.click(within(dlg).getByRole('button', { name: 'tortoise' }));
    expect((await client.listProjects()).find((p) => p.id === 'p1')?.icon).toBe('tortoise');
  });

  it('shows a save failure', async () => {
    const { client } = mountWp5();
    vi.spyOn(client, 'updateProject').mockRejectedValue(new Error('disk full'));
    const dlg = await open();
    await userEvent.click(within(dlg).getByRole('button', { name: 'red' }));
    expect(await within(dlg).findByRole('alert')).toHaveTextContent('disk full');
  });

  it('closes on Esc and on a click outside', async () => {
    mountWp5();
    await open();
    await userEvent.keyboard('{Escape}');
    expect(getOverlay()).toBeNull();
    await open();
    await userEvent.click(screen.getByTestId('overlay-backdrop'));
    expect(getOverlay()).toBeNull();
  });
});
