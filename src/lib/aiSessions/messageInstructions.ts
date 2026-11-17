import type { AggregatedAiMessage } from "./types";

/**
 * 역할로 구분되는 지침과, CLI가 사용자 역할로 기록하는 독립된 자동 문맥만 숨김 대상으로 삼는다. 일반 요청에 인용된 태그는 유지한다.
 * Claude Code가 슬래시 명령을 실행할 때 남기는 명령 기록과 그 출력, 백그라운드 작업 알림도 사용자가 대화로 보낸 글이 아니라서 여기에 든다.
 */
export function isAiInstructionMessage(message: AggregatedAiMessage): boolean {
  if (message.role === "system" || message.role === "developer") return true;
  if (message.role !== "user" || message.attachments?.length) return false;
  const text = (message.fullText || message.text).trim();
  if (/^# AGENTS\.md instructions for [^\n]+\n[\s\S]*<INSTRUCTIONS>[\s\S]*<\/INSTRUCTIONS>\s*(?:<environment_context>[\s\S]*<\/environment_context>)?$/.test(text)) return true;
  if (/^(?:<(command-name|command-message|command-args)>[\s\S]*?<\/\1>\s*)+$/.test(text)) return true;
  return /^(?:<(environment_context|skill|system-reminder|llm-wiki-context|recommended_plugins|local-command-stdout|task-notification)>[\s\S]*<\/\1>\s*)$/.test(text);
}
