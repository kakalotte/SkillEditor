#!/usr/bin/env bash
#
# Build a macOS .app bundle for the Tauri build of SkillForge.
#
# The Tauri CLI is intentionally not used anywhere in this project (see
# docs/decisions/0001-toolchain-and-tauri-version.md), so this script does the
# last step `tauri build` would normally do: assemble Contents/, write
# Info.plist, copy the icon and ad-hoc sign the result.
#
# A bundle is required for a normal launch: `open SkillForge.app` gives the
# process a proper GUI session and a stable bundle identity, which is what the
# macOS data directories and the Dock icon depend on.
#
# Usage:
#   packaging/make-macos-app.sh
#
# Environment:
#   CONFIG     cargo profile          (default: release)
#   VERSION    CFBundleShortVersion  (default: read from Cargo.toml)
#   BUNDLE_ID  CFBundleIdentifier    (default: the tauri.conf.json identifier)
#   OUT_DIR    output directory      (default: <repo>/dist)
#   NO_SIGN    set to 1 to skip codesign

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

CONFIG="${CONFIG:-release}"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
NO_SIGN="${NO_SIGN:-0}"

# Keep the bundle identity and version in sync with the Rust/Tauri config
# instead of hardcoding them.
read_cargo_version() {
    sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -1
}
read_identifier() {
    sed -n 's/.*"identifier"[[:space:]]*:[[:space:]]*"\(.*\)".*/\1/p' \
        "$ROOT/src-tauri/tauri.conf.json" | head -1
}

VERSION="${VERSION:-$(read_cargo_version)}"
VERSION="${VERSION:-0.1.0}"
BUNDLE_ID="${BUNDLE_ID:-$(read_identifier)}"
BUNDLE_ID="${BUNDLE_ID:-io.skillforge.desktop}"

PROFILE_DIR="$CONFIG"
if [ "$CONFIG" = "dev" ] || [ "$CONFIG" = "debug" ]; then
    PROFILE_DIR="debug"
fi

echo "==> cargo build (profile=$CONFIG, version=$VERSION, id=$BUNDLE_ID)"
cargo build --profile "$CONFIG" -p skillforge --manifest-path "$ROOT/Cargo.toml"

BIN="$ROOT/target/$PROFILE_DIR/skillforge"
if [ ! -x "$BIN" ]; then
    echo "error: expected binary not found: $BIN" >&2
    exit 1
fi

ICNS="$ROOT/src-tauri/icons/icon.icns"
APP="$OUT_DIR/SkillForge.app"

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/skillforge"
chmod +x "$APP/Contents/MacOS/skillforge"
if [ -f "$ICNS" ]; then
    cp "$ICNS" "$APP/Contents/Resources/icon.icns"
fi

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>SkillForge</string>
  <key>CFBundleDisplayName</key><string>SkillForge</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>skillforge</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

if [ "$NO_SIGN" != "1" ] && command -v codesign >/dev/null 2>&1; then
    echo "==> ad-hoc signing"
    # Sign the executable first, then the bundle: modifying a bundle after
    # signing invalidates its signature. Ad-hoc signing is enough for local
    # use; distribution needs a Developer ID certificate and notarisation.
    codesign --force --sign - "$APP/Contents/MacOS/skillforge"
    codesign --force --sign - "$APP"
else
    echo "==> skipped codesign (NO_SIGN=$NO_SIGN)"
fi

echo "==> done: $APP"
echo "    run:  open \"$APP\""
