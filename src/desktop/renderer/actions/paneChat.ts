import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { PaneChatActionResult, PaneChatAttachment, PaneChatAttachmentInput, PaneChatKey, PaneChatLayout, PaneChatSnapshot, PaneChatSuggestionKind, PaneChatSuggestions } from "@/desktop/shared/paneChat";

export const getPaneChatLayout = (taskId: string, terminalTabId?: string): Promise<PaneChatLayout> =>
  invokeDesktop("paneChat", "getPaneChatLayout", taskId, terminalTabId);

export const focusPaneChatTerminal = (taskId: string, paneId: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "focusPaneChatTerminal", taskId, paneId);

export const getPaneChatSnapshot = (taskId: string, paneId?: string): Promise<PaneChatSnapshot> =>
  invokeDesktop("paneChat", "getPaneChatSnapshot", taskId, paneId);

export const sendPaneChatPrompt = (taskId: string, targetToken: string, text: string, attachmentIds?: string[], requestId?: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "sendPaneChatPrompt", taskId, targetToken, text, attachmentIds, requestId);

export const interruptPaneChat = (taskId: string, targetToken: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "interruptPaneChat", taskId, targetToken);

export const openPaneChatModelPicker = (taskId: string, targetToken: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "openPaneChatModelPicker", taskId, targetToken);

export const sendPaneChatKey = (taskId: string, targetToken: string, key: PaneChatKey): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "sendPaneChatKey", taskId, targetToken, key);

export const stagePaneChatAttachment = (taskId: string, targetToken: string, input: PaneChatAttachmentInput): Promise<PaneChatAttachment> =>
  invokeDesktop("paneChat", "stagePaneChatAttachment", taskId, targetToken, input);

export const removePaneChatAttachment = (taskId: string, targetToken: string, id: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "removePaneChatAttachment", taskId, targetToken, id);

export const getPaneChatSuggestions = (taskId: string, targetToken: string, kind: PaneChatSuggestionKind, query: string): Promise<PaneChatSuggestions> =>
  invokeDesktop("paneChat", "getPaneChatSuggestions", taskId, targetToken, kind, query);
