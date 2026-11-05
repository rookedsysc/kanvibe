#!/bin/bash

# KanVibe Codex Hook: Stop
# Codex 응답이 완료되면 현재 task를 REVIEW로 변경한다.

KANVIBE_URL="http://KANVIBE_HOOK_HOST:19736"
TASK_ID="KANVIBE_TASK_ID"
KANVIBE_STATUS="review"
KANVIBE_REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd))"
KANVIBE_STATE_DIR="${KANVIBE_REPO_ROOT}/.kanvibe"
KANVIBE_TASK_STATE_FILE="${KANVIBE_STATE_DIR}/status.json"
KANVIBE_TARGETS_FILE="${KANVIBE_STATE_DIR}/targets.json"
KANVIBE_GIT_COMMON_DIR="$(git -C "${KANVIBE_REPO_ROOT}" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
if [ -n "${KANVIBE_GIT_COMMON_DIR}" ]; then
  KANVIBE_GIT_EXCLUDE_FILE="${KANVIBE_GIT_COMMON_DIR}/info/exclude"
  mkdir -p "$(dirname "${KANVIBE_GIT_EXCLUDE_FILE}")" 2>/dev/null || true
  touch "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null || true
  if ! grep -qxF ".kanvibe/" "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null; then
    if ! grep -qxF "# KanVibe AI hooks (auto-generated)" "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null; then
      printf '
%s
' "# KanVibe AI hooks (auto-generated)" >> "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null || true
    fi
    printf '%s
' ".kanvibe/" >> "${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null || true
  fi
fi

mkdir -p "${KANVIBE_STATE_DIR}" 2>/dev/null || true
KANVIBE_UPDATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || true)"

KANVIBE_TASK_STATE_JSON="{\"schemaVersion\":1,\"status\":\"${KANVIBE_STATUS}\""
if [ -n "${KANVIBE_UPDATED_AT}" ]; then
  KANVIBE_TASK_STATE_JSON="${KANVIBE_TASK_STATE_JSON},\"updatedAt\":\"${KANVIBE_UPDATED_AT}\""
fi
printf '%s}\n' "${KANVIBE_TASK_STATE_JSON}" > "${KANVIBE_TASK_STATE_FILE}" 2>/dev/null || true

KANVIBE_TARGET_ROWS="$(
  if [ -f "${KANVIBE_TARGETS_FILE}" ]; then
    grep -oE '"(url|taskId)"[[:space:]]*:[[:space:]]*"[^"]*"' "${KANVIBE_TARGETS_FILE}" 2>/dev/null       | awk '
          { s=$0; sub(/^"[^"]*"[[:space:]]*:[[:space:]]*"/,"",s); sub(/"$/,"",s) }
          /^"url"/ { u=s; sub(/\/+$/,"",u); haveUrl=1; next }
          /^"taskId"/ { if(haveUrl){ if(u!="" && s!="" && !(u in seen)){ seen[u]=1; print u "\t" s } haveUrl=0 } }
        ' 2>/dev/null || true
  else
    printf '%s	%s
' "${KANVIBE_URL%/}" "${TASK_ID}"
  fi
)"
if [ -z "${KANVIBE_TARGET_ROWS}" ]; then
  KANVIBE_TARGET_ROWS="$(printf '%s	%s
' "${KANVIBE_URL%/}" "${TASK_ID}")"
fi

printf '%s
' "${KANVIBE_TARGET_ROWS}" | {
  while IFS="$(printf '\t')" read -r KANVIBE_TARGET_URL KANVIBE_TARGET_TASK_ID; do
    if [ -z "${KANVIBE_TARGET_URL}" ] || [ -z "${KANVIBE_TARGET_TASK_ID}" ]; then
      continue
    fi
    curl -s --max-time 3 -X POST "${KANVIBE_TARGET_URL%/}/api/hooks/status"       -H "Content-Type: application/json"       -d "{\"taskId\": \"${KANVIBE_TARGET_TASK_ID}\", \"status\": \"review\"}"       > /dev/null 2>&1 &
  done
  wait
}

exit 0
