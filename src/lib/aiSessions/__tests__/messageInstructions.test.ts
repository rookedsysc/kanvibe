import { expect, it } from "vitest";
import { isAiInstructionMessage } from "../messageInstructions";
import type { AggregatedAiMessage } from "../types";

const message = (role: AggregatedAiMessage["role"], text: string): AggregatedAiMessage => ({ role, text, fullText: text, timestamp: null, isTruncated: false });

it("시스템·개발자 역할과 Codex의 독립된 AGENTS 문맥을 지침으로 구분한다", () => {
  expect(isAiInstructionMessage(message("system", "Rules"))).toBe(true);
  expect(isAiInstructionMessage(message("developer", "Rules"))).toBe(true);
  expect(isAiInstructionMessage(message("user", "# AGENTS.md instructions for /repo\n<INSTRUCTIONS>Rules</INSTRUCTIONS>\n<environment_context>cwd</environment_context>"))).toBe(true);
  expect(isAiInstructionMessage(message("user", "<environment_context>cwd</environment_context>"))).toBe(true);
});

it("태그나 AGENTS.md를 설명해 달라는 실제 사용자 요청과 AI 답변은 숨기지 않는다", () => {
  expect(isAiInstructionMessage(message("user", "AGENTS.md 내용을 설명해줘"))).toBe(false);
  expect(isAiInstructionMessage(message("user", "다음 <environment_context>cwd</environment_context>를 설명해줘"))).toBe(false);
  expect(isAiInstructionMessage(message("assistant", "<environment_context>example</environment_context>"))).toBe(false);
});

it("Claude가 사용자 역할로 남기는 슬래시 명령 기록과 그 출력, 백그라운드 작업 알림을 지침으로 구분한다", () => {
  expect(isAiInstructionMessage(message("user", "<command-name>/model</command-name>\n            <command-message>model</command-message>\n            <command-args>sonnet</command-args>"))).toBe(true);
  expect(isAiInstructionMessage(message("user", "<command-message>example-skill</command-message>\n<command-name>/example-skill</command-name>\n<command-args>요청 내용</command-args>"))).toBe(true);
  expect(isAiInstructionMessage(message("user", "<local-command-stdout>Set model to `Sonnet` and saved as your default for new sessions</local-command-stdout>"))).toBe(true);
  expect(isAiInstructionMessage(message("user", "<task-notification>\n<task-id>task-1</task-id>\n<status>completed</status>\n</task-notification>"))).toBe(true);
});

it("명령 기록의 태그를 인용한 실제 사용자 요청은 숨기지 않는다", () => {
  expect(isAiInstructionMessage(message("user", "<command-name>/model</command-name> 이 기록이 뭔지 설명해줘"))).toBe(false);
  expect(isAiInstructionMessage(message("user", "다음 <local-command-stdout>출력</local-command-stdout>을 설명해줘"))).toBe(false);
});
