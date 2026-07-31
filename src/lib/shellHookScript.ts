import {
  KANVIBE_GIT_EXCLUDE_MARKER,
  KANVIBE_STATE_DIR_EXCLUDE_PATTERN,
} from "@/lib/gitExclude";

/** hook이 KanVibe에 통보할 수 있는 task 상태 */
export type ShellHookStatus = "progress" | "pending" | "review" | "done";

function escapeShellDoubleQuotedValue(value: string) {
  return value
    .replace(/\\/g, "\\\\")
    .replace(/"/g, '\\"')
    .replace(/\$/g, "\\$")
    .replace(/`/g, "\\`");
}

function unescapeShellDoubleQuotedValue(value: string) {
  return value.replace(/\\([\\"$`])/g, "$1");
}

export function buildShellTaskIdResolver(defaultTaskId: string) {
  return `TASK_ID="${escapeShellDoubleQuotedValue(defaultTaskId)}"`;
}

export function extractShellTaskId(content: string): string | null {
  const match = content.match(/^TASK_ID="((?:\\.|[^"\\])*)"$/m);
  return match ? unescapeShellDoubleQuotedValue(match[1]) : null;
}

export interface ShellTaskIdBindingStatus {
  hasTaskIdBinding: boolean;
  boundTaskId: string | null;
}

/**
 * hook이 상태를 status.json에 기록하고 targets.json의 모든 client에 병렬로 통보하는 shell 조각을 만든다.
 * 다른 client가 기록한 프로젝트 색상은 재기록 시에도 보존한다.
 */
export function buildShellKanvibeStatusUpdater(status: ShellHookStatus) {
  return `KANVIBE_STATUS="${status}"
KANVIBE_REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "\${BASH_SOURCE[0]}")/../.." && pwd))"
KANVIBE_STATE_DIR="\${KANVIBE_REPO_ROOT}/.kanvibe"
KANVIBE_TASK_STATE_FILE="\${KANVIBE_STATE_DIR}/status.json"
KANVIBE_TARGETS_FILE="\${KANVIBE_STATE_DIR}/targets.json"
${buildShellKanvibeStatusExcludeUpdater()}

mkdir -p "\${KANVIBE_STATE_DIR}" 2>/dev/null || true
KANVIBE_UPDATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || true)"
${buildShellKanvibeProjectColorReader()}

KANVIBE_TASK_STATE_JSON="{\\"schemaVersion\\":1,\\"status\\":\\"\${KANVIBE_STATUS}\\""
if [ -n "\${KANVIBE_UPDATED_AT}" ]; then
  KANVIBE_TASK_STATE_JSON="\${KANVIBE_TASK_STATE_JSON},\\"updatedAt\\":\\"\${KANVIBE_UPDATED_AT}\\""
fi
if [ -n "\${KANVIBE_PROJECT_COLOR}" ]; then
  KANVIBE_TASK_STATE_JSON="\${KANVIBE_TASK_STATE_JSON},\\"projectColor\\":\\"\${KANVIBE_PROJECT_COLOR}\\""
fi
printf '%s}\\n' "\${KANVIBE_TASK_STATE_JSON}" > "\${KANVIBE_TASK_STATE_FILE}" 2>/dev/null || true

KANVIBE_TARGET_ROWS="$(
  if [ -f "\${KANVIBE_TARGETS_FILE}" ]; then
    grep -oE '"(url|taskId)"[[:space:]]*:[[:space:]]*"[^"]*"' "\${KANVIBE_TARGETS_FILE}" 2>/dev/null \
      | awk '
          { s=$0; sub(/^"[^"]*"[[:space:]]*:[[:space:]]*"/,"",s); sub(/"$/,"",s) }
          /^"url"/ { u=s; sub(/\\/+$/,"",u); haveUrl=1; next }
          /^"taskId"/ { if(haveUrl){ if(u!="" && s!="" && !(u in seen)){ seen[u]=1; print u "\\t" s } haveUrl=0 } }
        ' 2>/dev/null || true
  else
    printf '%s\t%s\n' "\${KANVIBE_URL%/}" "\${TASK_ID}"
  fi
)"
if [ -z "\${KANVIBE_TARGET_ROWS}" ]; then
  KANVIBE_TARGET_ROWS="$(printf '%s\t%s\n' "\${KANVIBE_URL%/}" "\${TASK_ID}")"
fi

printf '%s\n' "\${KANVIBE_TARGET_ROWS}" | {
  while IFS="$(printf '\\t')" read -r KANVIBE_TARGET_URL KANVIBE_TARGET_TASK_ID; do
    if [ -z "\${KANVIBE_TARGET_URL}" ] || [ -z "\${KANVIBE_TARGET_TASK_ID}" ]; then
      continue
    fi
    curl -s -X POST "\${KANVIBE_TARGET_URL%/}/api/hooks/status" \
      -H "Content-Type: application/json" \
      -d "{\\"taskId\\": \\"\${KANVIBE_TARGET_TASK_ID}\\", \\"status\\": \\"${status}\\"}" \
      > /dev/null 2>&1 &
  done
  wait
}`;
}

/**
 * status.json에 이미 기록된 프로젝트 색상을 읽어 재기록 시 보존한다.
 * 파일 내용이 shell로 흘러들지 않도록 `#RRGGBB` 형태만 통과시킨다.
 */
function buildShellKanvibeProjectColorReader() {
  return `KANVIBE_PROJECT_COLOR="$(
  if [ -f "\${KANVIBE_TASK_STATE_FILE}" ]; then
    grep -oE '"projectColor"[[:space:]]*:[[:space:]]*"[^"]*"' "\${KANVIBE_TASK_STATE_FILE}" 2>/dev/null \
      | sed -E -n 's/.*"([^"]*)"$/\\1/p;q' 2>/dev/null || true
  fi
)"
case "\${KANVIBE_PROJECT_COLOR}" in
  "#"[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]) ;;
  *) KANVIBE_PROJECT_COLOR="" ;;
esac`;
}

function buildShellKanvibeStatusExcludeUpdater() {
  return `KANVIBE_GIT_COMMON_DIR="$(git -C "\${KANVIBE_REPO_ROOT}" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
if [ -n "\${KANVIBE_GIT_COMMON_DIR}" ]; then
  KANVIBE_GIT_EXCLUDE_FILE="\${KANVIBE_GIT_COMMON_DIR}/info/exclude"
  mkdir -p "$(dirname "\${KANVIBE_GIT_EXCLUDE_FILE}")" 2>/dev/null || true
  touch "\${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null || true
  if ! grep -qxF "${KANVIBE_STATE_DIR_EXCLUDE_PATTERN}" "\${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null; then
    if ! grep -qxF "${KANVIBE_GIT_EXCLUDE_MARKER}" "\${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null; then
      printf '\n%s\n' "${KANVIBE_GIT_EXCLUDE_MARKER}" >> "\${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null || true
    fi
    printf '%s\n' "${KANVIBE_STATE_DIR_EXCLUDE_PATTERN}" >> "\${KANVIBE_GIT_EXCLUDE_FILE}" 2>/dev/null || true
  fi
fi`;
}

export function hasShellKanvibeStatusJsonPersistence(content: string): boolean {
  return content.includes("status.json")
    && content.includes("KANVIBE_TASK_STATE_FILE")
    && content.includes("KANVIBE_GIT_COMMON_DIR")
    && content.includes("--git-common-dir")
    && content.includes("KANVIBE_GIT_EXCLUDE_FILE")
    && content.includes(KANVIBE_STATE_DIR_EXCLUDE_PATTERN)
    && content.includes("KANVIBE_TASK_STATE_JSON")
    && content.includes('\\"schemaVersion\\":1')
    && content.includes('\\"status\\":\\"${KANVIBE_STATUS}\\"');
}

export function hasShellKanvibeProjectColorPersistence(content: string): boolean {
  return content.includes("KANVIBE_PROJECT_COLOR")
    && content.includes('"projectColor"')
    && content.includes('\\"projectColor\\":\\"${KANVIBE_PROJECT_COLOR}\\"');
}

export function hasShellKanvibeTargetFanout(content: string): boolean {
  return content.includes("targets.json")
    && content.includes("KANVIBE_TARGETS_FILE")
    && content.includes("KANVIBE_TARGET_ROWS")
    && content.includes("KANVIBE_TARGET_URL")
    && content.includes("KANVIBE_TARGET_TASK_ID")
    && content.includes("while IFS=")
    && content.includes("/api/hooks/status")
    && content.includes("taskId");
}

/** curl fan-out이 순차 실행이 아니라 병렬 실행 후 wait로 수렴하는지 확인한다 */
export function hasShellKanvibeParallelTargetFanout(content: string): boolean {
  return hasShellKanvibeTargetFanout(content)
    && content.includes("> /dev/null 2>&1 &")
    && /\n\s*wait\n/.test(content);
}

/** 모든 hook script가 같은 fallback task id를 고정하고 payload에서 사용하는지 확인한다 */
export function getShellTaskIdBindingStatus(contents: string[]): ShellTaskIdBindingStatus {
  const hasTaskIdPayloadBindings = contents.every((content) => (
    content.includes("taskId") && content.includes("${TASK_ID}")
  ));
  const boundTaskIds = contents.map(extractShellTaskId);
  const firstTaskId = boundTaskIds[0] ?? null;
  const boundTaskId = firstTaskId && boundTaskIds.every((value) => value === firstTaskId)
    ? firstTaskId
    : null;

  return {
    hasTaskIdBinding: hasTaskIdPayloadBindings && boundTaskId !== null,
    boundTaskId,
  };
}
