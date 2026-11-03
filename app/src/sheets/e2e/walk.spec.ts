import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';

// Walks every WP5 surface against the MockClient. The project name ("light"/"dark") is the OS scheme;
// the theme preference stays on Auto, so `<html>` must follow it.

const dialog = (page: Page, name: string) => page.getByRole('dialog', { name });

test.beforeEach(async ({ page }, info) => {
  await page.goto('/');
  await expect(page.getByTestId('shell')).toBeVisible();
  await expect(page.locator('html')).toHaveClass(info.project.name === 'dark' ? /dark/ : /^(?!.*dark).*$/);
});

test('New session: agents, command hint, worktree, first prompt', async ({ page }) => {
  await page.keyboard.press('Meta+t');
  const d = dialog(page, 'New session');
  await expect(d).toBeVisible();
  await expect(d.getByRole('button', { name: 'Terminal' })).toHaveAttribute('aria-pressed', 'true');
  await expect(d.getByTestId('cmd-hint')).toContainText('$SHELL -l');
  await d.getByRole('button', { name: 'Claude Code' }).click();
  await expect(d.getByTestId('cmd-hint')).toContainText('claude');
  await expect(d.getByLabel(/First prompt/)).toBeVisible();
  await d.getByRole('checkbox', { name: /worktree/ }).check();
  await expect(d.getByLabel('Worktree branch name')).toHaveValue('lotse/new-task');
  await d.getByLabel('Worktree branch name').fill('bad name');
  await expect(d.getByRole('button', { name: /Start/ })).toBeDisabled();
  await d.getByLabel('Worktree branch name').fill('lotse/e2e');
  await d.getByLabel(/First prompt/).fill('hello');
  await d.getByRole('button', { name: /Start/ }).click();
  await expect(d).toBeHidden();
});

test('Add project: browse path, clone, create, multi-repo', async ({ page }) => {
  await page.keyboard.press('Meta+Shift+o');
  const d = dialog(page, 'Add a project');
  await expect(d).toBeVisible();
  await d.getByRole('button', { name: /Clone from URL/ }).click();
  await d.getByLabel('Repository URL').fill('nope');
  await d.getByRole('button', { name: /Clone ⏎/ }).click();
  await expect(d.getByRole('alert')).toContainText('Use https');
  await d.getByLabel('Repository URL').fill('https://github.com/o/widget.git');
  await d.getByRole('button', { name: /Clone ⏎/ }).click();
  await expect(d).toBeHidden();

  await page.keyboard.press('Meta+Shift+o');
  await d.getByRole('button', { name: /Create new project/ }).click();
  await d.getByRole('button', { name: /Create ⏎/ }).click();
  await expect(d.getByRole('alert')).toContainText('Project name is required');
  await d.getByLabel('Project name').fill('e2e-new');
  await d.getByRole('button', { name: /Create ⏎/ }).click();
  await expect(d).toBeHidden();

  await page.keyboard.press('Meta+Shift+o');
  await d.getByRole('button', { name: /Browse folder/ }).click();
  await d.getByLabel('Folder path').fill('~/Repos/multi-demo');
  await d.getByRole('button', { name: /Add/ }).click();
  await expect(d.getByRole('checkbox')).toHaveCount(3);
  await d.getByRole('button', { name: /Add 3 projects/ }).click();
  await expect(d).toBeHidden();
});

test('Project colour and icon picker', async ({ page }) => {
  // The sidebar menu (WP3) opens it; until then, the dev-only hook.
  await page.evaluate(() => (window as unknown as { __lotse: { openProjectStylePicker(id: string): void } }).__lotse.openProjectStylePicker('p1'));
  const d = dialog(page, 'Project icon and color');
  await expect(d).toBeVisible();
  await expect(d.getByRole('group', { name: 'Colour' }).getByRole('button')).toHaveCount(15);
  await expect(d.getByRole('button', { name: 'purple' })).toHaveAttribute('aria-pressed', 'true');
  await d.getByRole('button', { name: 'pink' }).click();
  await expect(d.getByRole('button', { name: 'pink' })).toHaveAttribute('aria-pressed', 'true');
  await d.getByLabel('Search symbols').fill('rock');
  await d.getByRole('button', { name: 'rocket' }).click();
  await expect(d.getByRole('button', { name: 'rocket' })).toHaveAttribute('aria-pressed', 'true');
  await page.keyboard.press('Escape');
  await expect(d).toBeHidden();
});

test('Command palette: filter, keyboard, jump', async ({ page }) => {
  await page.keyboard.press('Meta+k');
  const d = dialog(page, 'Command palette');
  await expect(d).toBeVisible();
  await expect(d.getByRole('option').first()).toContainText('Patient list');
  await d.getByRole('combobox').fill('angular');
  await expect(d.getByRole('option')).toHaveCount(1);
  await page.keyboard.press('Enter');
  await expect(d).toBeHidden();
  await page.keyboard.press('Meta+k');
  await d.getByRole('combobox').fill('qqq');
  await expect(d.getByText('No matches')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(d).toBeHidden();
});

test('Settings: every section', async ({ page }) => {
  await page.keyboard.press('Meta+,');
  const d = dialog(page, 'Settings');
  await expect(d).toBeVisible();
  await expect(d.getByLabel('Claude Code launch command')).toHaveValue('claude');
  await d.getByLabel('Claude Code launch command').fill('claude --x');
  await d.getByRole('button', { name: 'Reset Claude Code command' }).click();
  await expect(d.getByLabel('Claude Code launch command')).toHaveValue('claude');
  for (const [name, text] of [
    ['Notifications', 'Per-project overrides'],
    ['Appearance', 'preview'],
    ['Remote', 'Not available yet'],
    ['Privacy', 'Zero telemetry'],
    ['Shortcuts', 'Command palette'],
  ]) {
    await d.getByRole('button', { name, exact: true }).click();
    await expect(d.getByText(text as string).first()).toBeVisible();
  }
  await d.getByRole('button', { name: 'Appearance' }).click();
  await d.getByRole('button', { name: 'Dark' }).click();
  await expect(page.locator('html')).toHaveClass(/dark/);
  await d.getByRole('button', { name: 'Light' }).click();
  await expect(page.locator('html')).not.toHaveClass(/dark/);
  await page.keyboard.press('Escape');
  await expect(d).toBeHidden();
});

test('Shortcuts sheet on ⌘/', async ({ page }) => {
  await page.keyboard.press('Meta+/');
  const d = dialog(page, 'Keyboard shortcuts');
  await expect(d).toBeVisible();
  await expect(d).toContainText('⇧⌘O');
  await page.keyboard.press('Escape');
  await expect(d).toBeHidden();
});
