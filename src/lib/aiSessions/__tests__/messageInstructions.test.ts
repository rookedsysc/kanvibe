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
