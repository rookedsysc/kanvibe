#!/usr/bin/env bash
# macOS 릴리스: KanVibe.app과 dist/KanVibe-<ver>.dmg를 만들고 서명·공증한다(scripts/dist-deploy.cjs와 같은 순서).
#
# 순서: macOS·자격 증명·도구 확인 → 서명 identity 확인 → 릴리스 빌드(호스트 아키텍처) → 원격 agent 네 플랫폼 빌드 →
# .app 조립 → Darwin agent 서명과 SHA256SUMS 재작성 → 앱 서명(Hardened Runtime, entitlements.plist) →
# codesign --verify --deep --strict → DMG(KanVibe-<ver>.dmg) 만들고 서명 → notarytool submit --wait → stapler staple·validate →
# DMG sha256 출력(Homebrew cask `rookedsysc/homebrew-kanvibe`의 sha256에 쓴다).
#
# 자격 증명은 환경 변수로만 받는다. 설정돼 있지 않으면 저장소 루트 .env(커밋 금지, dist-deploy.cjs와 같은 파일)에서
# 아래 네 키만 읽는다. .env를 셸로 실행하지 않는다.
#   CSC_NAME           codesign identity 이름(예: "Developer ID Application: ...")
#   APPLE_API_KEY      App Store Connect API 키(.p8) 경로. ~/로 시작해도 된다
#   APPLE_API_KEY_ID   키 ID
#   APPLE_API_ISSUER   issuer ID
# 원격 agent의 Linux 바이너리는 macOS에서 cargo-zigbuild로 교차 빌드한다(없으면 실패한다).
# 버전은 Cargo(kanvibe-app)에서 읽는다. 릴리스 전에 workspace 버전을 package.json 버전과 맞춰야 이름이 이어진다.
#
# 사용: packaging/macos/build-macos.sh [--unsigned]
#   --unsigned: 서명·공증 없이 .app과 DMG만 만든다(자격 증명 없는 로컬 확인용, 배포 금지)
# 이 스크립트는 Linux 개발 머신에서 실행해 보지 못했다(미검증).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# cargo가 쓰는 target 디렉터리. CARGO_TARGET_DIR이 있으면 그 경로에 산출물이 생긴다
cargo_target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
output_dir="$repo_root/dist"
is_signing_skipped=false
required_credential_keys="CSC_NAME APPLE_API_KEY APPLE_API_KEY_ID APPLE_API_ISSUER"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --unsigned) is_signing_skipped=true; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

fail() {
  echo "[kanvibe] $*" >&2
  exit 1
}

run() {
  echo "[kanvibe] \$ $*"
  "$@"
}

# .env 한 줄에서 KEY=VALUE 값을 꺼낸다(export 접두어, 따옴표, 줄 끝 # 주석 처리)
read_env_file_value() {
  local key="$1" line value
  line="$(grep -E "^[[:space:]]*(export[[:space:]]+)?${key}[[:space:]]*=" "$repo_root/.env" | tail -1 || true)"
  [[ -n "$line" ]] || return 0
  value="${line#*=}"
  value="$(printf '%s' "$value" | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')"
  case "$value" in
    \"*\") value="${value#\"}"; value="${value%\"}" ;;
    \'*\') value="${value#\'}"; value="${value%\'}" ;;
    *) value="$(printf '%s' "$value" | sed -E 's/[[:space:]]#.*$//')" ;;
  esac
  printf '%s' "$value"
}

load_credentials() {
  local key value
  for key in $required_credential_keys; do
    if [[ -z "${!key:-}" && -f "$repo_root/.env" ]]; then
      value="$(read_env_file_value "$key")"
      [[ -n "$value" ]] && printf -v "$key" '%s' "$value"
    fi
    [[ -n "${!key:-}" ]] || fail "missing credential: $key (environment or .env)"
  done
  case "$APPLE_API_KEY" in
    "~/"*) APPLE_API_KEY="$HOME/${APPLE_API_KEY#\~/}" ;;
  esac
  [[ -f "$APPLE_API_KEY" ]] || fail "APPLE_API_KEY does not point to an existing .p8 file: $APPLE_API_KEY"
}

