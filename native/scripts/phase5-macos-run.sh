#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
NATIVE_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd -- "$NATIVE_ROOT/.." && pwd)"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Phase 5 native replay requires Darwin for GPUI, screencapture, codesign, and hdiutil." >&2
  echo "Run cargo tests/builds on non-macOS hosts to verify portable Rust contracts." >&2
  exit 78
fi

RUN_ID=""
APP_BINARY=""
WINDOW_ID="${KANVIBE_QA_WINDOW_ID:-}"
FFMPEG_PATH="${KANVIBE_QA_FFMPEG:-}"
SKIP_PACKAGE=0
PACKAGE_ARGS=()

usage() {
  cat >&2 <<USAGE
usage: $0 [--run-id run-016] [--app-binary path] [--window-id id] [--ffmpeg path] [--skip-package] [--skip-sign] [--no-dmg]

Runs the macOS Phase 5 native QA handoff:
  1. package the release .app/.dmg unless --skip-package is set
  2. build the debug native-ui binary used by the QA socket
  3. generate protocol, replay-plan, in-process replay, real-app replay, visual parity, performance, and full-parity artifacts
  4. place capture paths under qa/parity/<run-id>/screens and qa/parity/<run-id>/videos
  5. copy the mandatory terminal/updater checklists and machine-readable evidence manifest into the run
USAGE
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --run-id)
      RUN_ID="${2:-}"
      shift 2
      ;;
    --app-binary)
      APP_BINARY="${2:-}"
      shift 2
      ;;
    --window-id)
      WINDOW_ID="${2:-}"
      shift 2
      ;;
    --ffmpeg)
      FFMPEG_PATH="${2:-}"
      shift 2
      ;;
    --skip-package)
      SKIP_PACKAGE=1
      shift
      ;;
    --skip-sign|--no-dmg)
      PACKAGE_ARGS+=("$1")
      shift
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

if [[ -z "$WINDOW_ID" || ! "$WINDOW_ID" =~ ^[0-9]+$ ]]; then
  echo "Phase 5 capture requires --window-id or KANVIBE_QA_WINDOW_ID with a numeric macOS window id." >&2
  exit 64
fi

next_run_id() {
  local max=0
  local dir base number

  for dir in "$REPO_ROOT"/qa/parity/run-*; do
    [[ -d "$dir" ]] || continue
    base="$(basename "$dir")"
    number="${base#run-}"
    [[ "$number" =~ ^[0-9]+$ ]] || continue
    if ((10#$number > max)); then
      max=$((10#$number))
    fi
  done

  printf 'run-%03d\n' "$((max + 1))"
}

if [[ -z "$RUN_ID" ]]; then
  RUN_ID="$(next_run_id)"
elif [[ "$RUN_ID" =~ ^[0-9]+$ ]]; then
  RUN_ID="$(printf 'run-%03d' "$((10#$RUN_ID))")"
elif [[ ! "$RUN_ID" =~ ^run-[0-9]+$ ]]; then
  echo "--run-id must be numeric or run-<number>, got: $RUN_ID" >&2
  exit 64
fi

RUN_DIR="$REPO_ROOT/qa/parity/$RUN_ID"
LOG_DIR="$RUN_DIR/logs"
mkdir -p "$RUN_DIR/screens" "$RUN_DIR/videos" "$LOG_DIR"
if [[ ! -e "$RUN_DIR/terminal-runtime-checklist.md" ]]; then
  cp "$REPO_ROOT/qa/checklists/terminal-macos.md" "$RUN_DIR/terminal-runtime-checklist.md"
fi
if [[ ! -e "$RUN_DIR/updater-runtime-checklist.md" ]]; then
  cp "$REPO_ROOT/qa/checklists/updater-macos.md" "$RUN_DIR/updater-runtime-checklist.md"
fi
if [[ ! -e "$RUN_DIR/evidence-manifest.json" ]]; then
  cp "$REPO_ROOT/qa/checklists/phase5-evidence-manifest.template.json" \
    "$RUN_DIR/evidence-manifest.json"
fi
cd "$REPO_ROOT"

if [[ -z "$APP_BINARY" ]]; then
  APP_BINARY="$NATIVE_ROOT/target/debug/kanvibe-app"
fi

run_logged() {
  local name="$1"
  shift
  echo "+ $*" | tee "$LOG_DIR/$name.log"
  "$@" 2>&1 | tee -a "$LOG_DIR/$name.log"
}

if [[ "$SKIP_PACKAGE" -eq 0 ]]; then
  run_logged package-macos-app "$SCRIPT_DIR/package-macos-app.sh" "${PACKAGE_ARGS[@]}"
fi

run_logged cargo-build-debug-native-ui \
  cargo build --manifest-path "$NATIVE_ROOT/Cargo.toml" -p kanvibe-app --features native-ui

export KANVIBE_QA_ARTIFACT_ROOT="qa/parity/$RUN_ID"
export KANVIBE_QA_WINDOW_ID="$WINDOW_ID"
if [[ -n "$FFMPEG_PATH" ]]; then
  export KANVIBE_QA_FFMPEG="$FFMPEG_PATH"
fi

run_harness() {
  local name="$1"
  shift
  run_logged "$name" cargo run --manifest-path "$NATIVE_ROOT/Cargo.toml" -p qa-harness -- "$@"
}

run_harness qa-control \
  qa-control --repo-root "$REPO_ROOT" --output "$RUN_DIR/qa-control-protocol.json"
run_harness qa-replay-plan \
  qa-replay-plan --repo-root "$REPO_ROOT" --output "$RUN_DIR/qa-control-replay-plan.json"
run_harness qa-replay-execute \
  qa-replay-execute --repo-root "$REPO_ROOT" --output "$RUN_DIR/qa-control-replay-execution.json"
run_harness qa-app-launch \
  qa-app-launch --repo-root "$REPO_ROOT" --app-binary "$APP_BINARY" --output "$RUN_DIR/qa-app-launch.json"
run_harness qa-app-replay \
  qa-app-replay --repo-root "$REPO_ROOT" --app-binary "$APP_BINARY" --output "$RUN_DIR/qa-app-replay.json"
run_harness native-visual-parity \
  native-visual-parity --repo-root "$REPO_ROOT" --artifact-root "$RUN_DIR" --output "$RUN_DIR/native-visual-parity.json"
run_harness native-performance \
  native-performance --repo-root "$REPO_ROOT" --output "$RUN_DIR/native-performance.json"
run_harness full-parity \
  full-parity --repo-root "$REPO_ROOT" --output-dir "$RUN_DIR"

echo "Complete the checklist and evidence manifest, then run:" >&2
echo "  $SCRIPT_DIR/verify-phase5-run.sh --run $RUN_DIR" >&2
echo "$RUN_DIR"
