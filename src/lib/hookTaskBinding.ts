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
