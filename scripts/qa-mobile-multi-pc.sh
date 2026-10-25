#!/usr/bin/env bash
set -euo pipefail

PLATFORM="${1:-ios}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN_ID="${MAESTRO_RUN_ID:-$(date +%Y%m%d-%H%M%S)-${PLATFORM}-multi-pc}"

case "${PLATFORM}" in
  android) APP_ID="com.kanvibe.kanvibe_mobile" ;;
  ios) APP_ID="com.kanvibe.kanvibeMobile" ;;
  *) echo "지원하지 않는 플랫폼: ${PLATFORM}" >&2; exit 2 ;;
esac

cd "${ROOT_DIR}"
node qa/mobile/lib/multiPcFixture.cjs &
FIXTURE_PID=$!
trap 'kill "${FIXTURE_PID}" 2>/dev/null || true; wait "${FIXTURE_PID}" 2>/dev/null || true' EXIT

for port in 19836 19837; do
  for _ in $(seq 30); do
    if curl --silent --fail "http://127.0.0.1:${port}/health" >/dev/null; then break; fi
    sleep 1
  done
  curl --silent --fail "http://127.0.0.1:${port}/health" >/dev/null
done

APP_ID="${APP_ID}" \
MAESTRO_PLATFORM="${PLATFORM}" \
MAESTRO_RUN_ID="${RUN_ID}" \
MAESTRO_OUTPUT_DIR="${ROOT_DIR}/qa-output/mobile/${RUN_ID}/maestro" \
FLOW_PATH="mobile/.maestro/flows/multi-pc.yaml" \
  bash scripts/maestro/run-maestro-e2e.sh multi-pc
