# Third-party notices

Lotse itself is unlicensed (private, `license = "UNLICENSED"` in `Cargo.toml`). It contains or is built
from the third-party material below. Regenerate the dependency summary with `node scripts/licenses.mjs`
(`--list` for every package); it reads `cargo metadata --locked` and `app/package-lock.json` only.

## Derived or bundled material

| Material | Where | Source | Licence |
|---|---|---|---|
| Codex screen-detection manifest | `integrations/manifests/codex.toml` | herdr, `src/detect/manifests/codex.toml` (<https://github.com/herdrdev/herdr>) | Apache-2.0, text in [`integrations/manifests/LICENSE-herdr`](integrations/manifests/LICENSE-herdr) |
| OpenCode screen-detection manifest | `integrations/manifests/opencode.toml` | herdr, `src/detect/manifests/opencode.toml` | Apache-2.0, same file |
| Recorded Codex terminal transcripts (test fixtures, not in the app bundle) | `integrations/manifests/fixtures/recorded/*.json` | Orca (Lovecast Inc.), `src/main/runtime/__fixtures__` | MIT, [`LICENSE-orca`](integrations/manifests/fixtures/recorded/LICENSE-orca) |
| OpenCode plugin shape and event names | `integrations/opencode/lotse-status.js` | Orca's OpenCode status plugin (API shape and field names only; the code is Lotse's own) | MIT, same `LICENSE-orca` text |

herdr ships no `NOTICE` file and its `LICENSE` names no copyright holder, so there is no attribution notice
to reproduce beyond the licence text. `codex.toml` and `opencode.toml` state in their header that they are
derived from herdr and what was changed (Apache-2.0 §4(b)).

Not bundled, only read at development time: the Orca and herdr checkouts under `research/`. No fonts are
shipped (the UI uses system fonts); icons come from `lucide-react` (ISC, listed below).

## Rust runtime dependencies

275 crates reachable through normal dependencies of `lotsed`, `lotse-hook` and `lotse-app`
(`aarch64-apple-darwin`; dev and build-only dependencies excluded). Counts by SPDX expression, as declared
by each crate:

| Licence | Crates |
|---|---|
| MIT OR Apache-2.0 (and spelling variants `Apache-2.0 OR MIT`, `MIT/Apache-2.0`, `Apache-2.0 / MIT`) | 164 |
| MIT | 53 |
| Unicode-3.0 (incl. `(MIT OR Apache-2.0) AND Unicode-3.0`) | 19 |
| Zlib OR Apache-2.0 OR MIT (any order) | 13 |
| Unlicense OR MIT | 7 |
| **MPL-2.0** | **5** |
| BSD-3-Clause (alone, `AND MIT`, `/MIT`) | 4 |
| Apache-2.0 / Apache-2.0 AND MIT | 3 |
| Zlib | 2 |
| Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 2 |
| 0BSD OR MIT OR Apache-2.0; CC0-1.0 OR MIT-0 OR Apache-2.0; BSD-2-Clause OR Apache-2.0 | 3 |

**Copyleft flag: MPL-2.0 (file-level, weak copyleft), 5 crates:** `selectors 0.38.0`, `cssparser 0.37.0`,
`cssparser-macros 0.7.1`, `dtoa-short 0.3.5` (all pulled in by Tauri's HTML handling) and `option-ext 0.2.0`
(via `dirs`). They are used unmodified as crates.io dependencies, so MPL-2.0 §3.2 is met by the source being
publicly available at the crates.io versions pinned in `Cargo.lock`. Nothing is GPL, LGPL or AGPL. The
crates carry their own licence texts in their published source; a binary distribution beyond personal use
should bundle them (for example with `cargo about`), which this Alpha does not do.

## npm runtime dependencies (bundled in the webview)

13 packages of `app/package-lock.json` outside `dev` (`MIT` 9, `MIT OR Apache-2.0` 3, `ISC` 1):
`@tauri-apps/api`, `@tauri-apps/plugin-dialog`, `@tauri-apps/plugin-notification` (Apache-2.0 OR MIT);
`@xterm/xterm`, `@xterm/addon-fit`, `@xterm/addon-web-links`, `@xterm/addon-webgl`, `react`, `react-dom`,
`scheduler` (MIT); `lucide-react` (ISC); `csstype`, `@types/react` (MIT, types only). No copyleft.

Dev-only packages (Vite, Vitest, Playwright, ESLint, TypeScript, Tailwind, Testing Library, jsdom) are not
part of the app bundle.

## Orca (MIT)

```
MIT License

Copyright (c) 2026 Lovecast Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## herdr (Apache-2.0)

Full text: [`integrations/manifests/LICENSE-herdr`](integrations/manifests/LICENSE-herdr).
