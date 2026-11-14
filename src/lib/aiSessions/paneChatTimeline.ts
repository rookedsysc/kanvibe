import { randomUUID } from "node:crypto";
import type { AggregatedAiMessage } from "@/lib/aiSessions/types";

/** 순번이 붙은 대화 한 줄. 같은 epoch 안에서 seq는 1씩 늘고 다시 쓰이지 않는다 */
export interface PaneChatTimelineRow {
  seq: number;
  message: AggregatedAiMessage;
}

/**
 * pane 하나의 대화 기록에 매긴 순번의 현재 상태.
 *
 * CLI 기록에는 메시지 ID가 없어서, 구독자가 "어디까지 받았는지"를 말할 수 있도록 데스크탑이 순번을 매긴다.
 * epoch은 이 순번이 유효한 범위다. 기록이 다시 쓰이거나 다른 세션으로 바뀌면 새 epoch으로 처음부터 센다.
 */
export interface PaneChatTimeline {
  epoch: string;
  sessionId: string | null;
  /** 오래된 것부터. 조회 창에 들어 있는 줄만 가진다 */
  rows: PaneChatTimelineRow[];
}

export interface PaneChatTimelineAdvance {
  timeline: PaneChatTimeline;
  /** 새로 붙었거나 내용이 자란 줄. 오래된 것부터 */
  changedRows: PaneChatTimelineRow[];
  /** 이전 순번을 이어 갈 수 없어 새 epoch을 시작했는지 */
  isReplaced: boolean;
}

function isSameMessage(left: AggregatedAiMessage, right: AggregatedAiMessage): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

/** CLI가 마지막 답변을 이어 쓴 결과인지. 같은 사람이 쓴 글이 앞부분을 그대로 둔 채 길어진 경우만 인정한다 */
function isGrownMessage(previous: AggregatedAiMessage, current: AggregatedAiMessage): boolean {
  return previous.role === current.role && current.fullText.startsWith(previous.fullText);
}

/**
 * 이전 줄들이 새 조회 창의 어디에서부터 이어지는지 찾는다.
 *
 * 조회 창은 최근 줄만 담으므로 오래된 줄이 창 밖으로 밀려나는 것은 어긋남이 아니다. 그래서 이전 줄을 앞에서부터
 * 하나씩 버려 가며, 남은 줄이 새 창의 앞부분과 같아지는 지점을 찾는다. CLI는 마지막 답변을 이어 쓰므로 맨 끝 줄
 * 하나는 글이 길어진 경우도 같은 줄로 본다.
 */
function findContinuationStart(previousRows: PaneChatTimelineRow[], messages: AggregatedAiMessage[]): number | null {
  for (let droppedCount = 0; droppedCount < previousRows.length; droppedCount++) {
    const keptRows = previousRows.slice(droppedCount);
    if (keptRows.length > messages.length) continue;
    const lastKeptIndex = keptRows.length - 1;
    const isContinuation = keptRows.every((row, index) => index === lastKeptIndex
      ? isSameMessage(row.message, messages[index]) || isGrownMessage(row.message, messages[index])
      : isSameMessage(row.message, messages[index]));
    if (isContinuation) return droppedCount;
  }
  return null;
}

function startTimeline(sessionId: string | null, messages: AggregatedAiMessage[], isReplaced: boolean): PaneChatTimelineAdvance {
  const rows = messages.map((message, index) => ({ seq: index + 1, message }));
  return { timeline: { epoch: randomUUID(), sessionId, rows }, changedRows: rows, isReplaced };
}

/**
 * 새로 읽은 대화 기록을 이전 상태와 맞춰 순번을 이어 매긴다.
 *
 * @param messages 오래된 것부터 늘어놓은 최근 대화
 */
export function advancePaneChatTimeline(previous: PaneChatTimeline | null, sessionId: string | null, messages: AggregatedAiMessage[]): PaneChatTimelineAdvance {
  if (!previous) return startTimeline(sessionId, messages, false);
  if (previous.sessionId !== sessionId) return startTimeline(sessionId, messages, true);
  if (previous.rows.length === 0) {
    const rows = messages.map((message, index) => ({ seq: index + 1, message }));
    return { timeline: { ...previous, rows }, changedRows: rows, isReplaced: false };
  }

  const continuationStart = findContinuationStart(previous.rows, messages);
  if (continuationStart === null) return startTimeline(sessionId, messages, true);

  const keptRows = previous.rows.slice(continuationStart);
  const lastKeptIndex = keptRows.length - 1;
  const lastKeptRow = keptRows[lastKeptIndex];
  const changedRows: PaneChatTimelineRow[] = [];
  const rows = keptRows.slice(0, lastKeptIndex);

  if (isSameMessage(lastKeptRow.message, messages[lastKeptIndex])) {
    rows.push(lastKeptRow);
  } else {
    const grownRow = { seq: lastKeptRow.seq, message: messages[lastKeptIndex] };
    rows.push(grownRow);
    changedRows.push(grownRow);
  }

  for (let index = keptRows.length; index < messages.length; index++) {
    const appendedRow = { seq: lastKeptRow.seq + (index - lastKeptIndex), message: messages[index] };
    rows.push(appendedRow);
    changedRows.push(appendedRow);
  }

  return { timeline: { ...previous, rows }, changedRows, isReplaced: false };
}

/** 구독자가 가진 위치가 이 기록의 끝과 같은지. 다르면 구독자는 전체를 다시 받아야 한다 */
export function isPaneChatCursorCurrent(timeline: PaneChatTimeline | null, cursor: { epoch: string; seq: number } | null): boolean {
  if (!timeline || !cursor) return false;
  return timeline.epoch === cursor.epoch && readPaneChatTimelineSeq(timeline) === cursor.seq;
}

/** 마지막 줄의 순번. 줄이 없으면 0이다 */
export function readPaneChatTimelineSeq(timeline: PaneChatTimeline): number {
  return timeline.rows.at(-1)?.seq ?? 0;
}
