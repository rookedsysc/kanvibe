#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_ID="${KANVIBE_QA_RUN_ID:-pr275-pr276-$(date -u +%Y%m%dT%H%M%SZ)}"
RUN_DIR="${KANVIBE_QA_RUN_DIR:-$ROOT_DIR/qa-output/$RUN_ID}"
mkdir -p "$RUN_DIR"

cd "$ROOT_DIR"

echo "[kanvibe-qa] branch: $(git branch --show-current)"
echo "[kanvibe-qa] commit: $(git rev-parse --short HEAD)"

if [[ ! -f "$ROOT_DIR/build/renderer/index.html" || ! -f "$ROOT_DIR/build/main/src/desktop/main/serviceRegistry.js" ]]; then
  echo "[kanvibe-qa] desktop build missing; running pnpm build"
  pnpm build
fi

echo "[kanvibe-qa] running PR #275/#276 targeted worktree cleanup tests"
pnpm exec vitest run src/lib/__tests__/worktree.test.ts --reporter=verbose | tee "$RUN_DIR/worktree-vitest.log"

echo "[kanvibe-qa] running Electron video smoke QA"
KANVIBE_QA_RUN_ID="$RUN_ID" KANVIBE_QA_RUN_DIR="$RUN_DIR" bash scripts/qa-electron-video.sh | tee "$RUN_DIR/electron-smoke.log"

echo "[kanvibe-qa] output: $RUN_DIR"
if [[ -f "$RUN_DIR/report.md" ]]; then
  echo "[kanvibe-qa] report: $RUN_DIR/report.md"
fi
if [[ -f "$RUN_DIR/run.mp4" ]]; then
  echo "[kanvibe-qa] video: $RUN_DIR/run.mp4"
fi
