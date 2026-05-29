#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_ID="${KANVIBE_QA_RUN_ID:-pr275-pr276-$(date -u +%Y%m%dT%H%M%SZ)}"
RUN_DIR="${KANVIBE_QA_RUN_DIR:-$ROOT_DIR/qa-output/$RUN_ID}"
mkdir -p "$RUN_DIR"

cd "$ROOT_DIR"

echo "[kanvibe-qa] branch: $(git branch --show-current)"
echo "[kanvibe-qa] commit: $(git rev-parse --short HEAD)"

echo "[kanvibe-qa] building desktop app from current source"
pnpm build

echo "[kanvibe-qa] verifying Node native runtime for regression tests"
node scripts/ensure-native-runtime.cjs

echo "[kanvibe-qa] running PR #275/#276 resource-cleanup regression tests"
pnpm exec vitest run \
  src/lib/__tests__/worktree.test.ts \
  src/desktop/main/services/__tests__/kanbanService.test.ts \
  src/desktop/renderer/actions/__tests__/kanban.test.ts \
  --reporter=verbose | tee "$RUN_DIR/resource-cleanup-vitest.log"

echo "[kanvibe-qa] verifying Electron native runtime for video QA"
node scripts/ensure-native-runtime.cjs --electron

echo "[kanvibe-qa] running Electron keyboard/mouse QA with mp4 capture"
KANVIBE_QA_RUN_ID="$RUN_ID" KANVIBE_QA_RUN_DIR="$RUN_DIR" bash scripts/qa-electron-video.sh | tee "$RUN_DIR/electron-video.log"

if [[ ! -s "$RUN_DIR/run.mp4" ]]; then
  echo "[kanvibe-qa] expected video missing or empty: $RUN_DIR/run.mp4" >&2
  exit 1
fi

if [[ ! -s "$RUN_DIR/diagnostics/playwright-trace.zip" ]]; then
  echo "[kanvibe-qa] expected Playwright trace missing or empty: $RUN_DIR/diagnostics/playwright-trace.zip" >&2
  exit 1
fi

if [[ ! -s "$RUN_DIR/diagnostics/cdp-diagnostics.json" ]]; then
  echo "[kanvibe-qa] expected CDP diagnostics missing or empty: $RUN_DIR/diagnostics/cdp-diagnostics.json" >&2
  exit 1
fi

echo "[kanvibe-qa] output: $RUN_DIR"
if [[ -f "$RUN_DIR/report.md" ]]; then
  echo "[kanvibe-qa] report: $RUN_DIR/report.md"
fi
echo "[kanvibe-qa] video: $RUN_DIR/run.mp4"
echo "[kanvibe-qa] trace: $RUN_DIR/diagnostics/playwright-trace.zip"
echo "[kanvibe-qa] cdp: $RUN_DIR/diagnostics/cdp-diagnostics.json"
