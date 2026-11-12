import type { AggregatedAiMessage } from "@/lib/aiSessions/types";

const MESSAGE_HASH_OFFSET = 2166136261;
const MESSAGE_HASH_PRIME = 16777619;

/** 최신순 기록에서 키를 계산해 새 응답과 이전 페이지가 추가되어도 펼친 행을 유지한다. */
export function getChatTranscriptRows(messages: AggregatedAiMessage[], sessionKey: string | null) {
  const occurrences = new Map<string, number>();
  return messages.map((message) => {
    const text = message.fullText || message.text;
    let hash = MESSAGE_HASH_OFFSET;
    for (let index = 0; index < text.length; index += 1) hash = Math.imul(hash ^ text.charCodeAt(index), MESSAGE_HASH_PRIME);
    const identity = `${message.role}:${message.timestamp ?? ""}:${hash >>> 0}`;
    const occurrence = occurrences.get(identity) ?? 0;
    occurrences.set(identity, occurrence + 1);
    return { message, key: `${sessionKey}:${identity}:${occurrence}` };
  }).reverse();
}
