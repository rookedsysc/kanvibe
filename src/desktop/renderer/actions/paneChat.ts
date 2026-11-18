import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { PaneChatActionResult, PaneChatAttachment, PaneChatCodexThreadReveal, PaneChatAttachmentInput, PaneChatKey, PaneChatLayout, PaneChatPromptAnswer, PaneChatSnapshot, PaneChatSuggestionKind, PaneChatSuggestions } from "@/desktop/shared/paneChat";
import type { PaneChatModelSettings, PaneChatModelSettingChange } from "@/lib/aiSessions/paneChatModelSettings";

export const getPaneChatModelSettings = (taskId: string, targetToken: string): Promise<PaneChatModelSettings> =>
  invokeDesktop("paneChat", "getPaneChatModelSettings", taskId, targetToken);

export const changePaneChatModelSetting = (taskId: string, targetToken: string, change: PaneChatModelSettingChange): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "changePaneChatModelSetting", taskId, targetToken, change);

export const getPaneChatLayout = (taskId: string, terminalTabId?: string): Promise<PaneChatLayout> =>
  invokeDesktop("paneChat", "getPaneChatLayout", taskId, terminalTabId);

export const focusPaneChatTerminal = (taskId: string, paneId: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "focusPaneChatTerminal", taskId, paneId);

export const getPaneChatModes = (taskId: string): Promise<Record<string, boolean>> =>
  invokeDesktop("paneChat", "getPaneChatModes", taskId);

export const setPaneChatMode = (taskId: string, paneId: string, isEnabled: boolean): Promise<void> =>
  invokeDesktop("paneChat", "setPaneChatMode", taskId, paneId, isEnabled);

export const getPaneChatAgentProvider = (taskId: string, paneId: string): Promise<PaneChatSnapshot["provider"]> =>
  invokeDesktop("paneChat", "getPaneChatAgentProvider", taskId, paneId);

export const getPaneChatSnapshot = (taskId: string, paneId?: string): Promise<PaneChatSnapshot> =>
  invokeDesktop("paneChat", "getPaneChatSnapshot", taskId, paneId);

export const getPaneChatTranscriptPage = (taskId: string, targetToken: string, cursor: string): Promise<PaneChatSnapshot["transcript"]> =>
  invokeDesktop("paneChat", "getPaneChatTranscriptPage", taskId, targetToken, cursor);

export const sendPaneChatPrompt = (taskId: string, targetToken: string, text: string, attachmentIds?: string[], requestId?: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "sendPaneChatPrompt", taskId, targetToken, text, attachmentIds, requestId);

export const interruptPaneChat = (taskId: string, targetToken: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "interruptPaneChat", taskId, targetToken);

export const openPaneChatModelPicker = (taskId: string, targetToken: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "openPaneChatModelPicker", taskId, targetToken);

export const revealPaneChatCodexThread = (taskId: string, paneId?: string): Promise<PaneChatCodexThreadReveal> =>
  invokeDesktop("paneChat", "revealPaneChatCodexThread", taskId, paneId);

export const sendPaneChatKey = (taskId: string, targetToken: string, key: PaneChatKey): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "sendPaneChatKey", taskId, targetToken, key);

export const answerPaneChatPrompt = (taskId: string, targetToken: string, promptId: string, answer: PaneChatPromptAnswer): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "answerPaneChatPrompt", taskId, targetToken, promptId, answer);

export const stagePaneChatAttachment = (taskId: string, targetToken: string, input: PaneChatAttachmentInput): Promise<PaneChatAttachment> =>
  invokeDesktop("paneChat", "stagePaneChatAttachment", taskId, targetToken, input);

export const removePaneChatAttachment = (taskId: string, targetToken: string, id: string): Promise<PaneChatActionResult> =>
  invokeDesktop("paneChat", "removePaneChatAttachment", taskId, targetToken, id);

export const getPaneChatSuggestions = (taskId: string, targetToken: string, kind: PaneChatSuggestionKind, query: string, limit?: number): Promise<PaneChatSuggestions> =>
  invokeDesktop("paneChat", "getPaneChatSuggestions", taskId, targetToken, kind, query, ...(limit === undefined ? [] : [limit]));
