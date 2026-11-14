import type { PaneChatModelSettings } from "@/lib/aiSessions/paneChatModelSettings";
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
    /** 대화에 매긴 순번이 유효한 범위. 순번을 매기지 않는 조회에는 없다 */
    epoch?: string;
    /** 가장 최근 메시지의 순번. messages는 최신순이라 index 0이 이 순번이다 */
    seq?: number;
  };
  modelPicker: { supported: boolean; mode: "native-cli" };
  /** 현재 모델·effort와 고를 수 있는 선택지. 기록에서 읽지 못했거나 지원하지 않는 CLI면 없다 */
  modelSettings?: PaneChatModelSettings;
  composer?: { attachments: boolean; skills: boolean; fileMentions: boolean; maxAttachmentBytes: number; maxAttachments: number };
}

/** 구독자가 대화를 어디까지 받았는지 */
export interface PaneChatCursor {
  epoch: string;
  seq: number;
}

/** 모바일이 push로 받으려는 pane 하나 */
export interface PaneChatSubscriptionTarget {
  taskId: string;
  paneId: string;
  /** chat은 대화와 입력 상태까지, agent는 에이전트가 돌고 있는지만 받는다 */
  detail: "chat" | "agent";
  /** CLI 화면이 실제로 보이는 동안에만 켠다 */
  wantsScreen: boolean;
}

export interface PaneChatSubscriptionFreshness {
  taskId: string;
  paneId: string;
  /** behind면 구독자가 스냅샷을 다시 받아야 한다 */
  freshness: "current" | "behind";
}

/** 대화 밖의 상태. 순번이 없고 마지막에 받은 값이 이긴다 */
export type PaneChatPushedState = Partial<Omit<PaneChatSnapshot, "transcript">> & {
  transcript?: Pick<PaneChatSnapshot["transcript"], "status" | "reason" | "sessionId" | "nextCursor">;
};

export type PaneChatPushEvent =
  | { type: "pane-chat-rows"; taskId: string; paneId: string; epoch: string; rows: { seq: number; message: AggregatedAiMessage }[] }
  /** 기록이 다시 쓰였거나 다른 세션으로 바뀌었다. 줄은 싣지 않고 구독자가 스냅샷을 다시 받는다 */
  | { type: "pane-chat-replaced"; taskId: string; paneId: string; epoch: string }
  | { type: "pane-chat-state"; taskId: string; paneId: string; state: PaneChatPushedState };

export interface PaneChatActionResult {
  success: true;
}
