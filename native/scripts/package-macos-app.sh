#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
NATIVE_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
APP_NAME="KanVibe"
APP_ID="app.kanvibe.native"
APP_VERSION="0.1.0"
DIST_DIR="$NATIVE_ROOT/dist"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
DMG_PATH="$DIST_DIR/$APP_NAME-$APP_VERSION.dmg"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
EXECUTABLE_NAME="KanVibe"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS app bundling requires Darwin because GPUI/native-ui and codesign are macOS runtime gates." >&2
  echo "Run cargo build --release on non-macOS hosts to verify portable Rust contracts." >&2
  exit 78
fi

SKIP_SIGN=0
CREATE_DMG=1
for arg in "$@"; do
  case "$arg" in
    --skip-sign)
      SKIP_SIGN=1
      ;;
    --no-dmg)
      CREATE_DMG=0
      ;;
    *)
      echo "unknown option: $arg" >&2
      echo "usage: $0 [--skip-sign] [--no-dmg]" >&2
      exit 64
      ;;
  esac
done

cd "$NATIVE_ROOT"
cargo build -p kanvibe-app --release --features native-ui

rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"
cp "$NATIVE_ROOT/target/release/kanvibe-app" "$MACOS_DIR/$EXECUTABLE_NAME"
chmod 755 "$MACOS_DIR/$EXECUTABLE_NAME"

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleExecutable</key>
  <string>$EXECUTABLE_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$APP_ID</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$APP_VERSION</string>
  <key>CFBundleVersion</key>
  <string>$APP_VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

printf 'APPL????' > "$CONTENTS_DIR/PkgInfo"

if [[ "$SKIP_SIGN" -eq 0 ]] && command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP_BUNDLE"
fi

du -sh "$APP_BUNDLE"
echo "$APP_BUNDLE"

if [[ "$CREATE_DMG" -eq 1 ]]; then
  if ! command -v hdiutil >/dev/null 2>&1; then
    echo "DMG creation requires hdiutil on macOS." >&2
    exit 69
  fi

  rm -f "$DMG_PATH"
  hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$APP_BUNDLE" \
    -ov \
    -format UDZO \
    "$DMG_PATH"
  du -sh "$DMG_PATH"
  echo "$DMG_PATH"
fi
