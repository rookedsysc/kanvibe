#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
NATIVE_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd -- "$NATIVE_ROOT/.." && pwd)"
APP_NAME="KanVibe"
APP_ID="com.kanvibe.desktop"
APP_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$NATIVE_ROOT/crates/kanvibe-app/Cargo.toml" | head -n 1)"
DIST_DIR="$NATIVE_ROOT/dist"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
DMG_PATH="$DIST_DIR/$APP_NAME-$APP_VERSION.dmg"
DMG_CHECKSUM_PATH="$DMG_PATH.sha256"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
HELPERS_DIR="$CONTENTS_DIR/Helpers"
EXECUTABLE_NAME="KanVibe"
UPDATER_EXECUTABLE_NAME="KanVibeUpdater"
BUILD_COMMIT="${KANVIBE_BUILD_COMMIT:-}"

if [[ -z "$BUILD_COMMIT" ]] && command -v git >/dev/null 2>&1; then
  BUILD_COMMIT="$(git -C "$REPO_ROOT" rev-parse HEAD 2>/dev/null || true)"
fi
if [[ ! "$BUILD_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
  BUILD_COMMIT="unknown"
fi

if [[ ! "$APP_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "kanvibe-app package version must be MAJOR.MINOR.PATCH, got: $APP_VERSION" >&2
  exit 65
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS app bundling requires Darwin because GPUI/native-ui and codesign are macOS runtime gates." >&2
  echo "Run cargo build --release on non-macOS hosts to verify portable Rust contracts." >&2
  exit 78
fi

SKIP_SIGN=0
CREATE_DMG=1
RELEASE_MODE=0
SIGN_IDENTITY="${KANVIBE_CODESIGN_IDENTITY:-}"
NOTARY_PROFILE="${KANVIBE_NOTARY_PROFILE:-}"
NOTARY_KEYCHAIN="${KANVIBE_NOTARY_KEYCHAIN:-}"

usage() {
  cat >&2 <<USAGE
usage: $0 [--skip-sign] [--no-dmg]
          [--release --sign-identity "Developer ID Application: …"
                     --notary-profile keychain-profile
                     [--notary-keychain keychain-path]]

Local packaging defaults to an ad-hoc signature. --release requires a
Developer ID identity, a notarytool keychain profile, DMG creation, hardened
runtime signing, universal arm64+x86_64 app/helper binaries, app+DMG
notarization/stapling, and Gatekeeper verification.
The identity/profile may also be supplied through KANVIBE_CODESIGN_IDENTITY
and KANVIBE_NOTARY_PROFILE.
KANVIBE_NOTARY_KEYCHAIN optionally selects the file-based CI keychain that
contains the notarytool profile.
USAGE
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --skip-sign)
      SKIP_SIGN=1
      shift
      ;;
    --no-dmg)
      CREATE_DMG=0
      shift
      ;;
    --release)
      RELEASE_MODE=1
      shift
      ;;
    --sign-identity)
      SIGN_IDENTITY="${2:-}"
      shift 2
      ;;
    --notary-profile)
      NOTARY_PROFILE="${2:-}"
      shift 2
      ;;
    --notary-keychain)
      NOTARY_KEYCHAIN="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage
      exit 64
      ;;
  esac
done

if [[ "$RELEASE_MODE" -eq 1 ]]; then
  if [[ "$SKIP_SIGN" -eq 1 || "$CREATE_DMG" -eq 0 ]]; then
    echo "--release requires signing and DMG creation." >&2
    exit 64
  fi
  if [[ -z "$SIGN_IDENTITY" || -z "$NOTARY_PROFILE" ]]; then
    echo "--release requires --sign-identity and --notary-profile (or their KANVIBE_* environment variables)." >&2
    exit 64
  fi
  for tool in codesign security xcrun ditto plutil spctl shasum git lipo rustup; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "--release requires macOS tool: $tool" >&2
      exit 69
    fi
  done
  if [[ "$BUILD_COMMIT" = "unknown" ]]; then
    echo "--release requires a 40-character source commit through Git or KANVIBE_BUILD_COMMIT." >&2
    exit 64
  fi
  if [[ -n "$(git -C "$REPO_ROOT" status --porcelain)" ]]; then
    echo "--release requires a clean source tree so bundle provenance matches its commit." >&2
    exit 65
  fi
  if ! security find-identity -v -p codesigning | grep -F "\"$SIGN_IDENTITY\"" >/dev/null; then
    echo "Developer ID signing identity is not available in the keychain: $SIGN_IDENTITY" >&2
    exit 69
  fi
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    if ! rustup target list --installed | grep -Fx "$target" >/dev/null; then
      echo "--release universal build requires installed Rust target: $target" >&2
      exit 69
    fi
  done
