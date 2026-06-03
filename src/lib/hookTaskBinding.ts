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
KANVIBE_TASK_STATE_FILE="\${KANVIBE_STATE_DIR}/status.md"

mkdir -p "\${KANVIBE_STATE_DIR}" 2>/dev/null || true
printf '%s\\n' "\${KANVIBE_STATUS}" > "\${KANVIBE_TASK_STATE_FILE}" 2>/dev/null || true

curl -s -X POST "\${KANVIBE_URL%/}/api/hooks/status" \
  -H "Content-Type: application/json" \
  -d "{\\"taskId\\": \\\"\${TASK_ID}\\\", \\\"status\\\": \\\"${status}\\\"}" \
  > /dev/null 2>&1 || true`;
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
