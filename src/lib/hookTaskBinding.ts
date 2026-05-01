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
