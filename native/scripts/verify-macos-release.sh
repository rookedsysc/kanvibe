#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
NATIVE_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
APP_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$NATIVE_ROOT/crates/kanvibe-app/Cargo.toml" | head -n 1)"
APP_BUNDLE="$NATIVE_ROOT/dist/KanVibe.app"
DMG_PATH="$NATIVE_ROOT/dist/KanVibe-$APP_VERSION.dmg"
DMG_CHECKSUM_PATH=""
EXPECTED_APP_ID="com.kanvibe.desktop"
EXPECTED_BUILD_COMMIT="${KANVIBE_BUILD_COMMIT:-}"

usage() {
  cat >&2 <<USAGE
usage: $0 [--app path] [--dmg path] [--checksum path] [--version MAJOR.MINOR.PATCH]

Verifies universal arm64+x86_64 binaries, Developer ID signing, hardened
runtime, notarization tickets, Gatekeeper assessment, bundle identity/version,
and the app embedded in the read-only DMG. This is a release gate and
intentionally rejects ad-hoc builds.
USAGE
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --app)
      APP_BUNDLE="${2:-}"
      shift 2
      ;;
    --dmg)
      DMG_PATH="${2:-}"
      shift 2
      ;;
    --checksum)
      DMG_CHECKSUM_PATH="${2:-}"
      shift 2
      ;;
    --version)
      APP_VERSION="${2:-}"
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

if [[ -z "$DMG_CHECKSUM_PATH" ]]; then
  DMG_CHECKSUM_PATH="$DMG_PATH.sha256"
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS release verification requires Darwin Gatekeeper, codesign, stapler, and hdiutil." >&2
  exit 78
fi

for tool in codesign xcrun spctl hdiutil plutil shasum lipo; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "release verification requires macOS tool: $tool" >&2
    exit 69
  fi
done

if [[ ! -d "$APP_BUNDLE" || ! -f "$DMG_PATH" || ! -f "$DMG_CHECKSUM_PATH" ]]; then
  echo "release app, DMG, or checksum is missing: $APP_BUNDLE / $DMG_PATH / $DMG_CHECKSUM_PATH" >&2
  exit 66
fi

verify_bundle() {
  local bundle="$1"
  local signature
  local helper_signature
  local plist="$bundle/Contents/Info.plist"
  local executable="$bundle/Contents/MacOS/KanVibe"
  local updater="$bundle/Contents/Helpers/KanVibeUpdater"
  local build_commit
  local executable_architectures
  local updater_architectures

  [[ -f "$plist" && -x "$executable" && -x "$updater" ]] || {
    echo "invalid KanVibe bundle structure: $bundle" >&2
    return 1
  }
  [[ "$(plutil -extract CFBundleIdentifier raw -o - "$plist")" = "$EXPECTED_APP_ID" ]] || {
    echo "unexpected bundle identifier in $bundle" >&2
    return 1
  }
  [[ "$(plutil -extract CFBundleShortVersionString raw -o - "$plist")" = "$APP_VERSION" ]] || {
    echo "unexpected bundle version in $bundle" >&2
    return 1
  }
  build_commit="$(plutil -extract KanVibeBuildCommit raw -o - "$plist")"
  [[ "$build_commit" =~ ^[0-9a-f]{40}$ ]] || {
    echo "bundle has no valid source commit: $bundle" >&2
    return 1
  }
  if [[ -n "$EXPECTED_BUILD_COMMIT" && "$build_commit" != "$EXPECTED_BUILD_COMMIT" ]]; then
    echo "bundle source commit does not match KANVIBE_BUILD_COMMIT: $bundle" >&2
    return 1
  fi
  executable_architectures="$(lipo -archs "$executable")"
  updater_architectures="$(lipo -archs "$updater")"
  for architecture in arm64 x86_64; do
    grep -qw "$architecture" <<<"$executable_architectures" || {
      echo "app executable is missing universal architecture $architecture: $bundle" >&2
      return 1
    }
    grep -qw "$architecture" <<<"$updater_architectures" || {
      echo "update helper is missing universal architecture $architecture: $bundle" >&2
      return 1
    }
  done

  codesign --verify --deep --strict --verbose=2 "$bundle"
  signature="$(codesign -d --verbose=4 "$bundle" 2>&1)"
  grep -F "Authority=Developer ID Application:" <<<"$signature" >/dev/null || {
    echo "bundle is not signed with Developer ID Application: $bundle" >&2
    return 1
  }
  grep -E '^TeamIdentifier=.+$' <<<"$signature" | grep -v '=not set$' >/dev/null || {
    echo "bundle signature has no TeamIdentifier: $bundle" >&2
    return 1
  }
  grep -E '^Runtime Version=' <<<"$signature" >/dev/null || {
    echo "bundle signature does not enable hardened runtime: $bundle" >&2
    return 1
  }
  codesign --verify --strict --verbose=2 "$updater"
  helper_signature="$(codesign -d --verbose=4 "$updater" 2>&1)"
  grep -F "Authority=Developer ID Application:" <<<"$helper_signature" >/dev/null || {
    echo "update helper is not signed with Developer ID Application: $updater" >&2
    return 1
  }
  grep -F "$(sed -n 's/^TeamIdentifier=/TeamIdentifier=/p' <<<"$signature" | head -n 1)" \
    <<<"$helper_signature" >/dev/null || {
    echo "update helper TeamIdentifier does not match its app: $updater" >&2
    return 1
  }
  grep -E '^Runtime Version=' <<<"$helper_signature" >/dev/null || {
    echo "update helper does not enable hardened runtime: $updater" >&2
    return 1
  }
  xcrun stapler validate "$bundle"
  spctl --assess --type execute --verbose=2 "$bundle"
}