fi

cd "$NATIVE_ROOT"
if [[ "$RELEASE_MODE" -eq 1 ]]; then
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    KANVIBE_BUILD_COMMIT="$BUILD_COMMIT" \
      cargo build \
        -p kanvibe-app \
        --release \
        --features native-ui \
        --bins \
        --target "$target"
  done
else
  KANVIBE_BUILD_COMMIT="$BUILD_COMMIT" \
    cargo build -p kanvibe-app --release --features native-ui --bins
fi

rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR" "$HELPERS_DIR"
if [[ "$RELEASE_MODE" -eq 1 ]]; then
  lipo -create \
    "$NATIVE_ROOT/target/aarch64-apple-darwin/release/kanvibe-app" \
    "$NATIVE_ROOT/target/x86_64-apple-darwin/release/kanvibe-app" \
    -output "$MACOS_DIR/$EXECUTABLE_NAME"
  lipo -create \
    "$NATIVE_ROOT/target/aarch64-apple-darwin/release/kanvibe-updater" \
    "$NATIVE_ROOT/target/x86_64-apple-darwin/release/kanvibe-updater" \
    -output "$HELPERS_DIR/$UPDATER_EXECUTABLE_NAME"
else
  cp "$NATIVE_ROOT/target/release/kanvibe-app" "$MACOS_DIR/$EXECUTABLE_NAME"
  cp "$NATIVE_ROOT/target/release/kanvibe-updater" "$HELPERS_DIR/$UPDATER_EXECUTABLE_NAME"
fi
chmod 755 "$MACOS_DIR/$EXECUTABLE_NAME"
chmod 755 "$HELPERS_DIR/$UPDATER_EXECUTABLE_NAME"

# 앱은 Contents/Resources를 리소스 루트로 해석한다. 환경변수 없이 Finder에서 실행해도
# 메시지 카탈로그와 최초 실행용 seed DB를 찾을 수 있어야 한다.
if [[ ! -d "$REPO_ROOT/messages" ]]; then
  echo "message catalogs not found at $REPO_ROOT/messages" >&2
  exit 66
fi
cp -R "$REPO_ROOT/messages" "$RESOURCES_DIR/messages"

APP_ICON="$REPO_ROOT/resources/icon.icns"
if [[ ! -f "$APP_ICON" ]]; then
  echo "app icon not found at $APP_ICON" >&2
  exit 66
fi
cp "$APP_ICON" "$RESOURCES_DIR/icon.icns"

BUNDLED_SEED="$REPO_ROOT/resources/database/app.seed.db"
if [[ ! -f "$BUNDLED_SEED" ]]; then
  echo "bundled seed database not found at $BUNDLED_SEED. Run pnpm db:prepare first." >&2
  exit 66
fi
mkdir -p "$RESOURCES_DIR/resources/database"
cp "$BUNDLED_SEED" "$RESOURCES_DIR/resources/database/app.seed.db"

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
  <key>CFBundleIconFile</key>
  <string>icon.icns</string>
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
  <key>KanVibeBuildCommit</key>
  <string>$BUILD_COMMIT</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

printf 'APPL????' > "$CONTENTS_DIR/PkgInfo"

