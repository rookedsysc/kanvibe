#!/usr/bin/env bash
# 원격 호스트에 올릴 kanvibe-remote-agent 정적 바이너리를 플랫폼별로 빌드해 한 디렉터리에 모은다.
#
# 파일 이름은 앱이 찾는 `kanvibe-remote-agent-<uname -s>-<uname -m>`(crates/kanvibe-remote-host/src/agent_binaries.rs)다.
# 앱은 시작할 때 이 파일들의 sha256을 직접 계산해 원격 loader에 넘기고, 올리기 직전에 파일을 다시 읽어 같은지 확인한다.
# 그래서 번들에 해시를 따로 박아 둘 필요는 없고, 여기서 만드는 SHA256SUMS는 번들에 복사한 파일이 빌드 결과와 같은지
# `sha256sum -c`로 확인하고 릴리스 기록으로 남기는 용도다. macOS 서명은 Darwin 바이너리를 바꾸므로 서명 뒤 다시 만든다.
#
# 대상 플랫폼과 Rust target:
#   Linux-x86_64  x86_64-unknown-linux-musl     Linux-aarch64  aarch64-unknown-linux-musl
#   Darwin-x86_64 x86_64-apple-darwin           Darwin-arm64   aarch64-apple-darwin
# 호스트와 OS가 같은 target은 cargo로, 다른 OS는 cargo-zigbuild가 있을 때만 빌드한다. rustup target이 없거나 빌드할 수 없는
# 플랫폼은 --allow-missing이 없으면 실패한다(릴리스는 네 플랫폼이 모두 있어야 한다).
#
# 사용: packaging/build-remote-agents.sh --out <dir> [--platform <OS-arch>]... [--allow-missing]
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# cargo가 쓰는 target 디렉터리. CARGO_TARGET_DIR이 있으면 그 경로에 산출물이 생긴다
cargo_target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
output_dir=""
requested_platforms=()
is_missing_allowed=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) output_dir="$2"; shift 2 ;;
    --platform) requested_platforms+=("$2"); shift 2 ;;
    --allow-missing) is_missing_allowed=true; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ -n "$output_dir" ]] || { echo "--out is required" >&2; exit 2; }
if [[ ${#requested_platforms[@]} -eq 0 ]]; then
  requested_platforms=(Linux-x86_64 Linux-aarch64 Darwin-x86_64 Darwin-arm64)
fi

rust_target_for() {
  case "$1" in
    Linux-x86_64) echo x86_64-unknown-linux-musl ;;
    Linux-aarch64) echo aarch64-unknown-linux-musl ;;
    Darwin-x86_64) echo x86_64-apple-darwin ;;
    Darwin-arm64) echo aarch64-apple-darwin ;;
    *) echo "unknown agent platform: $1" >&2; return 1 ;;
  esac
}

host_platform="$(uname -s)-$(uname -m)"

# 빌드 명령을 고른다. 고를 수 없으면 아무것도 쓰지 않는다
cargo_subcommand_for() {
  local platform="$1"
  if [[ "${platform%%-*}" == "${host_platform%%-*}" ]]; then
    echo build
  elif command -v cargo-zigbuild >/dev/null 2>&1; then
    echo zigbuild
  fi
}

# ring(rustls 암호 공급자)은 musl target용 C 컴파일러가 필요하다. 교차 컴파일러가 없고 호스트와 같은 아키텍처면
# 호스트 gcc를 이 cargo 실행에만 쓴다. glibc의 _FORTIFY_SOURCE는 musl에 없는 __*_chk 기호를 부르므로 끈다
musl_compiler_environment() {
  local platform="$1" target="$2"
  local arch="${target%%-*}" target_key="${target//-/_}"
  if [[ "$target" != *-linux-musl ]] || command -v "${arch}-linux-musl-gcc" >/dev/null 2>&1; then
    return
  fi
  if [[ "$platform" == "$host_platform" ]]; then
    echo "CC_${target_key}=gcc" "CFLAGS_${target_key}=-U_FORTIFY_SOURCE"
  fi
}

build_agent() {
  local platform="$1" target subcommand
  target="$(rust_target_for "$platform")"
  if ! rustup target list --installed | grep -qx "$target"; then
    echo "[agents] $platform: rustup target $target is not installed" >&2
    return 1
  fi
  subcommand="$(cargo_subcommand_for "$platform")"
  if [[ -z "$subcommand" ]]; then
    echo "[agents] $platform: cross-OS build needs cargo-zigbuild" >&2
    return 1
  fi
  # shellcheck disable=SC2046 # 비었거나 공백 없는 KEY=VALUE 목록이다
  env $(musl_compiler_environment "$platform" "$target") \
    cargo "$subcommand" --release -j "${CARGO_BUILD_JOBS:-4}" -p kanvibe-remote-agent --target "$target" \
    --config 'profile.release.strip=true' --manifest-path "$repo_root/Cargo.toml"
  install -m 0755 "$cargo_target_dir/$target/release/kanvibe-remote-agent" "$output_dir/kanvibe-remote-agent-$platform"
}

write_checksums() {
  (cd "$output_dir" && if command -v sha256sum >/dev/null 2>&1; then
     sha256sum kanvibe-remote-agent-* > SHA256SUMS
   else
     shasum -a 256 kanvibe-remote-agent-* > SHA256SUMS
   fi)
}

mkdir -p "$output_dir"
missing_platforms=()
for platform in "${requested_platforms[@]}"; do
  if ! build_agent "$platform"; then
    missing_platforms+=("$platform")
  fi
done
if [[ ${#missing_platforms[@]} -gt 0 ]]; then
  echo "[agents] missing platforms: ${missing_platforms[*]}" >&2
  [[ "$is_missing_allowed" == true ]] || exit 1
fi
if compgen -G "$output_dir/kanvibe-remote-agent-*" >/dev/null; then
  write_checksums
  cat "$output_dir/SHA256SUMS"
fi