verify_bundle "$APP_BUNDLE"
APP_SIGNATURE="$(codesign -d --verbose=4 "$APP_BUNDLE" 2>&1)"
APP_TEAM_ID="$(sed -n 's/^TeamIdentifier=//p' <<<"$APP_SIGNATURE" | head -n 1)"
APP_BUILD_COMMIT="$(plutil -extract KanVibeBuildCommit raw -o - "$APP_BUNDLE/Contents/Info.plist")"
EXPECTED_DIGEST="$(awk 'NR == 1 { print $1 }' "$DMG_CHECKSUM_PATH")"
EXPECTED_FILE="$(awk 'NR == 1 { print $2 }' "$DMG_CHECKSUM_PATH")"
ACTUAL_DIGEST="$(shasum -a 256 "$DMG_PATH" | awk '{ print $1 }')"
[[ "$EXPECTED_DIGEST" =~ ^[0-9a-f]{64}$ \
  && "$EXPECTED_FILE" = "$(basename "$DMG_PATH")" \
  && "$ACTUAL_DIGEST" = "$EXPECTED_DIGEST" ]] || {
  echo "DMG checksum file does not match the selected DMG." >&2
  exit 1
}
codesign --verify --strict --verbose=2 "$DMG_PATH"
DMG_SIGNATURE="$(codesign -d --verbose=4 "$DMG_PATH" 2>&1)"
grep -F "Authority=Developer ID Application:" <<<"$DMG_SIGNATURE" >/dev/null || {
  echo "DMG is not signed with Developer ID Application." >&2
  exit 1
}
grep -F "TeamIdentifier=$APP_TEAM_ID" <<<"$DMG_SIGNATURE" >/dev/null || {
  echo "DMG TeamIdentifier does not match the app." >&2
  exit 1
}
xcrun stapler validate "$DMG_PATH"
spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG_PATH"

MOUNT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/kanvibe-release-mount.XXXXXX")"
cleanup_mount() {
  if mount | grep -F " on $MOUNT_DIR " >/dev/null; then
    hdiutil detach "$MOUNT_DIR" >/dev/null
  fi
  rmdir "$MOUNT_DIR" 2>/dev/null || true
}
trap cleanup_mount EXIT

hdiutil attach -readonly -nobrowse -mountpoint "$MOUNT_DIR" "$DMG_PATH" >/dev/null
MOUNTED_APP="$MOUNT_DIR/KanVibe.app"
verify_bundle "$MOUNTED_APP"
MOUNTED_SIGNATURE="$(codesign -d --verbose=4 "$MOUNTED_APP" 2>&1)"
grep -F "TeamIdentifier=$APP_TEAM_ID" <<<"$MOUNTED_SIGNATURE" >/dev/null || {
  echo "mounted app TeamIdentifier does not match the packaged app." >&2
  exit 1
}
[[ "$(plutil -extract KanVibeBuildCommit raw -o - "$MOUNTED_APP/Contents/Info.plist")" = "$APP_BUILD_COMMIT" ]] || {
  echo "mounted app source commit does not match the packaged app." >&2
  exit 1
}

echo "PASS: checksum, Developer ID identity, notarization, Gatekeeper, bundle identity, and DMG contents verified."