if [[ "$SKIP_SIGN" -eq 0 ]]; then
  if ! command -v codesign >/dev/null 2>&1; then
    echo "Signing requires codesign on macOS; use --skip-sign only for local unsigned QA." >&2
    exit 69
  fi
  if [[ -n "$SIGN_IDENTITY" ]]; then
    codesign \
      --force \
      --options runtime \
      --timestamp \
      --sign "$SIGN_IDENTITY" \
      "$HELPERS_DIR/$UPDATER_EXECUTABLE_NAME"
    codesign \
      --force \
      --options runtime \
      --timestamp \
      --sign "$SIGN_IDENTITY" \
      "$APP_BUNDLE"
  else
    codesign --force --sign - "$HELPERS_DIR/$UPDATER_EXECUTABLE_NAME"
    codesign --force --sign - "$APP_BUNDLE"
  fi
  codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"
fi

if [[ "$RELEASE_MODE" -eq 1 ]]; then
  NOTARY_ARGS=(--keychain-profile "$NOTARY_PROFILE")
  if [[ -n "$NOTARY_KEYCHAIN" ]]; then
    NOTARY_ARGS+=(--keychain "$NOTARY_KEYCHAIN")
  fi
  NOTARY_TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/kanvibe-notary.XXXXXX")"
  cleanup_notary_temp() {
    rm -rf "$NOTARY_TEMP_DIR"
  }
  trap cleanup_notary_temp EXIT
  APP_ARCHIVE="$NOTARY_TEMP_DIR/$APP_NAME-$APP_VERSION.zip"
  ditto -c -k --keepParent "$APP_BUNDLE" "$APP_ARCHIVE"
  xcrun notarytool submit \
    "$APP_ARCHIVE" \
    "${NOTARY_ARGS[@]}" \
    --wait \
    --output-format json \
    > "$DIST_DIR/notary-app.json"
  if [[ "$(plutil -extract status raw -o - "$DIST_DIR/notary-app.json")" != "Accepted" ]]; then
    echo "Apple notarization did not accept the app archive." >&2
    exit 70
  fi
  xcrun stapler staple "$APP_BUNDLE"
  xcrun stapler validate "$APP_BUNDLE"
  spctl --assess --type execute --verbose=2 "$APP_BUNDLE"
fi

du -sh "$APP_BUNDLE"
echo "$APP_BUNDLE"

if [[ "$CREATE_DMG" -eq 1 ]]; then
  if ! command -v hdiutil >/dev/null 2>&1; then
    echo "DMG creation requires hdiutil on macOS." >&2
    exit 69
  fi

  rm -f "$DMG_PATH"
  rm -f "$DMG_CHECKSUM_PATH"
  hdiutil create \
    -volname "$APP_NAME" \
    -srcfolder "$APP_BUNDLE" \
    -ov \
    -format UDZO \
    "$DMG_PATH"

  if [[ -n "$SIGN_IDENTITY" ]]; then
    codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG_PATH"
    codesign --verify --strict --verbose=2 "$DMG_PATH"
  fi

  if [[ "$RELEASE_MODE" -eq 1 ]]; then
    xcrun notarytool submit \
      "$DMG_PATH" \
      "${NOTARY_ARGS[@]}" \
      --wait \
      --output-format json \
      > "$DIST_DIR/notary-dmg.json"
    if [[ "$(plutil -extract status raw -o - "$DIST_DIR/notary-dmg.json")" != "Accepted" ]]; then
      echo "Apple notarization did not accept the DMG." >&2
      exit 70
    fi
    xcrun stapler staple "$DMG_PATH"
    xcrun stapler validate "$DMG_PATH"
    spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG_PATH"
    (
      cd "$DIST_DIR"
      shasum -a 256 "$(basename "$DMG_PATH")" > "$(basename "$DMG_CHECKSUM_PATH")"
    )
    (
      cd "$DIST_DIR"
      shasum -a 256 --check "$(basename "$DMG_CHECKSUM_PATH")"
    )
  fi

  du -sh "$DMG_PATH"
  echo "$DMG_PATH"
  if [[ -f "$DMG_CHECKSUM_PATH" ]]; then
    echo "$DMG_CHECKSUM_PATH"
  fi
fi
