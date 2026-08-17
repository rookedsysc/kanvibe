#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/qa-electron-prepare.sh
source "$ROOT_DIR/scripts/qa-electron-prepare.sh"
RUN_ID="${KANVIBE_QA_RUN_ID:-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
RUN_DIR="${KANVIBE_QA_RUN_DIR:-$ROOT_DIR/qa-output/$RUN_ID}"
mkdir -p "$RUN_DIR"

run_flow() {
  node qa/electron/flows/smoke.cjs --run-dir "$RUN_DIR" --run-id "$RUN_ID"
}

cd "$ROOT_DIR"

ensure_qa_electron_prepared "${1:-}"

if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
  if ! command -v xvfb-run >/dev/null 2>&1; then
    echo "[kanvibe-qa] xvfb-run is required for headless Electron QA. Install xvfb or run under a desktop DISPLAY." >&2
    exit 127
  fi
  xvfb-run -a -s "-screen 0 ${KANVIBE_QA_VIDEO_SIZE:-1600x1000}x24" bash -lc "DISPLAY=\$DISPLAY KANVIBE_QA_RUN_ID='$RUN_ID' KANVIBE_QA_RUN_DIR='$RUN_DIR' '$0' --inside-xvfb"
  exit "$?"
fi

run_flow
