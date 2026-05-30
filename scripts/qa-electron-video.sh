#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_ID="${KANVIBE_QA_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
RUN_DIR="${KANVIBE_QA_RUN_DIR:-$ROOT_DIR/qa-output/$RUN_ID}"
VIDEO_PATH="$RUN_DIR/run.mp4"
LOG_PATH="$RUN_DIR/ffmpeg.log"
mkdir -p "$RUN_DIR"

run_flow() {
  node qa/electron/flows/smoke.cjs --run-dir "$RUN_DIR" --run-id "$RUN_ID"
}

record_and_run() {
  local ffmpeg_pid=""
  if command -v ffmpeg >/dev/null 2>&1 && [[ -n "${DISPLAY:-}" ]]; then
    ffmpeg -y \
      -video_size "${KANVIBE_QA_VIDEO_SIZE:-1600x1000}" \
      -framerate "${KANVIBE_QA_FRAMERATE:-15}" \
      -f x11grab \
      -i "${DISPLAY}" \
      -pix_fmt yuv420p \
      "$VIDEO_PATH" >"$LOG_PATH" 2>&1 &
    ffmpeg_pid="$!"
    sleep 1
  fi

  set +e
  run_flow
  local flow_status="$?"
  set -e

  if [[ -n "$ffmpeg_pid" ]]; then
    kill -INT "$ffmpeg_pid" >/dev/null 2>&1 || true
    wait "$ffmpeg_pid" >/dev/null 2>&1 || true
  fi

  if [[ ! -s "$VIDEO_PATH" ]] && command -v ffmpeg >/dev/null 2>&1; then
    local first_shot
    first_shot="$(find "$RUN_DIR/screenshots" -type f -name '*.png' | sort | head -n 1 || true)"
    if [[ -n "$first_shot" ]]; then
      ffmpeg -y -loop 1 -i "$first_shot" -t 5 -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2" -pix_fmt yuv420p "$VIDEO_PATH" >>"$LOG_PATH" 2>&1 || true
    fi
  fi

  exit "$flow_status"
}

cd "$ROOT_DIR"

if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
  if ! command -v xvfb-run >/dev/null 2>&1; then
    echo "[kanvibe-qa] xvfb-run is required for headless Electron QA. Install xvfb or run under a desktop DISPLAY." >&2
    exit 127
  fi
  xvfb-run -a -s "-screen 0 ${KANVIBE_QA_VIDEO_SIZE:-1600x1000}x24" bash -lc "DISPLAY=\$DISPLAY KANVIBE_QA_RUN_ID='$RUN_ID' KANVIBE_QA_RUN_DIR='$RUN_DIR' '$0' --inside-xvfb"
  exit "$?"
fi

record_and_run
