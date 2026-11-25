#!/usr/bin/env bash
# Linux 릴리스 번들을 만든다: tarball(`kanvibe-<ver>-linux-<arch>.tar.gz`)과 .deb(`kanvibe_<ver>_<deb arch>.deb`).
#
# 배치(두 산출물 같음, tarball은 <prefix> = 압축을 푼 디렉터리, .deb는 /usr):
#   <prefix>/bin/kanvibe                                  strip한 릴리스 바이너리(`packaged` 기능). 글꼴·아이콘·로고는 바이너리에 들어 있다
#   <prefix>/lib/kanvibe/remote-agents/                   원격 agent 바이너리와 SHA256SUMS. 앱이 실행 파일 위치로 찾는다
#                                                         (crates/kanvibe-app/src/desktop_process.rs bundled_agent_binary_directory)
#   <prefix>/share/applications/kanvibe.desktop           StartupWMClass=kanvibe(GPUI app_id)
#   <prefix>/share/pixmaps/kanvibe.png                    resources/branding/kanvibe-emblem.png
#   <prefix>/share/doc/kanvibe/                           LICENSE와 번들 글꼴 라이선스
# 시드 DB는 넣지 않는다. Rust 앱은 빈 데이터 디렉터리에서 스키마와 마이그레이션을 직접 적용한다(kanvibe-db, 시드 경로 None).
#
# 재현성: 파일 시각은 HEAD 커밋 시각(SOURCE_DATE_EPOCH), 소유자 0:0, 이름 순 정렬, gzip -n이다. 같은 바이너리에서 만든
# tarball은 바이트가 같다. 바이너리 자체의 재현성(경로 재배치 등)은 cargo 빌드 설정의 몫이라 여기서 다루지 않는다.
# AppImage는 appimagetool(이 머신에 없음, 내려받기 금지)이 필요해 만들지 않는다.
#
# 사용: packaging/linux/build-linux.sh [--out dist] [--skip-build] [--allow-missing-agents] [--verify]
#   --skip-build: 이미 있는 target/release/kanvibe를 쓴다. `cargo build --release -p kanvibe-app --features packaged`로
#                 만든 바이너리여야 설치된 앱 데이터 디렉터리와 hook 포트 9736을 쓴다(기능 없이 만들면 수동 QA로 뜬다)
#   --verify: 만든 tarball을 임시 디렉터리에 풀고 parity/tools/app-launch/check_release_launch.py로 실행을 확인한다
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# cargo가 쓰는 target 디렉터리. CARGO_TARGET_DIR이 있으면 그 경로에 산출물이 생긴다
cargo_target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
output_dir="$repo_root/dist"
is_build_skipped=false
agent_arguments=()
is_verify_requested=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) output_dir="$2"; shift 2 ;;
    --skip-build) is_build_skipped=true; shift ;;
    --allow-missing-agents) agent_arguments+=(--allow-missing); shift ;;
    --verify) is_verify_requested=true; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ "$(uname -s)" == Linux ]] || { echo "build-linux.sh runs on Linux" >&2; exit 2; }
version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$repo_root/Cargo.toml" \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "kanvibe-app"))')"
arch="$(uname -m)"
case "$arch" in
  x86_64) deb_arch=amd64 ;;
  aarch64) deb_arch=arm64 ;;
  *) echo "unsupported architecture: $arch" >&2; exit 2 ;;
esac
source_date_epoch="$(git -C "$repo_root" log -1 --format=%ct)"
bundle_name="kanvibe-${version}-linux-${arch}"
# 140MB 바이너리를 두 번 strip하므로 tmpfs(/tmp) 대신 디스크의 target/ 아래에서 조립한다
mkdir -p "$cargo_target_dir"
work_dir="$(mktemp -d "$cargo_target_dir/packaging-work.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

build_binaries() {
  if [[ "$is_build_skipped" == false ]]; then
    cargo build --release -j "${CARGO_BUILD_JOBS:-4}" -p kanvibe-app --bin kanvibe --features packaged \
      --manifest-path "$repo_root/Cargo.toml"
  fi
  "$repo_root/packaging/build-remote-agents.sh" --out "$work_dir/agents" "${agent_arguments[@]}"
}

