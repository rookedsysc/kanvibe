import { mkdir, readFile, writeFile } from "fs/promises";
import path from "path";

export const KANVIBE_TASK_ID_RELATIVE_PATH = ".kanvibe/task-id";

export function getHookTaskIdFilePath(repoPath: string) {
  return path.join(repoPath, KANVIBE_TASK_ID_RELATIVE_PATH);
}

export async function writeHookTaskIdFile(repoPath: string, taskId: string) {
  const filePath = getHookTaskIdFilePath(repoPath);
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${taskId}\n`, "utf-8");
}

export async function readHookTaskIdFile(repoPath: string): Promise<string | null> {
  try {
    const taskId = (await readFile(getHookTaskIdFilePath(repoPath), "utf-8")).trim();
    return taskId.length > 0 ? taskId : null;
  } catch {
    return null;
  }
}

export function buildShellTaskIdResolver(defaultTaskId: string) {
  return [
    `TASK_ID_FILE="${KANVIBE_TASK_ID_RELATIVE_PATH}"`,
    `TASK_ID="${defaultTaskId}"`,
    "",
    'if [ -f "$TASK_ID_FILE" ]; then',
    "  FILE_TASK_ID=$(tr -d '\\r\\n' < \"$TASK_ID_FILE\")",
    '  if [ -n "$FILE_TASK_ID" ]; then',
    '    TASK_ID="$FILE_TASK_ID"',
    "  fi",
    "fi",
  ].join("\n");
}
