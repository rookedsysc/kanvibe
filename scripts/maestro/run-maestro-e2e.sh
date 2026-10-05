#!/usr/bin/env bash
set -euo pipefail

SCOPE="${1:-smoke}"
APP_ID="${APP_ID:-com.example.app}"
PLATFORM="${MAESTRO_PLATFORM:-android}"
RUN_ID="${MAESTRO_RUN_ID:-$(date +%Y%m%d-%H%M%S)-$$-${SCOPE}}"
OUTPUT_DIR="${MAESTRO_OUTPUT_DIR:-qa-output/maestro/${RUN_ID}}"
FLOW_DIR="${MAESTRO_FLOW_DIR:-.maestro/flows}"
FLOW_PATH="${FLOW_PATH:-${FLOW_DIR}/${SCOPE}.yaml}"

TEST_OUTPUT_DIR="${MAESTRO_TEST_OUTPUT_DIR:-${OUTPUT_DIR}/test-output}"
mkdir -p "${OUTPUT_DIR}/screenshots" "${OUTPUT_DIR}/videos" "${OUTPUT_DIR}/hierarchy" "${TEST_OUTPUT_DIR}"

abspath() {
  python3 - "$1" <<'PY'
from pathlib import Path
import sys
print(Path(sys.argv[1]).expanduser().resolve())
PY
}

OUTPUT_DIR_ABS="$(abspath "${OUTPUT_DIR}")"
TEST_OUTPUT_DIR_ABS="$(abspath "${TEST_OUTPUT_DIR}")"
VIDEO_DIR="${MAESTRO_VIDEO_DIR:-${OUTPUT_DIR}/videos}"
VIDEO_DIR_ABS="$(abspath "${VIDEO_DIR}")"
mkdir -p "${VIDEO_DIR_ABS}"

RECORD_VIDEO_SETTING="${MAESTRO_RECORD_VIDEO:-true}"
case "${RECORD_VIDEO_SETTING}" in
  [Ff][Aa][Ll][Ss][Ee]|0|[Nn][Oo]|[Oo][Ff][Ff])
    RECORD_VIDEO_ENABLED=false
    ;;
  *)
    RECORD_VIDEO_ENABLED=true
    ;;
esac

RECORDING_SKIP_REASON="${MAESTRO_RECORDING_SKIP_REASON:-}"
if [[ "${RECORD_VIDEO_ENABLED}" == true ]]; then
  RECORDING_SKIP_REASON=""
  RECORDING_REQUIREMENT="required"
  RECORDING_STATUS="pending"
else
  RECORDING_REQUIREMENT="skip"
  if [[ -n "${RECORDING_SKIP_REASON//[[:space:]]/}" ]]; then
    RECORDING_STATUS="skipped"
  else
    RECORDING_STATUS="rejected"
  fi
fi

run_with_timeout() {
  local timeout_seconds="$1"
  shift

  if command -v timeout >/dev/null 2>&1; then
    timeout "${timeout_seconds}s" "$@"
    return $?
  fi

  if command -v gtimeout >/dev/null 2>&1; then
    gtimeout "${timeout_seconds}s" "$@"
    return $?
  fi

  python3 - "$timeout_seconds" "$@" <<'PY'
import subprocess
import sys

timeout_seconds = float(sys.argv[1])
command = sys.argv[2:]

try:
    completed = subprocess.run(command, timeout=timeout_seconds)
except subprocess.TimeoutExpired:
    sys.exit(124)

sys.exit(completed.returncode)
PY
}

