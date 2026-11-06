#!/usr/bin/env bash
set -euo pipefail

# 모바일 Maestro 여정을 한 플랫폼에서 끝까지 돌린다.
#
# 격리 데스크탑을 띄우고, 기기가 붙어 있는지 확인하고, 하네스 러너에 넘긴 뒤,
# 여정이 끝나면 데스크탑과 tmux 세션을 되돌린다. 데스크탑은 정적 UI를 포함한 Tauri debug 바이너리로
# 한 번 준비한다. 모바일 앱 빌드와 설치는 별도의 Flutter 툴체인에서 수행한다.
#
# 쓰는 법: bash scripts/qa-mobile.sh android|ios

PLATFORM="${1:-android}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_ID="${MAESTRO_RUN_ID:-$(date +%Y%m%d-%H%M%S)-${PLATFORM}}"
RUN_DIR="${ROOT_DIR}/qa-output/mobile/${RUN_ID}"
FIXTURE_READY_TIMEOUT_SECONDS=300

case "${PLATFORM}" in
  android) APP_ID="com.kanvibe.kanvibe_mobile" ;;
  ios) APP_ID="com.kanvibe.kanvibeMobile" ;;
  *) echo "지원하지 않는 플랫폼: ${PLATFORM} (android 또는 ios)" >&2; exit 2 ;;
esac

cd "${ROOT_DIR}"
mkdir -p "${RUN_DIR}"

if [[ "${PLATFORM}" == "ios" ]] && ! xcrun simctl list devices booted | grep -q "("; then
  echo "부팅된 iOS 시뮬레이터가 없습니다. xcrun simctl boot <이름>으로 먼저 띄우세요." >&2
  exit 3
fi

# Vite 서버 없이 UI를 포함한 네이티브 바이너리를 한 번 준비한다.
pnpm qa:tauri:prepare

# Android는 기기를 두 가지로 손봐 둔다.
#
# 애니메이션을 끄는 것은 화면이 멈춘 뒤에 단정하기 위해서다.
# IME를 내리는 것은 키보드가 접근성 트리를 통째로 부풀려 기기 서버가 죽기 때문이고,
# Maestro는 입력을 IME 없이 넣으므로 꺼도 글자는 그대로 들어간다.
if [[ "${PLATFORM}" == "android" ]]; then
  ADB="${ADB:-${ANDROID_HOME:-$HOME/Library/Android/sdk}/platform-tools/adb}"
  "${ADB}" wait-for-device
  for scale in window_animation_scale transition_animation_scale animator_duration_scale; do
    "${ADB}" shell settings put global "${scale}" 0
  done
  "${ADB}" shell ime list -s | while read -r ime; do "${ADB}" shell ime disable "${ime}" >/dev/null; done

  # 앞선 실행이 터미널에 포커스를 둔 채 끝나면 키보드가 올라온 상태로 남는다.
  # 그 화면의 접근성 트리는 Maestro가 처음 읽기에 너무 커서, 첫 명령을 내기도 전에 기기 서버가 멈춘다.
  "${ADB}" shell am force-stop "${APP_ID}"
fi

node qa/mobile/lib/prepareQaDesktop.cjs --run-dir "${RUN_DIR}" &
PREPARE_PID=$!
trap 'kill "${PREPARE_PID}" 2>/dev/null || true; wait "${PREPARE_PID}" 2>/dev/null || true' EXIT

for _ in $(seq "${FIXTURE_READY_TIMEOUT_SECONDS}"); do
  [[ -f "${RUN_DIR}/fixture.json" ]] && break
  if ! kill -0 "${PREPARE_PID}" 2>/dev/null; then
    echo "격리 데스크탑 준비가 먼저 끝나 버렸습니다. 로그: ${RUN_DIR}/desktop.log" >&2
    exit 4
  fi
  sleep 1
done

if [[ ! -f "${RUN_DIR}/fixture.json" ]]; then
  echo "격리 데스크탑이 ${FIXTURE_READY_TIMEOUT_SECONDS}초 안에 준비되지 않았습니다. 로그: ${RUN_DIR}/desktop.log" >&2
  exit 5
fi

APP_ID="${APP_ID}" \
MAESTRO_PLATFORM="${PLATFORM}" \
MAESTRO_RUN_ID="${RUN_ID}" \
MAESTRO_OUTPUT_DIR="${RUN_DIR}/maestro" \
FLOW_PATH="mobile/.maestro/flows/mobile-journey.yaml" \
  bash scripts/maestro/run-maestro-e2e.sh journey
