#!/bin/bash

# KanVibe Codex CLI Hook: notify (agent-turn-complete)
# Codex 응답이 완료되면 현재 브랜치의 작업을 REVIEW로 변경한다.
# Codex notify 스크립트는 첫 번째 인자로 JSON payload를 받는다.

KANVIBE_URL="http://localhost:7777"
PROJECT_NAME="kanvibe_dev"

JSON_PAYLOAD="$1"

# agent-turn-complete 이벤트만 처리
EVENT_TYPE=$(echo "$JSON_PAYLOAD" | grep -o '"type":"[^"]*"' | head -1 | cut -d'"' -f4)
if [ "$EVENT_TYPE" != "agent-turn-complete" ]; then
  exit 0
fi

BRANCH_NAME=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
if [ -z "$BRANCH_NAME" ] || [ "$BRANCH_NAME" = "HEAD" ]; then
  exit 0
fi

curl -s -X POST "${KANVIBE_URL}/api/hooks/status" \
  -H "Content-Type: application/json" \
  -d "{\"branchName\": \"${BRANCH_NAME}\", \"projectName\": \"${PROJECT_NAME}\", \"status\": \"review\"}" \
  > /dev/null 2>&1

exit 0