ensure_signing_tools() {
  xcrun --find notarytool >/dev/null 2>&1 || fail "xcrun notarytool is unavailable"
  xcrun --find stapler >/dev/null 2>&1 || fail "xcrun stapler is unavailable"
  security find-identity -v -p codesigning | grep -qF "$CSC_NAME" \
    || fail "CSC_NAME was not found in the codesigning identities: $CSC_NAME"
}

cargo_version() {
  cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
    | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "kanvibe-app"))'
}

assemble_app() {
  local app="$1" agents="$2" version="$3"
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/remote-agents"
  install -m 0755 "$cargo_target_dir/release/kanvibe" "$app/Contents/MacOS/kanvibe"
  sed "s/@VERSION@/$version/g" "$repo_root/packaging/macos/Info.plist.in" > "$app/Contents/Info.plist"
  plutil -lint "$app/Contents/Info.plist" >/dev/null
  printf 'APPL????' > "$app/Contents/PkgInfo"
  install -m 0644 "$repo_root/resources/icon.icns" "$app/Contents/Resources/icon.icns"
  install -m 0755 "$agents"/kanvibe-remote-agent-* "$app/Contents/Resources/remote-agents/"
  install -m 0644 "$agents/SHA256SUMS" "$app/Contents/Resources/remote-agents/SHA256SUMS"
  (cd "$app/Contents/Resources/remote-agents" && shasum -a 256 -c SHA256SUMS >/dev/null)
}

# 공증은 번들 안의 Mach-O를 모두 서명된 것으로 요구한다. Darwin agent는 서명하면 내용이 바뀌므로 해시를 다시 쓴다.
# 앱은 실행할 때 파일에서 해시를 다시 계산하므로 서명된 파일의 해시가 원격 loader에 그대로 넘어간다
sign_app() {
  local app="$1" agent
  for agent in "$app/Contents/Resources/remote-agents"/kanvibe-remote-agent-Darwin-*; do
    run codesign --force --options runtime --timestamp --sign "$CSC_NAME" "$agent"
  done
  (cd "$app/Contents/Resources/remote-agents" && shasum -a 256 kanvibe-remote-agent-* > SHA256SUMS)
  run codesign --force --options runtime --timestamp \
    --entitlements "$repo_root/packaging/macos/entitlements.plist" --sign "$CSC_NAME" "$app"
  run codesign --verify --deep --strict --verbose=2 "$app"
}

create_dmg() {
  local app="$1" dmg="$2" staging
  staging="$(mktemp -d)"
  cp -R "$app" "$staging/"
  ln -s /Applications "$staging/Applications"
  rm -f "$dmg"
  run hdiutil create -volname KanVibe -srcfolder "$staging" -fs HFS+ -format UDZO -ov "$dmg"
  rm -rf "$staging"
}

notarize_dmg() {
  local dmg="$1"
  run codesign --force --timestamp --sign "$CSC_NAME" "$dmg"
  run xcrun notarytool submit "$dmg" --key "$APPLE_API_KEY" --key-id "$APPLE_API_KEY_ID" \
    --issuer "$APPLE_API_ISSUER" --wait
  run xcrun stapler staple "$dmg"
  run xcrun stapler validate "$dmg"
}

main() {
  [[ "$(uname -s)" == Darwin ]] || fail "build-macos.sh requires macOS (codesign, hdiutil, notarytool)"
  if [[ "$is_signing_skipped" == false ]]; then
    load_credentials
    ensure_signing_tools
  fi
  local version work_dir app dmg
  version="$(cargo_version)"
  work_dir="$(mktemp -d)"
  app="$output_dir/mac/KanVibe.app"
  dmg="$output_dir/KanVibe-$version.dmg"
  mkdir -p "$output_dir/mac"

  run cargo build --release -j "${CARGO_BUILD_JOBS:-4}" -p kanvibe-app --bin kanvibe --manifest-path "$repo_root/Cargo.toml"
  run "$repo_root/packaging/build-remote-agents.sh" --out "$work_dir/agents"
  assemble_app "$app" "$work_dir/agents" "$version"
  if [[ "$is_signing_skipped" == false ]]; then
    sign_app "$app"
  fi
  create_dmg "$app" "$dmg"
  if [[ "$is_signing_skipped" == false ]]; then
    notarize_dmg "$dmg"
  fi
  rm -rf "$work_dir"
  echo "[kanvibe] DMG sha256:"
  shasum -a 256 "$dmg"
}

main
