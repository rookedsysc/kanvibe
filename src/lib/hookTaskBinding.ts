import {
  KANVIBE_GIT_EXCLUDE_MARKER,
  KANVIBE_STATE_DIR_EXCLUDE_PATTERN,
} from "@/lib/gitExclude";

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
  hasExpectedTaskId: boolean;
  boundTaskId: string | null;
}

export function buildShellKanvibeStatusUpdater(status: "progress" | "pending" | "review" | "done") {
  return `KANVIBE_STATUS="${status}"
KANVIBE_REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "\${BASH_SOURCE[0]}")/../.." && pwd))"
KANVIBE_STATE_DIR="\${KANVIBE_REPO_ROOT}/.kanvibe"
KANVIBE_TASK_STATE_FILE="\${KANVIBE_STATE_DIR}/status.json"
${buildShellKanvibeStatusExcludeUpdater()}

mkdir -p "\${KANVIBE_STATE_DIR}" 2>/dev/null || true
KANVIBE_UPDATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || true)"
if [ -n "\${KANVIBE_UPDATED_AT}" ]; then
  printf '{"schemaVersion":1,"status":"%s","updatedAt":"%s"}\\n' "\${KANVIBE_STATUS}" "\${KANVIBE_UPDATED_AT}" > "\${KANVIBE_TASK_STATE_FILE}" 2>/dev/null || true
else
  printf '{"schemaVersion":1,"status":"%s"}\\n' "\${KANVIBE_STATUS}" > "\${KANVIBE_TASK_STATE_FILE}" 2>/dev/null || true
fi

curl -s -X POST "\${KANVIBE_URL%/}/api/hooks/status" \
  -H "Content-Type: application/json" \
  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"${status}\\\"}" \
  > /dev/null 2>&1 || true`;
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
    && content.includes('"schemaVersion":1')
    && content.includes('"status":"%s"');
}

export function getShellTaskIdBindingStatus(
  contents: string[],
  expectedTaskId?: string,
): ShellTaskIdBindingStatus {
  const hasTaskIdPayloadBindings = contents.every((content) => (
    content.includes("taskId") && content.includes("${TASK_ID}")
  ));
  const boundTaskIds = contents.map(extractShellTaskId);
  const firstTaskId = boundTaskIds[0] ?? null;
  const boundTaskId = firstTaskId && boundTaskIds.every((value) => value === firstTaskId)
    ? firstTaskId
    : null;
  const hasTaskIdBinding = hasTaskIdPayloadBindings && boundTaskId !== null;
  const hasExpectedTaskId = hasTaskIdBinding && (!expectedTaskId || boundTaskId === expectedTaskId);

  return {
    hasTaskIdBinding,
    hasExpectedTaskId,
    boundTaskId,
  };
}
