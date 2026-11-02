// Terminal screens from spec/mock.html (`T`), as ANSI text for xterm.js. MockClient replays them on attach.

const d = (s: string) => `\x1b[90m${s}\x1b[0m`; // dim
const o = (s: string) => `\x1b[33m${s}\x1b[0m`; // orange
const g = (s: string) => `\x1b[32m${s}\x1b[0m`; // green
const r = (s: string) => `\x1b[31m${s}\x1b[0m`; // red
const b = (s: string) => `\x1b[34m${s}\x1b[0m`; // blue
const bold = (s: string) => `\x1b[1m${s}\x1b[0m`;
const rule = d('─'.repeat(76));

const screen = (lines: string[]) => lines.join('\r\n') + '\r\n';

const shell = screen([
  d('Last login: Sun Nov  2 09:14 on ttys004'),
  `naim@mbp ${b('~/Documents/notes')} % ls`,
  '2026-q4-goals.md   standup.md   ideas/',
  `naim@mbp ${b('~/Documents/notes')} % git status`,
  r('fatal: not a git repository (or any of the parent directories): .git'),
  `naim@mbp ${b('~/Documents/notes')} % `,
]).trimEnd();

const claude = screen([
  `${o('✻')} ${bold('Claude Code')} ${d('v2.1.309')}`,
  d('  Opus 5.5 · ~/Repos/billing-api · worktree lotse/fix-rounding'),
  '',
  `${d('❯')} Fix the flaky invoice rounding test and make totals use banker's rounding`,
  '',
  `${b('●')} I'll start by finding the rounding logic and the failing test.`,
  '',
  `${g('●')} ${bold('Task')}(explore-repo)`,
  d('  ⎿  Found 3 call sites in src/invoice/totals.ts'),
  `${g('●')} ${bold('Read')}(src/invoice/totals.ts)`,
  d('  ⎿  Read 212 lines'),
  `${g('●')} ${bold('Update')}(src/invoice/totals.ts)`,
  d('  ⎿  Updated with 2 additions and 1 removal'),
  `     ${d('41')}  ${r('-   return Math.round(sum * 100) / 100')}`,
  `     ${d('41')}  ${g('+   return roundHalfEven(sum, 2)')}`,
  `     ${d('42')}  ${g("+   // banker's rounding avoids 0.005 drift")}`,
  `${b('✻')} ${bold('Task')}(test-runner) ${d('· running 1m 12s')}`,
  d('  ⎿  Running invoice totals suite…'),
  '',
  `${b('✻ Simmering…')} ${d('(2m 41s · ↓ 8.2k tokens · esc to interrupt)')}`,
  '',
  rule,
  `${d('❯')} `,
  rule,
  d('  Opus 5.5 · billing-api · lotse/fix-rounding ↑2 · ctx 38% · $1.14'),
  `  ${o('⏵⏵ auto mode on')} ${d('(shift+tab to cycle) · 1 background agent')}`,
]);

const permHead = [
  `${o('✻')} ${bold('Claude Code')} ${d('v2.1.309')}`,
  d('  Opus 5.5 · ~/Repos/praxis-web · worktree lotse/patients-signals'),
  '',
  `${d('❯')} Move the patient list to Signals and keep the e2e suite green`,
  '',
  `${g('●')} ${bold('Update')}(src/app/patients/patient-list.component.ts)`,
  d('  ⎿  Updated with 18 additions and 24 removals'),
  `${g('●')} ${bold('Task')}(spec-updater) ${d('⎿ Done · 6 specs updated')}`,
  '',
];

const perm = screen([
  ...permHead,
  o('╭──────────────────────────────────────────────────────────────╮'),
  `${o('│')} ${bold('Bash command')}                                                 ${o('│')}`,
  `${o('│')}   npx playwright test e2e/patients                           ${o('│')}`,
  `${o('│')}   ${d('Run patient list e2e tests')}                                 ${o('│')}`,
  `${o('│')}                                                              ${o('│')}`,
  `${o('│')} Do you want to proceed?                                      ${o('│')}`,
  `${o('│')} ${b('❯ 1. Yes')}                                                     ${o('│')}`,
  `${o('│')}   2. Yes, and don't ask again for npx playwright in praxis-web${o('│')}`,
  `${o('│')}   3. No, and tell Claude what to do differently (esc)        ${o('│')}`,
  o('╰──────────────────────────────────────────────────────────────╯'),
]);

const codex = screen([
  d('╭───────────────────────────────────────────╮'),
  `${d('│')} ${bold('>_ Codex CLI')} ${d('(v0.9x)')}                       ${d('│')}`,
  `${d('│')} ${d('model: gpt-5 · dir: ~/Repos/praxis-web')}    ${d('│')}`,
  d('╰───────────────────────────────────────────╯'),
  '',
  d('user'),
  'Bump Angular 19 → 20',
  '',
  d('codex'),
  'Running ng update @angular/core@20 @angular/cli@20',
  '',
  r('✗ npm ERR! ERESOLVE could not resolve peer dependency'),
  r('  @ngrx/store@18 requires @angular/core@^19'),
  '',
  'Task failed after 3 attempts. Exit code 1.',
  '',
  `${d('▌')} ${d('Ask Codex to do anything')}`,
  d('  ⏎ send · ⇧⏎ newline · ctrl+c quit'),
]);

const opencode = screen([
  `${bold('opencode')} ${d('· ~/Repos/dotfiles · session 3f2a')}`,
  '',
  `${d('>')} Migrate prompt to starship and remove p10k`,
  '',
  `${g('✓')} write  .config/starship.toml`,
  `${g('✓')} edit   .zshrc  ${g('+4')} ${r('-11')}`,
  `${g('✓')} bash   starship explain`,
  '',
  'Done. Restart your shell to load the new prompt.',
  '',
  d('─'.repeat(46)),
  `${d('>')} `,
  d('  build · claude-sonnet · ctrl+p commands'),
]);

/** Screen shown for each seeded session id. */
export const MOCK_TERMINAL: Record<string, string> = {
  a1: claude,
  a2: screen([
    d('╭───────────────────────────────────────────╮'),
    `${d('│')} ${bold('>_ Codex CLI')} ${d('(v0.9x)')}                       ${d('│')}`,
    `${d('│')} ${d('model: gpt-5 · dir: ~/Repos/billing-api')}  ${d('│')}`,
    d('╰───────────────────────────────────────────╯'),
    '',
    d('user'),
    'Add Flyway migration V42 for invoice totals',
    '',
    d('codex'),
    `${g('✓')} Created V42__invoice_totals.sql. Migration applied.`,
    '',
    `${d('▌')} ${d('Ask Codex to do anything')}`,
  ]),
  a3: perm,
  a4: codex,
  a5: claude,
  a6: opencode,
  a7: shell,
};

/** Screens after Allow / Deny on a3 (mock `permOk`, `permNo`). */
export const MOCK_PERM_OK = screen([
  ...permHead,
  `${b('●')} ${bold('Bash')}(npx playwright test e2e/patients)`,
  d('  ⎿  Running… 12 passed, 3 pending'),
  '',
  `${b('✻ Testing…')} ${d('(esc to interrupt)')}`,
]);
export const MOCK_PERM_NO = screen([
  ...permHead,
  d('  ⎿  User denied Bash(npx playwright test e2e/patients)'),
  '',
  `${d('❯')} `,
]);

/** Generic screen for a freshly created session. */
export const mockNewSessionScreen = (label: string, cwd: string) =>
  screen([`${d(`${label} · ${cwd}`)}`, '', `${d('❯')} `]);
