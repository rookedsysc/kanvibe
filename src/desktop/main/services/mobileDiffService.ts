import path from "node:path";
import { TaskStatus } from "@/entities/KanbanTask";
import { getTasksByStatus } from "./kanbanService";
import { getGitDiffFiles, getOriginalFileContent, getFileContent, saveFileContent, getTaskDiffStats } from "./diffService";
import { findSymbolicLinkPath } from "@/lib/hostFileAccess";
import { MobileFeatureError, readMobileObject, readMobileText, requireMobileTask } from "./mobileFeatureValidation";

const MAX_EDIT_BYTES = 1024 * 1024;

async function requireMobileDiffTask(taskId: string) {
  const task = await requireMobileTask(taskId);
  if (!task.worktreePath || !task.branchName) throw new MobileFeatureError(409, "변경을 볼 worktree가 없습니다");
  return task;
}

export async function getMobileDiff(taskId: string) {
  await requireMobileDiffTask(taskId);
  return getGitDiffFiles(taskId);
}

export async function getMobileDiffStats() {
  const board = await getTasksByStatus();
  return getTaskDiffStats(board.tasks[TaskStatus.PROGRESS].map((task) => task.id));
}

async function validateMobileDiffPath(taskId: string, submittedPath: unknown) {
  const filePath = readMobileText(submittedPath, "파일 경로");
  const components = filePath.split("/");
  if (path.posix.isAbsolute(filePath) || filePath.includes("\\") || components.some((part) => ["", ".", "..", ".git"].includes(part))) {
    throw new MobileFeatureError(400, "worktree 안의 상대 파일 경로를 선택해 주세요");
  }
  const task = await requireMobileDiffTask(taskId);
  const files = await getGitDiffFiles(taskId);
  const file = files.find((entry) => entry.path === filePath);
  if (!file) throw new MobileFeatureError(404, "변경 목록에 없는 파일입니다");
  const rootPath = task.worktreePath!;
  const paths = [rootPath, ...components.map((_, index) => path.posix.join(rootPath, ...components.slice(0, index + 1)))];
  if (await findSymbolicLinkPath(paths, task.sshHost)) throw new MobileFeatureError(400, "심볼릭 링크 파일은 열 수 없습니다");
  return { filePath, originalPath: file.originalPath ?? filePath };
}

export async function getMobileDiffFile(taskId: string, submittedPath: unknown) {
  const { filePath, originalPath } = await validateMobileDiffPath(taskId, submittedPath);
  const [original, content] = await Promise.all([getOriginalFileContent(taskId, originalPath), getFileContent(taskId, filePath)]);
  if (Buffer.byteLength(original) > MAX_EDIT_BYTES || Buffer.byteLength(content) > MAX_EDIT_BYTES) {
    throw new MobileFeatureError(409, "모바일에서 열 수 있는 파일 크기를 넘었습니다");
  }
  if (original.includes("\0") || content.includes("\0")) throw new MobileFeatureError(409, "바이너리 파일은 편집할 수 없습니다");
  return { original, content };
}

export async function saveMobileDiffFile(taskId: string, submittedPath: unknown, input: unknown) {
  const body = readMobileObject(input);
  if (typeof body.content !== "string" || body.content.includes("\0") || Buffer.byteLength(body.content) > MAX_EDIT_BYTES) {
    throw new MobileFeatureError(400, "저장할 파일 내용이 올바르지 않거나 너무 큽니다");
  }
  const { filePath } = await validateMobileDiffPath(taskId, submittedPath);
  const saved = await saveFileContent(taskId, filePath, body.content);
  if (!saved.success) throw new MobileFeatureError(409, "파일을 저장하지 못했습니다");
}
