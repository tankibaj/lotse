import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';

// WP3 sidebar + status bar against the dev server (MockClient). Start it first: `npm run dev -- --port 1421`.
// Screenshots for the mock parity gate are written to ../parity/wp3 (committed, small PNGs).

const SHOTS = '../parity/wp3';

// No bundled browser download in this setup: drive the installed Google Chrome (set LOTSE_E2E_BUNDLED=1 to use Playwright's).
test.use({ channel: process.env.LOTSE_E2E_BUNDLED ? undefined : 'chrome' });

async function open(page: Page, theme: 'light' | 'dark', size = { width: 1440, height: 900 }) {
  await page.setViewportSize(size);
  await page.addInitScript((t) => {
    localStorage.setItem('lotse-theme', t);
  }, theme);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.clock.install(); // the MockClient script only advances when a test runs the clock
  await page.goto('/');
  await expect(page.getByTestId('sidebar-region').getByText('billing-api')).toBeVisible();
}

/** Seed many projects through the dev-only handle (`window.__lotse`, MockClient only). */
async function seed(page: Page, projects: number, longName = false) {
  await page.evaluate(
    async ({ projects, longName }) => {
      const { client } = (window as unknown as { __lotse: { client: any } }).__lotse; // eslint-disable-line @typescript-eslint/no-explicit-any
      for (let i = 0; i < projects; i++) {
        const name = longName && i === 0 ? 'a-very-long-project-name-that-would-push-the-sidebar-wider-than-its-limit' : `project-${String(i).padStart(2, '0')}`;
        const p = await client.addProject({ path: `/tmp/${name}`, name, color: null, icon: null });
        await client.createSession({
          project_id: p.id,
          agent: 'claude_code',
          command: null,
          title: longName && i === 0 ? 'An extremely long session title that must be cut with an ellipsis and never widen the sidebar' : `Task ${i}`,
          worktree: { branch: longName && i === 0 ? 'lotse/an-extremely-long-branch-name-for-a-worktree-that-goes-on-and-on' : `lotse/t${i}` },
          first_prompt: null,
        });
      }
    },
    { projects, longName },
  );
}

const sidebar = (page: Page) => page.getByTestId('sidebar-region');
const width = async (page: Page) => (await sidebar(page).boundingBox())?.width ?? 0;

for (const theme of ['light', 'dark'] as const) {
  test.describe(theme, () => {
    test('default view, collapsed projects, perm/fail cards', async ({ page }) => {
      await open(page, theme);
      await expect(sidebar(page)).toBeVisible();
      expect(await width(page)).toBe(260);
      await expect(page.locator('[data-sid]')).toHaveCount(7);
      await page.screenshot({ path: `${SHOTS}/${theme}-1440-default.png` });

      await page.getByRole('button', { name: /infra-terraform/ }).click();
      await page.getByRole('button', { name: /dotfiles/ }).click();
      await page.getByRole('button', { name: /praxis-web/ }).click();
      await expect(page.locator('[data-rollup]')).toHaveCount(4); // perm, fail, run, done
      await page.mouse.move(700, 400);
      await page.screenshot({ path: `${SHOTS}/${theme}-1440-collapsed.png` });
    });

    test('need-you filter and 1280x800', async ({ page }) => {
      await open(page, theme, { width: 1280, height: 800 });
      await page.getByRole('button', { name: '▲ 2 need you' }).click();
      await expect(page.locator('[data-sid]')).toHaveCount(2);
      await page.mouse.move(700, 400);
      await page.screenshot({ path: `${SHOTS}/${theme}-1280-filter.png` });
      await sidebar(page).getByRole('button', { name: 'Show all' }).click();
      await expect(page.locator('[data-sid]')).toHaveCount(7);
    });
  });
}

test('resize 200–420 persists and clamps', async ({ page }) => {
  await open(page, 'light');
  const grip = page.getByRole('separator', { name: 'Resize sidebar' });
  const box = await grip.boundingBox();
  if (!box) throw new Error('no grip');
  const y = box.y + 200;
  const x = box.x + box.width / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 400, y); // far left: clamps to 200
  await page.mouse.up();
  expect(await width(page)).toBe(200);
  // nothing (perm buttons included) is wider than the list at the minimum width
  const nav = page.getByRole('navigation', { name: 'Projects' });
  expect(await nav.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
  await page.screenshot({ path: `${SHOTS}/light-1440-w200.png` });

  const b2 = await page.getByRole('separator', { name: 'Resize sidebar' }).boundingBox();
  if (!b2) throw new Error('no grip');
  await page.mouse.move(b2.x + 2, y);
  await page.mouse.down();
  await page.mouse.move(b2.x + 600, y); // far right: clamps to 420
  await page.mouse.up();
  expect(await width(page)).toBe(420);
  await page.screenshot({ path: `${SHOTS}/light-1440-w420.png` });

  await page.reload();
  await expect(page.getByTestId('sidebar-region').getByText('billing-api')).toBeVisible();
  expect(await width(page)).toBe(420); // persisted
});

/** Per git line: is a part clipped (scrollWidth > clientWidth), and does the line itself overflow its card? */
async function gitLines(page: Page) {
  return page.locator('[data-sid] [data-line="git"]').evaluateAll((lines) =>
    lines.map((line) => {
      const clipped = (part: string) => {
        const el = line.querySelector<HTMLElement>(`[data-part="${part}"]`);
        return el ? el.scrollWidth > el.clientWidth : false;
      };
      const card = line.closest<HTMLElement>('[data-sid]');
      const last = line.lastElementChild?.getBoundingClientRect();
      return {
        sid: card?.dataset.sid ?? '',
        agentClipped: clipped('agent'),
        ahead: clipped('ahead'),
        dirty: clipped('dirty'),
        branchClipped: clipped('branch'),
        agentWidth: line.querySelector<HTMLElement>('[data-part="agent"]')?.getBoundingClientRect().width ?? 0,
        agentNatural: line.querySelector<HTMLElement>('[data-part="agent"]')?.scrollWidth ?? 0,
        overflow: last && card ? last.right > card.getBoundingClientRect().right + 0.5 : false,
      };
    }),
  );
}

