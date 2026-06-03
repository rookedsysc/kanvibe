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
KANVIBE_TARGETS_FILE="\${KANVIBE_STATE_DIR}/hooks-targets.json"
KANVIBE_TASK_STATE_FILE="\${KANVIBE_STATE_DIR}/task-state.json"

mkdir -p "\${KANVIBE_STATE_DIR}" 2>/dev/null || true
if command -v node >/dev/null 2>&1; then
  node - "\${KANVIBE_TASK_STATE_FILE}" "\${TASK_ID}" "\${KANVIBE_STATUS}" <<'NODE' >/dev/null 2>&1 || true
const fs = require("fs");
const [filePath, taskId, status] = process.argv.slice(2);
fs.mkdirSync(require("path").dirname(filePath), { recursive: true });
fs.writeFileSync(filePath, JSON.stringify({ version: 1, taskId, status, updatedAt: new Date().toISOString() }, null, 2) + "\\n");
NODE
else
  printf '{"version":1,"taskId":"%s","status":"${status}"}\\n' "\${TASK_ID}" > "\${KANVIBE_TASK_STATE_FILE}" 2>/dev/null || true
fi

KANVIBE_TARGET_ROWS=""
if [ -f "\${KANVIBE_TARGETS_FILE}" ] && command -v node >/dev/null 2>&1; then
  KANVIBE_TARGET_ROWS="$(node - "\${KANVIBE_TARGETS_FILE}" <<'NODE' 2>/dev/null || true
const fs = require("fs");
const filePath = process.argv[2];
const parsed = JSON.parse(fs.readFileSync(filePath, "utf8"));
for (const target of Array.isArray(parsed.targets) ? parsed.targets : []) {
  if (target && typeof target.url === "string" && typeof target.taskId === "string" && target.url && target.taskId) {
    process.stdout.write(target.url + "\\t" + target.taskId + "\\n");
  }
}
NODE
)"
elif [ -f "\${KANVIBE_TARGETS_FILE}" ] && command -v python3 >/dev/null 2>&1; then
  KANVIBE_TARGET_ROWS="$(python3 - "\${KANVIBE_TARGETS_FILE}" <<'PY' 2>/dev/null || true
import json, sys
with open(sys.argv[1], "r", encoding="utf-8") as fp:
    parsed = json.load(fp)
for target in parsed.get("targets", []):
    url = target.get("url")
    task_id = target.get("taskId")
    if isinstance(url, str) and isinstance(task_id, str) and url and task_id:
        print(f"{url}\\t{task_id}")
PY
)"
fi

if [ -z "\${KANVIBE_TARGET_ROWS}" ]; then
  KANVIBE_TARGET_ROWS="$(printf '%s\\t%s\\n' "\${KANVIBE_URL}" "\${TASK_ID}")"
fi

printf '%s\\n' "\${KANVIBE_TARGET_ROWS}" | while IFS="$(printf '\\t')" read -r KANVIBE_TARGET_URL KANVIBE_TARGET_TASK_ID; do
  [ -n "\${KANVIBE_TARGET_URL}" ] || continue
  [ -n "\${KANVIBE_TARGET_TASK_ID}" ] || KANVIBE_TARGET_TASK_ID="\${TASK_ID}"
  curl -s -X POST "\${KANVIBE_TARGET_URL%/}/api/hooks/status" \\
    -H "Content-Type: application/json" \\
    -d "{\\"taskId\\": \\\"\${KANVIBE_TARGET_TASK_ID}\\\", \\\"status\\\": \\\"${status}\\\"}" \\
    > /dev/null 2>&1 || true
done`;
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
