#!/bin/sh
# Build the local Alpha .app: release lotsed + lotse-hook are copied next to the app binary
# (tauri.conf.json › bundle › macOS › files), the bundle is ad-hoc signed and smoke-tested.
#
#   scripts/build-app.sh              build, sign, verify, smoke test
#   scripts/build-app.sh --no-smoke   skip the launch check
#
# Apple Silicon, macOS only. No network is needed once `npm ci` and the cargo cache are in place.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
app="$root/target/release/bundle/macos/Lotse.app"
macos="$app/Contents/MacOS"
smoke=1
[ "${1:-}" = "--no-smoke" ] && smoke=0

cd "$root"
cargo build --release -p lotsed -p lotse-hook
(cd app && npx tauri build --bundles app)

# ad-hoc signature over the whole bundle, helper binaries included
codesign --force --deep -s - "$app"
codesign --verify --deep --strict "$app"
codesign -dv "$app" 2>&1 | grep -E '^(Identifier|Signature|Format)'

for bin in lotsed lotse-hook; do
  [ -x "$macos/$bin" ] || { echo "missing $macos/$bin" >&2; exit 1; }
done
main=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")
echo "bundle: $app ($(du -sh "$app" | cut -f1)), executable $main"

if [ "$smoke" = 1 ]; then
  # Start the bundled app against a throw-away data dir; the app starts its sibling lotsed.
  data=$(mktemp -d)
  "$macos/$main" --isolated-data-dir "$data" >/dev/null 2>&1 &
  app_pid=$!
  i=0
  while [ ! -S "$data/lotsed.sock" ] && [ "$i" -lt 40 ]; do sleep 0.5; i=$((i + 1)); done
  ok=0
  [ -S "$data/lotsed.sock" ] && ok=1
  kill "$app_pid" 2>/dev/null || true
  [ -f "$data/lotsed.pid" ] && kill "$(cat "$data/lotsed.pid")" 2>/dev/null || true
  sleep 1
  rm -rf "$data"
  [ "$ok" = 1 ] || { echo "smoke test failed: no daemon socket after 20 s" >&2; exit 1; }
  echo "smoke test ok: bundled app started its lotsed (socket appeared)"
fi
