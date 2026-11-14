#!/usr/bin/env node
// Licence summary of what ships in the app: Rust crates reachable through normal (non-dev, non-build)
// dependencies of the workspace binaries on aarch64-apple-darwin, and npm packages of app/package-lock.json
// that are not dev-only. Prints a markdown summary; exits 1 on strong copyleft (GPL, AGPL, ...) without
// an `OR` alternative; MPL and other weak copyleft is flagged in the output only.
//
//   node scripts/licenses.mjs            # summary
//   node scripts/licenses.mjs --list     # also every package
//
// Reads local metadata only (cargo metadata --locked, the lockfile); no network.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const list = process.argv.includes('--list');
// Strong copyleft fails the script. Weak, file-level copyleft (MPL, EPL, CDDL, LGPL) and unknown licences
// are listed as flagged for a human decision (THIRD_PARTY_NOTICES.md records the accepted ones).
const STRONG = /\b(A?GPL|SSPL|CC-BY-SA|OSL|EUPL)/i;
const COPYLEFT = /\b(A?GPL|LGPL|MPL|EPL|CDDL|SSPL|CC-BY-SA|OSL|EUPL)/i;

// ---- Rust ------------------------------------------------------------------------------------
const meta = JSON.parse(
  execFileSync(
    'cargo',
    ['metadata', '--format-version', '1', '--locked', '--filter-platform', 'aarch64-apple-darwin'],
    { cwd: root, maxBuffer: 1 << 28, encoding: 'utf8' },
  ),
);
const byId = new Map(meta.packages.map((p) => [p.id, p]));
const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
const seen = new Set();
const queue = [...meta.workspace_members];
while (queue.length) {
  const id = queue.pop();
  if (seen.has(id)) continue;
  seen.add(id);
  for (const d of nodes.get(id)?.deps ?? []) {
    if (d.dep_kinds.some((k) => k.kind === null)) queue.push(d.pkg);
  }
}
const members = new Set(meta.workspace_members);
const rust = [...seen]
  .filter((id) => !members.has(id))
  .map((id) => byId.get(id))
  .map((p) => ({ name: p.name, version: p.version, license: p.license ?? p.license_file ?? 'UNKNOWN' }));

// ---- npm -------------------------------------------------------------------------------------
const lock = JSON.parse(readFileSync(join(root, 'app/package-lock.json'), 'utf8'));
const npm = Object.entries(lock.packages)
  .filter(([path, p]) => path && !p.dev)
  .map(([path, p]) => ({
    name: path.replace(/^.*node_modules\//, ''),
    version: p.version,
    license: p.license ?? 'UNKNOWN',
  }));

function summarize(title, pkgs) {
  const counts = new Map();
  for (const p of pkgs) counts.set(p.license, (counts.get(p.license) ?? 0) + 1);
  const rows = [...counts].sort((a, b) => b[1] - a[1]);
  console.log(`### ${title}: ${pkgs.length} packages\n`);
  console.log('| Licence | Packages |\n|---|---|');
  for (const [lic, n] of rows) console.log(`| ${lic} | ${n} |`);
  console.log();
  const flagged = pkgs.filter((p) => COPYLEFT.test(p.license) || p.license === 'UNKNOWN');
  if (flagged.length) {
    console.log(`Copyleft or unknown (${flagged.length}):\n`);
    for (const p of flagged) console.log(`- ${p.name} ${p.version}: ${p.license}`);
    console.log();
  }
  if (list) {
    for (const p of pkgs.sort((a, b) => a.name.localeCompare(b.name))) {
      console.log(`- ${p.name} ${p.version}: ${p.license}`);
    }
    console.log();
  }
  return flagged;
}

const flagged = [...summarize('Rust (runtime)', rust), ...summarize('npm (runtime, bundled)', npm)];
process.exit(flagged.some((p) => STRONG.test(p.license) && !/\bOR\b/.test(p.license)) ? 1 : 0);
