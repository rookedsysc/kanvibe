#!/usr/bin/env bash
# Electron QA 진입 스크립트들이 공유하는 준비 단계.
#
# `pnpm qa:*` 를 거치지 않고 이 스크립트들을 bash로 직접 불러도 렌더러 번들과
# better-sqlite3의 Electron ABI가 맞도록, 플로우를 띄우기 직전에 여기서 보장한다.
# 준비가 빠지면 `pnpm test` 가 남긴 Node ABI 때문에 QA가 통째로
# `Module did not self-register` 로 무너지는데, 화면에는 원인이 드러나지 않는다.
#
# node가 아니라 pnpm을 거쳐 부르는 이유는 ensure-native-runtime이 `npm_execpath` 로
# 패키지 매니저를 판별하기 때문이다. node로 직접 부르면 npx 경로로 넘어가고,
# 이 저장소의 devEngines가 pnpm을 요구해 그 자리에서 막힌다.

ensure_qa_electron_prepared() {
  # xvfb 재실행은 같은 스크립트를 처음부터 다시 타므로, 바깥 호출이 끝낸 준비를 반복하지 않는다.
  if [[ "${1:-}" == "--inside-xvfb" ]]; then
    return 0
  fi

  local repo_root
  repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  (cd "$repo_root" && pnpm run qa:electron:prepare)
}
