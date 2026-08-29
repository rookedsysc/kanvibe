import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { EditorOpenResult } from "@/desktop/main/services/editorService";

export function openTaskInVsCode(taskId: string): Promise<EditorOpenResult> {
  return invokeDesktop("editor", "openTaskInVsCode", taskId);
}
