import type { PaneChatModelSettings } from "@/lib/aiSessions/paneChatModelSettings";
import type { PaneChatPromptScreen } from "@/lib/aiSessions/paneChatClaudePrompt";
import type { SessionType } from "@/entities/KanbanTask";
import type { AggregatedAiMessage, AiSessionProvider } from "@/lib/aiSessions/types";

export type PaneChatKey = "up" | "down" | "left" | "right" | "enter" | "escape";
export const MAX_PANE_CHAT_ATTACHMENTS = 10;
export const MAX_PANE_CHAT_SUGGESTIONS = 20;
/** 입력창이 메뉴를 열 때 한 번에 받아 두고 자기 쪽에서 거르는 skill·명령 목록의 상한 */
export const MAX_PANE_CHAT_CATALOG_SUGGESTIONS = 512;

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

/** 이름이나 설명에 검색어가 들어 있는 제안을 이름순으로 limit개까지 남기고, 더 있었는지 알린다 */
export function filterPaneChatSuggestions(suggestions: PaneChatSuggestion[], query: string, limit: number): Pick<PaneChatSuggestions, "items" | "truncated"> {
  const loweredQuery = query.toLowerCase();
  const matches = suggestions.filter((suggestion) => `${suggestion.label} ${suggestion.description}`.toLowerCase().includes(loweredQuery));
  matches.sort((left, right) => left.label.localeCompare(right.label));
  return { items: matches.slice(0, limit), truncated: matches.length > limit };
}

/** CLI가 답을 기다리며 띄운 선택 화면. 채팅이 같은 내용을 카드로 보여 주고, 고른 답을 키 입력으로 전달한다 */
export interface PaneChatPrompt extends PaneChatPromptScreen {
  /** 같은 선택 화면인지 가리는 값. 답을 보낼 때 그대로 돌려준다 */
  id: string;
}

/**
 * 체크 상태가 바뀌어도 같은 선택 화면으로 묶는 값.
 * 답을 보낼 때의 `id`는 체크가 바뀌면 달라지므로, 화면에 떠 있는 카드와 그 카드에 남길 안내는 이 값으로 이어 붙인다.
 */
export function getPaneChatPromptDialogKey(prompt: PaneChatPrompt): string {
  return JSON.stringify([prompt.kind, prompt.title, prompt.detail]);
}

/** 선택 화면에 보낼 답. option은 번호 선택, text는 직접 입력 줄에 글을 넣어 고르기, step은 질문 사이 이동, cancel은 Esc다 */
export type PaneChatPromptAnswer =
  | { type: "option"; number: number }
  | { type: "text"; number: number; text: string }
  | { type: "step"; direction: "previous" | "next" }
  | { type: "cancel" };
/** 숫자 키 하나로 고를 수 있는 선택지 번호의 상한. 두 자리 번호는 첫 숫자가 다른 선택지를 골라 버린다 */
export const MAX_PANE_CHAT_PROMPT_OPTION_NUMBER = 9;
/** 직접 입력 줄에 넣을 수 있는 글자 수. 넣은 글이 그 줄에 그대로 보이는지 확인할 수 있는 길이로 둔다 */
export const MAX_PANE_CHAT_PROMPT_TEXT_LENGTH = 500;

/** 렌더러가 보낸 값이 선택 화면에 보낼 수 있는 답이면 그 답을, 아니면 null을 돌려준다 */
export function parsePaneChatPromptAnswer(value: unknown): PaneChatPromptAnswer | null {
  if (!value || typeof value !== "object") return null;
  const answer = value as Record<string, unknown>;
  if (answer.type === "cancel") return { type: "cancel" };
  if (answer.type === "step") return answer.direction === "previous" || answer.direction === "next" ? { type: "step", direction: answer.direction } : null;
  const number = answer.number;
  if (typeof number !== "number" || !Number.isInteger(number) || number < 1 || number > MAX_PANE_CHAT_PROMPT_OPTION_NUMBER) return null;
  if (answer.type === "option") return { type: "option", number };
  if (answer.type !== "text" || typeof answer.text !== "string") return null;
  const text = answer.text.trim();
  return text && text.length <= MAX_PANE_CHAT_PROMPT_TEXT_LENGTH && !/[\x00-\x1f\x7f]/.test(text) ? { type: "text", number, text } : null;
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
  /** host·작업·pane·대화 범위의 draft 키. 전송 권한인 targetToken과 수명이 다르다 */
  conversationKey?: string;
  canSend: boolean;
  inputState: "ready" | "occupied" | "unknown";
  /** 에이전트가 응답을 만들거나 도구를 실행하는 중인지. 화면으로 확인하지 못하는 CLI나 surface는 unknown이다 */
  agentActivity: "working" | "idle" | "unknown";
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
  modelPicker: { supported: boolean; mode: "native-cli"; supportsApi?: boolean };
  /** 현재 모델·effort와 고를 수 있는 선택지. 기록에서 읽지 못했거나 지원하지 않는 CLI면 없다 */
  modelSettings?: PaneChatModelSettings;
  /** CLI가 답을 기다리며 띄운 선택 화면. 화면 배치를 확인한 CLI에서 대화가 연결돼 있을 때만 있다 */
  prompt?: PaneChatPrompt;
  composer?: { attachments: boolean; skills: boolean; fileMentions: boolean; canSendWhileBusy?: boolean; maxAttachmentBytes: number; maxAttachments: number };
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

/**
 * "대화 ID 표시" 버튼의 결과.
 * revealed는 footer에 대화 ID가 보이는 상태이고, input-not-empty는 입력란이 비어 있지 않거나 작업 중이라 입력하지 않은 경우다.
 * unsupported-version은 footer 배치를 확인하지 않은 Codex 버전, thread-not-found는 명령을 실행했지만 ID가 화면에 나타나지 않은 경우다.
 */
export interface PaneChatCodexThreadReveal {
  outcome: "revealed" | "input-not-empty" | "unsupported-version" | "thread-not-found";
}
