import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { PaneChatActionResult, PaneChatKey, PaneChatSnapshot } from "@/desktop/shared/paneChat";

export const getPaneChatSnapshot = (taskId: string): Promise<PaneChatSnapshot> =>
  invokeDesktop("paneChat", "getPaneChatSnapshot", taskId);

export const sendPaneChatPrompt = (taskId: string, targetToken: string, text: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "sendPaneChatPrompt", taskId, targetToken, text);

export const interruptPaneChat = (taskId: string, targetToken: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "interruptPaneChat", taskId, targetToken);

export const openPaneChatModelPicker = (taskId: string, targetToken: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "openPaneChatModelPicker", taskId, targetToken);

export const sendPaneChatKey = (taskId: string, targetToken: string, key: PaneChatKey): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "sendPaneChatKey", taskId, targetToken, key);
