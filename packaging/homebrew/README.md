# Homebrew cask

`lotse.rb` is the template; `.github/workflows/release.yml` fills `@VERSION@`/`@SHA256@` and pushes it to
`tankibaj/homebrew-tap` (`Casks/lotse.rb`) using the `TAP_GITHUB_TOKEN` secret (PAT, contents:write on the tap only).

Install: `brew install --cask tankibaj/tap/lotse`

Release: bump `version` in `app/src-tauri/tauri.conf.json`, `Cargo.toml` (workspace) and `app/package.json`
(the workflow fails on a mismatch) and push to main. Skipped when `v<version>` already exists.

The repo must be public for release assets to download without auth. The build is ad-hoc signed, not notarized;
the cask's `postflight` strips the quarantine attribute. Apple Silicon only.