test('agent · branch line: the branch truncates, the agent label and counts stay whole, at 260 and 200 px', async ({ page }) => {
  await open(page, 'light');
  let lines = await gitLines(page);
  expect(lines.length).toBeGreaterThan(0);
  for (const l of lines) {
    expect(l.agentClipped, `agent clipped on ${l.sid}`).toBe(false);
    expect(l.agentWidth, `agent shrunk on ${l.sid}`).toBeGreaterThanOrEqual(l.agentNatural - 1); // scrollWidth is rounded
    expect(l.overflow, `line overflows card ${l.sid}`).toBe(false);
  }
  expect(lines.find((l) => l.sid === 'a1')?.branchClipped).toBe(true); // "Claude Code · fix-ro… ↑2 ±3"
  expect(lines.find((l) => l.sid === 'a3')?.branchClipped).toBe(true);

  const grip = page.getByRole('separator', { name: 'Resize sidebar' });
  const box = await grip.boundingBox();
  if (!box) throw new Error('no grip');
  await page.mouse.move(box.x + 2, box.y + 200);
  await page.mouse.down();
  await page.mouse.move(box.x - 400, box.y + 200);
  await page.mouse.up();
  expect(await width(page)).toBe(200);
  lines = await gitLines(page);
  for (const l of lines) {
    expect(l.agentClipped, `agent clipped at 200 on ${l.sid}`).toBe(false);
    expect(l.agentWidth, `agent shrunk at 200 on ${l.sid}`).toBeGreaterThanOrEqual(l.agentNatural - 1); // scrollWidth is rounded
    expect(l.ahead || l.dirty, `counts clipped at 200 on ${l.sid}`).toBe(false);
    expect(l.overflow, `line overflows card at 200 on ${l.sid}`).toBe(false);
  }
});

test('hide and show with ⌃⌘S and the buttons', async ({ page }) => {
  await open(page, 'light');
  await page.keyboard.press('Control+Meta+s');
  await expect(sidebar(page)).toHaveCount(0);
  // the status bar still works with the sidebar hidden
  await expect(page.getByRole('contentinfo')).toContainText('2 need you');
  await page.keyboard.press('Meta+j');
  await expect(page.getByTestId('status-git')).toContainText('patients-signals');
  await page.getByRole('button', { name: 'Show sidebar' }).click();
  await expect(sidebar(page)).toBeVisible();
  await expect(page.locator('[data-sid="a3"]')).toHaveAttribute('aria-current', 'true');
  await page.getByRole('button', { name: /Hide sidebar/ }).click();
  await expect(sidebar(page)).toHaveCount(0);
});

test('30 projects: only the list scrolls, search and header stay fixed, nothing scrolls sideways', async ({ page }) => {
  await open(page, 'light');
  await seed(page, 30, true);
  const nav = page.getByRole('navigation', { name: 'Projects' });
  await expect(nav.getByText('project-29')).toHaveCount(1);
  const m = await nav.evaluate((el) => ({ sh: el.scrollHeight, ch: el.clientHeight, sw: el.scrollWidth, cw: el.clientWidth }));
  expect(m.sh).toBeGreaterThan(m.ch);
  expect(m.sw).toBeLessThanOrEqual(m.cw); // no horizontal scroll, long names truncate
  const search = page.getByRole('button', { name: /Search/ });
  const before = await search.boundingBox();
  await nav.evaluate((el) => (el.scrollTop = el.scrollHeight));
  await expect(nav.getByText('project-29')).toBeInViewport();
  expect(await search.boundingBox()).toEqual(before);
  expect(await width(page)).toBe(260);
  const long = nav.getByText(/^a-very-long-project-name/);
  await expect(long).toHaveAttribute('title', /a-very-long/);
  expect(await long.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true); // clipped with an ellipsis
  await page.screenshot({ path: `${SHOTS}/light-1440-30-projects.png` });
});

test('pointer stays stable: hovering a failed card swaps time for Retry without changing its height', async ({ page }) => {
  await open(page, 'light');
  const a4 = page.locator('[data-sid="a4"]');
  const before = await a4.boundingBox();
  await a4.hover();
  await expect(a4.getByRole('button', { name: /Retry/ })).toBeVisible();
  expect((await a4.boundingBox())?.height).toBe(before?.height);
  await page.screenshot({ path: `${SHOTS}/light-1440-retry-hover.png` });
});

test('context menu look', async ({ page }) => {
  await open(page, 'dark');
  await page.getByRole('button', { name: /billing-api/ }).click({ button: 'right' });
  await page.getByRole('menuitem', { name: /Notifications/ }).hover();
  await expect(page.getByRole('menu', { name: 'Notifications' })).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/dark-1440-project-menu.png` });
});

test('MockClient script updates the sidebar live (a5 done, a2 fails)', async ({ page }) => {
  await open(page, 'light');
  await page.clock.runFor(17_000);
  await expect(page.locator('[data-sid="a2"] [data-status="fail"]')).toBeVisible();
  await expect(page.getByRole('contentinfo')).toContainText('3 need you');
});
