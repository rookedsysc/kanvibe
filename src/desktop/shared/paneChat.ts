import type { SessionType } from "@/entities/KanbanTask";
import type { AggregatedAiMessage, AiSessionProvider } from "@/lib/aiSessions/types";

export type PaneChatKey = "up" | "down" | "left" | "right" | "enter" | "escape";

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
}

export interface PaneChatActionResult {
  success: true;
}
