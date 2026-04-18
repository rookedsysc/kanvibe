import { mkdir, writeFile } from "fs/promises";
import path from "path";
import { readTextFile } from "@/lib/hostFileAccess";

export const KANVIBE_TASK_ID_RELATIVE_PATH = ".kanvibe/task-id";

export function getHookTaskIdFilePath(repoPath: string, sshHost?: string | null) {
  return (sshHost ? path.posix : path).join(repoPath, KANVIBE_TASK_ID_RELATIVE_PATH);
}

export async function writeHookTaskIdFile(repoPath: string, taskId: string) {
  const filePath = getHookTaskIdFilePath(repoPath);
  await mkdir(path.dirname(filePath), { recursive: true });
  await writeFile(filePath, `${taskId}\n`, "utf-8");
}

export async function readHookTaskIdFile(repoPath: string, sshHost?: string | null): Promise<string | null> {
  const taskId = (await readTextFile(getHookTaskIdFilePath(repoPath, sshHost), sshHost)).trim();
  return taskId.length > 0 ? taskId : null;
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
