#!/bin/bash

# KanVibe Gemini CLI Hook: BeforeAgent
# 사용자가 prompt를 입력하면 현재 task를 PROGRESS로 변경한다.
# Gemini CLI hooks는 stdout에 JSON만 출력해야 한다.

KANVIBE_URL="http://KANVIBE_HOOK_HOST:19736"
TASK_ID="KANVIBE_TASK_ID"
KANVIBE_STATUS="progress"
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
    grep -oE '"(url|taskId|clientId)"[[:space:]]*:[[:space:]]*"[^"]*"' "${KANVIBE_TARGETS_FILE}" 2>/dev/null       | awk '
          function emit() { if(u!="" && t!="" && !(u in seen)){ seen[u]=1; print u "\t" t "\t" c } }
          { s=$0; sub(/^"[^"]*"[[:space:]]*:[[:space:]]*"/,"",s); sub(/"$/,"",s) }
          /^"url"/ { emit(); u=s; sub(/\/+$/,"",u); t=""; c=""; next }
          /^"taskId"/ { t=s; next }
          /^"clientId"/ { c=s; next }
          END { emit() }
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

KANVIBE_HOOK_SOCKET_RESOLVER

kanvibe_post_status() {
  curl -s --max-time 3 -X POST "$@" --url "${KANVIBE_TARGET_URL%/}/api/hooks/status"     -H "Content-Type: application/json"     -d "{\"taskId\": \"${KANVIBE_TARGET_TASK_ID}\", \"status\": \"progress\"}"
}

printf '%s
' "${KANVIBE_TARGET_ROWS}" | {
  while IFS="$(printf '\t')" read -r KANVIBE_TARGET_URL KANVIBE_TARGET_TASK_ID KANVIBE_TARGET_CLIENT_ID; do
    if [ -z "${KANVIBE_TARGET_URL}" ] || [ -z "${KANVIBE_TARGET_TASK_ID}" ]; then
      continue
    fi
    case "${KANVIBE_TARGET_URL}" in
      http://*|https://*) ;;
      *) continue ;;
    esac
    KANVIBE_TARGET_SOCKET="$(kanvibe_hook_socket "${KANVIBE_TARGET_CLIENT_ID}")"
    {
      if [ -z "${KANVIBE_TARGET_SOCKET}" ] || ! kanvibe_post_status --unix-socket "${KANVIBE_TARGET_SOCKET}"; then
        kanvibe_post_status
      fi
    } > /dev/null 2>&1 &
  done
  wait
}

echo '{}'
exit 0