# 접두 디렉터리 하나를 채운다(tarball과 .deb가 같은 배치를 쓴다)
stage_prefix() {
  local prefix="$1"
  install -d "$prefix/bin" "$prefix/lib/kanvibe/remote-agents" "$prefix/share/applications" \
    "$prefix/share/pixmaps" "$prefix/share/doc/kanvibe"
  install -m 0755 "$cargo_target_dir/release/kanvibe" "$prefix/bin/kanvibe"
  strip --strip-unneeded "$prefix/bin/kanvibe"
  if compgen -G "$work_dir/agents/kanvibe-remote-agent-*" >/dev/null; then
    install -m 0755 "$work_dir/agents"/kanvibe-remote-agent-* "$prefix/lib/kanvibe/remote-agents/"
    install -m 0644 "$work_dir/agents/SHA256SUMS" "$prefix/lib/kanvibe/remote-agents/SHA256SUMS"
    (cd "$prefix/lib/kanvibe/remote-agents" && sha256sum --check --quiet SHA256SUMS)
  fi
  install -m 0644 "$repo_root/packaging/linux/kanvibe.desktop" "$prefix/share/applications/kanvibe.desktop"
  install -m 0644 "$repo_root/resources/branding/kanvibe-emblem.png" "$prefix/share/pixmaps/kanvibe.png"
  install -m 0644 "$repo_root/LICENSE" "$prefix/share/doc/kanvibe/LICENSE"
  install -m 0644 "$repo_root"/crates/kanvibe-app/assets/fonts/LICENSE-*.txt "$prefix/share/doc/kanvibe/"
}

# 디렉터리 권한은 빌드 머신 umask를 따르지 않게 고정하고, 시각은 커밋 시각으로 맞춘다
normalize_tree() {
  find "$1" -type d -exec chmod 0755 {} +
  find "$1" -exec touch --no-dereference --date="@$source_date_epoch" {} +
}

build_tarball() {
  local stage="$work_dir/tar/$bundle_name"
  stage_prefix "$stage"
  normalize_tree "$work_dir/tar"
  tar --sort=name --mtime="@$source_date_epoch" --owner=0 --group=0 --numeric-owner --format=gnu \
    -C "$work_dir/tar" -cf - "$bundle_name" | gzip -n -9 > "$output_dir/$bundle_name.tar.gz"
}

build_deb() {
  local root="$work_dir/deb"
  stage_prefix "$root/usr"
  install -d "$root/DEBIAN"
  cat > "$root/DEBIAN/control" <<EOF
Package: kanvibe
Version: $version
Architecture: $deb_arch
Maintainer: KanVibe <noreply@kanvibe.invalid>
Section: devel
Priority: optional
Depends: libc6, libgcc-s1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libvulkan1
Recommends: tmux, git, mesa-vulkan-drivers
Description: KanVibe desktop app
 Kanban board that runs and tracks AI coding agent sessions in tmux or zellij panes.
EOF
  normalize_tree "$root"
  SOURCE_DATE_EPOCH="$source_date_epoch" dpkg-deb --root-owner-group -Zxz --build "$root" \
    "$output_dir/kanvibe_${version}_${deb_arch}.deb" >/dev/null
}

verify_tarball() {
  local extract_dir="$work_dir/verify"
  mkdir -p "$extract_dir"
  tar -C "$extract_dir" -xzf "$output_dir/$bundle_name.tar.gz"
  python3 "$repo_root/parity/tools/app-launch/check_release_launch.py" --app "$extract_dir/$bundle_name/bin/kanvibe"
}

mkdir -p "$output_dir"
build_binaries
build_tarball
build_deb
(cd "$output_dir" && sha256sum "$bundle_name.tar.gz" "kanvibe_${version}_${deb_arch}.deb")
if [[ "$is_verify_requested" == true ]]; then
  verify_tarball
fi