if [[ -n "${MAESTRO_RECORDING_PATH:-}" ]]; then
  RECORDING_PATH_NO_EXT="${MAESTRO_RECORDING_PATH%.mp4}"
  if [[ "${RECORDING_PATH_NO_EXT}" != /* ]]; then
    RECORDING_PATH_NO_EXT="$(abspath "${RECORDING_PATH_NO_EXT}")"
  fi
else
  RECORDING_BASENAME="${MAESTRO_RECORDING_BASENAME:-${RUN_ID}-${SCOPE}}"
  RECORDING_PATH_NO_EXT="${VIDEO_DIR_ABS}/${RECORDING_BASENAME}"
fi
VIDEO_PATH="${RECORDING_PATH_NO_EXT}.mp4"
export APP_ID
export MAESTRO_RECORDING_PATH="${RECORDING_PATH_NO_EXT}"
export MAESTRO_VIDEO_PATH="${VIDEO_PATH}"

write_preflight_failure() {
  local reason="$1"
  python3 - "$OUTPUT_DIR/result.json" "$reason" "$SCOPE" "$PLATFORM" "$APP_ID" "$FLOW_PATH" "$OUTPUT_DIR" "$TEST_OUTPUT_DIR" "$RECORD_VIDEO_ENABLED" "$RECORDING_REQUIREMENT" "$RECORDING_STATUS" "$RECORDING_SKIP_REASON" "$VIDEO_PATH" <<'PY'
import json, sys
(
    result_path,
    reason,
    scope,
    platform,
    app_id,
    flow_path,
    output_dir,
    test_output_dir,
    record_video,
    recording_requirement,
    recording_status,
    recording_skip_reason,
    video_path,
) = sys.argv[1:]
with open(result_path, "w", encoding="utf-8") as file:
    json.dump({
        "ok": False,
        "scope": scope,
        "platform": platform,
        "appId": app_id,
        "flowPath": flow_path,
        "outputDir": output_dir,
        "testOutputDir": test_output_dir,
        "recordVideo": record_video == "true",
        "recordingRequirement": recording_requirement,
        "recordingStatus": recording_status,
        "recordingSkipReason": recording_skip_reason or None,
        "video": video_path if record_video == "true" else None,
        "artifacts": {
            "result": result_path,
            "testOutputDir": test_output_dir,
            "video": video_path if record_video == "true" else None,
        },
        "errors": [{"stage": "preflight", "message": reason}],
    }, file, ensure_ascii=False, indent=2)
    file.write("\n")
PY
  cat > "${OUTPUT_DIR}/report.md" <<REPORT
# Maestro Mobile QA Report

- Run ID: ${RUN_ID}
- Scope: ${SCOPE}
- Platform: ${PLATFORM}
- App ID: ${APP_ID}
- Flow: ${FLOW_PATH}
- Status: FAIL
- Recording requirement: ${RECORDING_REQUIREMENT}
- Recording status: ${RECORDING_STATUS}
- Recording skip reason: ${RECORDING_SKIP_REASON:-none}

## Preflight Failure

${reason}

## Evidence

- Result JSON: ${OUTPUT_DIR}/result.json
- Maestro output dir: ${TEST_OUTPUT_DIR}
- Video: $([[ "${RECORD_VIDEO_ENABLED}" == true ]] && echo "${VIDEO_PATH}" || echo "disabled")
REPORT
}

preflight_fail() {
  local reason="$1"
  local exit_code="${2:-1}"
  echo "$reason" >&2
  write_preflight_failure "$reason"
  exit "$exit_code"
}

if [[ "${RECORD_VIDEO_ENABLED}" == false && "${RECORDING_STATUS}" == "rejected" ]]; then
  preflight_fail "MAESTRO_RECORD_VIDEO=false requires a non-empty MAESTRO_RECORDING_SKIP_REASON approved in the QA plan." 6
fi

if [[ "${RECORD_VIDEO_ENABLED}" == true && ( -e "${VIDEO_PATH}" || -L "${VIDEO_PATH}" ) ]]; then
  RECORDING_STATUS="rejected"
  preflight_fail "Recording target already exists; refusing to reuse or overwrite it: ${VIDEO_PATH}" 7
fi

if ! command -v maestro >/dev/null 2>&1; then
  preflight_fail "maestro CLI not found. Install with: curl -Ls https://get.maestro.mobile.dev | bash" 127
fi

if [[ ! -f "${FLOW_PATH}" ]]; then
  preflight_fail "Flow not found: ${FLOW_PATH}" 2
fi

if [[ "${PLATFORM}" == "android" ]] && command -v adb >/dev/null 2>&1; then
  ADB_WAIT_TIMEOUT_SECONDS="${ADB_WAIT_TIMEOUT_SECONDS:-30}"
  if ! run_with_timeout "${ADB_WAIT_TIMEOUT_SECONDS}" adb wait-for-device; then
    preflight_fail "No Android device became available within ${ADB_WAIT_TIMEOUT_SECONDS}s. Start an emulator/device or set MAESTRO_PLATFORM to the intended target." 3
  fi
  if [[ "$(adb get-state 2>/dev/null || true)" != "device" ]]; then
    preflight_fail "Android device is not online after adb wait-for-device." 3
  fi
  adb shell settings put global window_animation_scale 0 || true
  adb shell settings put global transition_animation_scale 0 || true
  adb shell settings put global animator_duration_scale 0 || true
fi

if [[ -n "${MOCK_API_HEALTH_URL:-}" ]]; then
  if ! python3 - <<'PY'
import os, sys, time, urllib.request
url = os.environ['MOCK_API_HEALTH_URL']
deadline = time.time() + int(os.environ.get('MOCK_API_TIMEOUT_SECONDS', '30'))
last = None
while time.time() < deadline:
    try:
        with urllib.request.urlopen(url, timeout=3) as response:
            if 200 <= response.status < 300:
                print(f"mock api ready: {url} status={response.status}")
                sys.exit(0)
            last = f"status={response.status}"
    except Exception as error:
        last = error
    time.sleep(1)
print(f"mock api not ready: {url} last_error={last}", file=sys.stderr)
sys.exit(1)
PY
  then
    preflight_fail "Mock API health URL did not return a 2xx response within ${MOCK_API_TIMEOUT_SECONDS:-30}s: ${MOCK_API_HEALTH_URL}" 4
  fi
fi

EXECUTION_FLOW_PATH="${FLOW_PATH}"
RECORDING_WRAPPER_PATH=""
if [[ "${RECORD_VIDEO_ENABLED}" == true ]]; then
  FLOW_PATH_ABS="$(abspath "${FLOW_PATH}")"
  RECORDING_WRAPPER_PATH="${OUTPUT_DIR}/recording-wrapper.yaml"
  # Maestro 2.10은 startRecording 경로가 테스트 출력 폴더 밖으로 풀리면 실행 전에 거부한다.
  # 파일명만 넘겨 Maestro가 자기 폴더에 두게 하고, 끝난 뒤 아래에서 원래 자리로 옮긴다.
  python3 - "$RECORDING_WRAPPER_PATH" "$APP_ID" "$(basename "$MAESTRO_RECORDING_PATH")" "$FLOW_PATH_ABS" <<'PY'
import sys

def yaml_quote(value: str) -> str:
    return '"' + value.replace('\\', '\\\\').replace('"', '\\"') + '"'

wrapper_path, app_id, recording_path, flow_path = sys.argv[1:]
with open(wrapper_path, "w", encoding="utf-8") as file:
    file.write("appId: " + yaml_quote(app_id) + "\n")
    file.write("onFlowComplete:\n")
    file.write("  - stopRecording\n")
    file.write("---\n")
    file.write("- startRecording: " + yaml_quote(recording_path) + "\n")
    file.write("- runFlow: " + yaml_quote(flow_path) + "\n")
    file.write("- stopRecording\n")
PY
  EXECUTION_FLOW_PATH="${RECORDING_WRAPPER_PATH}"
fi

set +e
maestro test \
  -e "APP_ID=${APP_ID}" \
  --test-output-dir "${TEST_OUTPUT_DIR}" \
  --format junit \
  --output "${OUTPUT_DIR}/junit.xml" \
  "${EXECUTION_FLOW_PATH}" 2>&1 | tee "${OUTPUT_DIR}/run.log"
STATUS=${PIPESTATUS[0]}
set -e

if [[ "${RECORD_VIDEO_ENABLED}" == true && ! -s "${VIDEO_PATH}" ]]; then
  # Maestro 2.7+는 startRecording에 절대경로를 줘도 그 자리에 두지 않고
  # --test-output-dir/<flow>/startRecording/<주어진 절대경로> 아래에 중첩해 떨어뜨린다.
  # 요청한 자리만 보고 실패로 판정하기 전에 중첩본을 찾아 원래 자리로 옮긴다.
  NESTED_VIDEO="$(find "${TEST_OUTPUT_DIR_ABS}" -type f -path "*startRecording*" \
    -name "$(basename "${VIDEO_PATH}")" -print -quit 2>/dev/null || true)"
  if [[ -n "${NESTED_VIDEO}" && -s "${NESTED_VIDEO}" ]]; then
    mv "${NESTED_VIDEO}" "${VIDEO_PATH}"
  fi
fi

if [[ "${RECORD_VIDEO_ENABLED}" == true && ${STATUS} -eq 0 && ! -s "${VIDEO_PATH}" ]]; then
  echo "Expected Maestro recording was not created: ${VIDEO_PATH}" | tee -a "${OUTPUT_DIR}/run.log" >&2
  STATUS=5
fi

if [[ "${RECORD_VIDEO_ENABLED}" == true ]]; then
  if [[ -s "${VIDEO_PATH}" ]]; then
    RECORDING_STATUS="recorded"
  else
    RECORDING_STATUS="missing"
  fi
fi

if [[ ${STATUS} -eq 0 ]]; then
  OK_JSON=true
else
  OK_JSON=false
fi

python3 - "$OUTPUT_DIR/result.json" "$OK_JSON" "$SCOPE" "$PLATFORM" "$APP_ID" "$FLOW_PATH" "$EXECUTION_FLOW_PATH" "$OUTPUT_DIR" "$TEST_OUTPUT_DIR" "$RECORD_VIDEO_ENABLED" "$RECORDING_REQUIREMENT" "$RECORDING_STATUS" "$RECORDING_SKIP_REASON" "$VIDEO_PATH" "$RECORDING_WRAPPER_PATH" <<'PY'
import json, sys
(
    result_path,
    ok_json,
    scope,
    platform,
    app_id,
    flow_path,
    execution_flow_path,
    output_dir,
    test_output_dir,
    record_video,
    recording_requirement,
    recording_status,
    recording_skip_reason,
    video_path,
    recording_wrapper_path,
) = sys.argv[1:]
recording_enabled = record_video == "true"
with open(result_path, "w", encoding="utf-8") as file:
    json.dump({
        "ok": ok_json == "true",
        "scope": scope,
        "platform": platform,
        "appId": app_id,
        "flowPath": flow_path,
        "executionFlowPath": execution_flow_path,
        "outputDir": output_dir,
        "testOutputDir": test_output_dir,
        "recordVideo": recording_enabled,
        "recordingRequirement": recording_requirement,
        "recordingStatus": recording_status,
        "recordingSkipReason": recording_skip_reason or None,
        "recordingWrapper": recording_wrapper_path if recording_enabled else None,
        "junit": f"{output_dir}/junit.xml",
        "log": f"{output_dir}/run.log",
        "video": video_path if recording_enabled else None,
        "artifacts": {
            "junit": f"{output_dir}/junit.xml",
            "log": f"{output_dir}/run.log",
            "recordingWrapper": recording_wrapper_path if recording_enabled else None,
            "video": video_path if recording_enabled else None,
            "testOutputDir": test_output_dir,
        },
    }, file, ensure_ascii=False, indent=2)
    file.write("\n")
PY

cat > "${OUTPUT_DIR}/report.md" <<REPORT
# Maestro Mobile QA Report

- Run ID: ${RUN_ID}
- Scope: ${SCOPE}
- Platform: ${PLATFORM}
- App ID: ${APP_ID}
- Flow: ${FLOW_PATH}
- Execution flow: ${EXECUTION_FLOW_PATH}
- Status: $([[ ${STATUS} -eq 0 ]] && echo PASS || echo FAIL)
- Recording requirement: ${RECORDING_REQUIREMENT}
- Recording status: ${RECORDING_STATUS}
- Recording skip reason: ${RECORDING_SKIP_REASON:-none}

## Evidence

- JUnit: ${OUTPUT_DIR}/junit.xml
- Log: ${OUTPUT_DIR}/run.log
- Result JSON: ${OUTPUT_DIR}/result.json
- Maestro output dir: ${TEST_OUTPUT_DIR}
- Video: $([[ "${RECORD_VIDEO_ENABLED}" == true ]] && echo "${VIDEO_PATH}" || echo "disabled")

## Notes

If this failed, inspect the log with the simulator/emulator screenshot, hierarchy, and recording, then prefer selector/state fixes over sleeps.
REPORT

if [[ ${STATUS} -ne 0 ]]; then
  echo "Maestro failed. Artifacts: ${OUTPUT_DIR}" >&2
fi

exit "${STATUS}"
