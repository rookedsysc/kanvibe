import type { AggregatedAiMessage } from "./types";

/** 역할로 구분되는 지침과 Codex가 사용자 역할로 기록하는 독립된 자동 문맥만 숨김 대상으로 삼는다. 일반 요청에 인용된 태그는 유지한다. */
export function isAiInstructionMessage(message: AggregatedAiMessage): boolean {
  if (message.role === "system" || message.role === "developer") return true;
  if (message.role !== "user" || message.attachments?.length) return false;
  const text = (message.fullText || message.text).trim();
  if (/^# AGENTS\.md instructions for [^\n]+\n[\s\S]*<INSTRUCTIONS>[\s\S]*<\/INSTRUCTIONS>\s*(?:<environment_context>[\s\S]*<\/environment_context>)?$/.test(text)) return true;
  return /^(?:<(environment_context|skill|system-reminder|llm-wiki-context|recommended_plugins)>[\s\S]*<\/\1>\s*)$/.test(text);
}
