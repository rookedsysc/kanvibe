import type { SessionType } from "@/entities/KanbanTask";
import type { AggregatedAiMessage, AiSessionProvider } from "@/lib/aiSessions/types";

export type PaneChatKey = "up" | "down" | "left" | "right" | "enter" | "escape";
export const MAX_PANE_CHAT_ATTACHMENTS = 10;
export const MAX_PANE_CHAT_SUGGESTIONS = 20;

export interface PaneChatAttachmentInput {
  name: string;
  mimeType: string;
  dataUrl: string;
}

export interface PaneChatAttachment {
  id: string;
  name: string;
  mimeType: string;
  kind: "image" | "file";
  sizeBytes: number;
  previewDataUrl?: string;
}

export type PaneChatSuggestionKind = "skill" | "command" | "file";
export interface PaneChatSuggestion {
  id: string;
  label: string;
  description: string;
  insertText: string;
}

export interface PaneChatSuggestions {
  targetToken: string;
  items: PaneChatSuggestion[];
  truncated: boolean;
}

export interface PaneChatBounds {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface PaneChatLayoutPane {
  id: string;
  tabId: string;
  /** 셀 단위 좌표이며, null은 분할 없는 plain PTY 전체 영역이다. */
  bounds: PaneChatBounds | null;
}

export interface PaneChatLayout {
  activePaneId: string | null;
  panes: PaneChatLayoutPane[];
}

export interface PaneChatSnapshot {
  taskId: string;
  paneId: string | null;
  sessionType: SessionType | null;
  provider: AiSessionProvider | null;
  targetToken: string | null;
  canSend: boolean;
  inputState: "ready" | "occupied" | "unknown";
  screen: string;
  reason: string | null;
  transcript: {
    status: "available" | "unavailable";
    reason: string | null;
    sessionId: string | null;
    messages: AggregatedAiMessage[];
    nextCursor: string | null;
  };
  modelPicker: { supported: boolean; mode: "native-cli" };
  composer?: { attachments: boolean; skills: boolean; fileMentions: boolean; maxAttachmentBytes: number; maxAttachments: number };
}

export interface PaneChatActionResult {
  success: true;
}
