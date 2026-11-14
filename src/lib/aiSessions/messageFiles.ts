import { extractPlainText, truncateText } from "./shared";
import type { AggregatedAiMessage, AiMessageFile } from "./types";

export interface ToolCallSummary {
  toolName: string;
  /** 도구 입력을 한눈에 알아볼 수 있게 줄인 글 */
  summary: string;
  files: AiMessageFile[];
}

const CLAUDE_FILE_PATH_TOOLS = ["Read", "Write", "Edit", "MultiEdit", "NotebookEdit"];
const CLAUDE_FILE_CHANGE_BY_TOOL: Record<string, AiMessageFile["change"]> = {
  Write: "created",
  Edit: "modified",
  MultiEdit: "modified",
  NotebookEdit: "modified",
};
const CODEX_SUMMARY_LINE_COUNT = 3;

function readClaudeToolFilePath(toolInput: Record<string, unknown>): string | null {
  const filePath = toolInput.file_path ?? toolInput.notebook_path;
  return typeof filePath === "string" && filePath ? filePath : null;
}

/** Claude 기록의 `tool_use` 조각 하나를 도구 이름, 입력 요약, 만들거나 고친 파일로 옮긴다. 도구 호출이 아니면 null이다 */
export function summarizeClaudeToolUse(part: unknown): ToolCallSummary | null {
  if (!part || typeof part !== "object") return null;
  const { type, name, input } = part as Record<string, unknown>;
  if (type !== "tool_use" || typeof name !== "string" || !name) return null;

  const toolInput = input && typeof input === "object" ? input as Record<string, unknown> : {};
  const filePath = CLAUDE_FILE_PATH_TOOLS.includes(name) ? readClaudeToolFilePath(toolInput) : null;
  const command = name === "Bash" && typeof toolInput.command === "string" ? toolInput.command : null;
  const change = CLAUDE_FILE_CHANGE_BY_TOOL[name];

  return {
    toolName: name,
    summary: command || filePath || truncateText(JSON.stringify(toolInput)),
    files: filePath && change ? [{ path: filePath, change }] : [],
  };
}

/**
 * `apply_patch` 본문의 파일 머리줄에서 만든 파일과 고친 파일을 읽는다. 지운 파일은 미리 볼 내용이 없어 넣지 않는다.
 * 머리줄은 줄 맨 앞에서 시작하므로, 공백·`+`·`-`로 시작하는 본문 줄에 같은 글이 있어도 파일로 읽지 않는다.
 */
export function parseApplyPatchFiles(patchText: unknown): AiMessageFile[] {
  if (typeof patchText !== "string") return [];

  const files = new Map<string, AiMessageFile>();
  for (const line of patchText.split(/\r?\n/)) {
    const header = /^\*\*\* (Add|Update) File: (.+)$/.exec(line.trimEnd());
    if (!header || files.has(header[2])) continue;
    files.set(header[2], { path: header[2], change: header[1] === "Add" ? "created" : "modified" });
  }
  return [...files.values()];
}

function readCodexPatchFiles(toolName: string, input: unknown): AiMessageFile[] {
  if (toolName === "apply_patch") return parseApplyPatchFiles(input);
  // 요즘 Codex는 패치를 `exec` 스크립트 안의 `tools.apply_patch("...")` 문자열로 남기고, 줄바꿈이 `\n` 두 글자로 적혀 있다.
  if (toolName === "exec" && typeof input === "string" && input.includes("tools.apply_patch(")) {
    return parseApplyPatchFiles(input.replaceAll("\\n", "\n"));
  }
  return [];
}

/** Codex 기록의 `custom_tool_call` 하나를 도구 이름, 입력 요약, 만들거나 고친 파일로 옮긴다 */
export function summarizeCodexCustomToolCall(payload: Record<string, unknown>): ToolCallSummary {
  const toolName = typeof payload.name === "string" && payload.name ? payload.name : "tool";
  const files = readCodexPatchFiles(toolName, payload.input);
  const inputLines = typeof payload.input === "string"
    ? payload.input.split(/\r?\n/).filter((line) => line.trim())
    : extractPlainText(payload.input).split("\n");
  const summary = files.length
    ? files.map((file) => file.path).join("\n")
    : inputLines.slice(0, CODEX_SUMMARY_LINE_COUNT).join("\n");

  return { toolName, summary: summary || `${toolName} called`, files };
}

/** 리더가 만든 메시지에 도구 이름과 파일 목록을 얹는다. 없는 값은 필드 자체를 두지 않는다 */
export function withToolCall(
  message: AggregatedAiMessage,
  toolCall: { toolName?: string; files?: AiMessageFile[] },
): AggregatedAiMessage {
  return {
    ...message,
    ...(toolCall.toolName ? { toolName: toolCall.toolName } : {}),
    ...(toolCall.files?.length ? { files: toolCall.files } : {}),
  };
}
