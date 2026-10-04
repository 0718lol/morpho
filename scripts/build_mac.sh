#!/usr/bin/env bash
# Build a slim macOS Morpho.app.
#
# tauri.macos.conf.json keeps engines OUT of the bundle (the Windows NSIS
# build still bundles them via tauri.windows.conf.json). The app instead
# resolves engines through the repo checkout, so:
#   - the .app stays ~10MB instead of ~200MB
#   - the repo must stay where it was built
# For a self-contained distributable .app, put mac engines into
# .app/Contents/Resources/engines (engines.rs looks there too).
set -euo pipefail
cd "$(dirname "$0")/.."

npm run tauri build

APP="target/release/bundle/macos/Morpho.app"
[ -d "$APP" ] || { echo "bundle missing: $APP" >&2; exit 1; }

rm -rf "$APP/Contents/Resources/engines"
ln -sfn "$PWD/engines" "$APP/Contents/MacOS/engines"
codesign --force --deep -s - "$APP"

echo "built: $APP"
if [ "${1:-}" = "--desktop" ]; then
  rm -rf ~/Desktop/Morpho.app
  cp -R "$APP" ~/Desktop/
  echo "copied to ~/Desktop/Morpho.app"
fi
