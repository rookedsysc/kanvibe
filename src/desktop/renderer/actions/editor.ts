import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { EditorOpenResult } from "@/desktop/shared/contracts/desktop-main-services-editorService";

export function openTaskInVsCode(taskId: string): Promise<EditorOpenResult> {
  return invokeDesktop("editor", "openTaskInVsCode", taskId);
}
