import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { DiffFile } from "@/desktop/main/services/diffService";

export type { DiffFile };

export function getGitDiffFiles(taskId: string): Promise<DiffFile[]> {
  return invokeDesktop("diff", "getGitDiffFiles", taskId);
}

export function getOriginalFileContent(taskId: string, filePath: string): Promise<string> {
  return invokeDesktop("diff", "getOriginalFileContent", taskId, filePath);
}

export function getFileContent(taskId: string, filePath: string): Promise<string> {
  return invokeDesktop("diff", "getFileContent", taskId, filePath);
}

export function saveFileContent(taskId: string, filePath: string, content: string): Promise<{ success: boolean; error?: string }> {
  return invokeDesktop("diff", "saveFileContent", taskId, filePath, content);
}
