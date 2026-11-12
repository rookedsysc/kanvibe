import type { SessionType } from "@/entities/KanbanTask";
import type { AggregatedAiMessage, AiSessionProvider } from "@/lib/aiSessions/types";

export type PaneChatKey = "up" | "down" | "left" | "right" | "enter" | "escape";

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
