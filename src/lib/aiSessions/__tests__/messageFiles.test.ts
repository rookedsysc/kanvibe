import { describe, expect, it } from "vitest";
import { parseApplyPatchFiles, summarizeClaudeToolUse, summarizeCodexCustomToolCall } from "@/lib/aiSessions/messageFiles";

describe("summarizeClaudeToolUse", () => {
  it("Write는 만든 파일로, Edit·MultiEdit·NotebookEdit는 고친 파일로 옮긴다", () => {
    expect(summarizeClaudeToolUse({ type: "tool_use", name: "Write", input: { file_path: "/repo/a.md", content: "x" } }))
      .toEqual({ toolName: "Write", summary: "/repo/a.md", files: [{ path: "/repo/a.md", change: "created" }] });
    expect(summarizeClaudeToolUse({ type: "tool_use", name: "MultiEdit", input: { file_path: "/repo/b.ts", edits: [] } }))
      .toEqual({ toolName: "MultiEdit", summary: "/repo/b.ts", files: [{ path: "/repo/b.ts", change: "modified" }] });
    expect(summarizeClaudeToolUse({ type: "tool_use", name: "NotebookEdit", input: { notebook_path: "/repo/c.ipynb", new_source: "x" } }))
      .toEqual({ toolName: "NotebookEdit", summary: "/repo/c.ipynb", files: [{ path: "/repo/c.ipynb", change: "modified" }] });
  });

  it("Read는 경로를, Bash는 명령을 요약으로 쓰고 파일은 싣지 않는다", () => {
    expect(summarizeClaudeToolUse({ type: "tool_use", name: "Read", input: { file_path: "/repo/a.md", limit: 10 } }))
      .toEqual({ toolName: "Read", summary: "/repo/a.md", files: [] });
    expect(summarizeClaudeToolUse({ type: "tool_use", name: "Bash", input: { command: "pnpm check", description: "타입 검사" } }))
      .toEqual({ toolName: "Bash", summary: "pnpm check", files: [] });
  });

  it("그 밖의 도구는 입력을 한 줄 JSON으로 줄여 보여 준다", () => {
    expect(summarizeClaudeToolUse({ type: "tool_use", name: "ToolSearch", input: { query: "notebook", max_results: 5 } }))
      .toEqual({ toolName: "ToolSearch", summary: "{\"query\":\"notebook\",\"max_results\":5}", files: [] });

    const longSummary = summarizeClaudeToolUse({ type: "tool_use", name: "Agent", input: { prompt: "가".repeat(600) } })?.summary ?? "";
    expect(longSummary.length).toBeLessThanOrEqual(242);
    expect(longSummary.endsWith("...")).toBe(true);
  });

  it("경로가 없거나 문자열이 아니면 던지지 않고 파일 없이 돌려준다", () => {
    expect(summarizeClaudeToolUse({ type: "tool_use", name: "Write", input: { content: "x" } }))
      .toEqual({ toolName: "Write", summary: "{\"content\":\"x\"}", files: [] });
    expect(summarizeClaudeToolUse({ type: "tool_use", name: "Edit", input: { file_path: 42 } }))
      .toEqual({ toolName: "Edit", summary: "{\"file_path\":42}", files: [] });
    expect(summarizeClaudeToolUse({ type: "tool_use", name: "Write", input: "not an object" }))
      .toEqual({ toolName: "Write", summary: "{}", files: [] });
  });

  it("도구 호출 조각이 아니면 null이다", () => {
    expect(summarizeClaudeToolUse({ type: "text", text: "hello" })).toBeNull();
    expect(summarizeClaudeToolUse({ type: "tool_use", input: {} })).toBeNull();
    expect(summarizeClaudeToolUse(null)).toBeNull();
    expect(summarizeClaudeToolUse("tool_use")).toBeNull();
  });
});

describe("parseApplyPatchFiles", () => {
  it("Add는 만든 파일, Update는 고친 파일로 읽고 Delete는 버린다", () => {
    const patch = "*** Begin Patch\n*** Add File: docs/a.md\n+x\n*** Update File: src/b.ts\n@@\n-a\n+b\n*** Delete File: src/c.ts\n*** End Patch";

    expect(parseApplyPatchFiles(patch)).toEqual([
      { path: "docs/a.md", change: "created" },
      { path: "src/b.ts", change: "modified" },
    ]);
  });

  it("같은 파일이 여러 번 나오면 처음 것 하나만 남긴다", () => {
    expect(parseApplyPatchFiles("*** Add File: a.md\n*** Update File: a.md")).toEqual([{ path: "a.md", change: "created" }]);
  });

  it("고치는 파일 본문에 든 머리줄 모양의 문맥·추가·삭제 줄은 파일로 읽지 않는다", () => {
    const patch = "*** Begin Patch\n*** Update File: src/a.test.ts\n@@\n *** Update File: fake/context.ts\n-*** Add File: fake/removed.ts\n+*** Add File: fake/added.ts\n*** End Patch";

    expect(parseApplyPatchFiles(patch)).toEqual([{ path: "src/a.test.ts", change: "modified" }]);
  });

  it("문자열이 아니거나 경로 없는 머리줄은 던지지 않고 빈 목록이다", () => {
    expect(parseApplyPatchFiles(undefined)).toEqual([]);
    expect(parseApplyPatchFiles({ patch: "*** Add File: a.md" })).toEqual([]);
    expect(parseApplyPatchFiles("*** Add File:\n*** Update File: ")).toEqual([]);
  });
});

describe("summarizeCodexCustomToolCall", () => {
  it("exec 스크립트 안에서 부른 apply_patch의 파일도 읽는다", () => {
    const input = "text(await tools.apply_patch(\"*** Begin Patch\\n*** Add File: docs/a.md\\n+x\\n*** Update File: src/b.ts\\n@@\\n-a\\n+b\\n*** End Patch\"));";

    expect(summarizeCodexCustomToolCall({ type: "custom_tool_call", name: "exec", input })).toEqual({
      toolName: "exec",
      summary: "docs/a.md\nsrc/b.ts",
      files: [{ path: "docs/a.md", change: "created" }, { path: "src/b.ts", change: "modified" }],
    });
  });

  it("패치가 아닌 호출은 입력의 앞 세 줄을 요약으로 쓴다", () => {
    const input = "text(await tools.exec_command({cmd:\"ls\"}));\n\nline 2\nline 3\nline 4";

    expect(summarizeCodexCustomToolCall({ type: "custom_tool_call", name: "exec", input })).toEqual({
      toolName: "exec",
      summary: "text(await tools.exec_command({cmd:\"ls\"}));\nline 2\nline 3",
      files: [],
    });
  });

  it("이름이나 입력이 없어도 던지지 않는다", () => {
    expect(summarizeCodexCustomToolCall({ type: "custom_tool_call" })).toEqual({ toolName: "tool", summary: "tool called", files: [] });
    expect(summarizeCodexCustomToolCall({ type: "custom_tool_call", name: "apply_patch", input: 42 }))
      .toEqual({ toolName: "apply_patch", summary: "apply_patch called", files: [] });
  });
});
