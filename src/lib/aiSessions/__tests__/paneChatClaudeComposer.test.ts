import { describe, expect, it } from "vitest";
import { isClaudeComposerShowingPaste, readClaudeComposerText } from "@/lib/aiSessions/paneChatClaudeComposer";

const BORDER = "─".repeat(40);
/** Claude Code 2.1.326의 입력란을 옮긴 것. 첫 줄은 표시 기호와 줄바꿈 없는 공백으로 시작하고, 이어지는 줄은 두 칸 들여쓴다. 커서는 마지막 줄에 있다 */
const screen = (...composerRows: string[]) => ["● earlier reply", BORDER, ...composerRows, BORDER, "  ? for shortcuts"];
const read = (...composerRows: string[]) => readClaudeComposerText(screen(...composerRows), 1 + composerRows.length);

describe("readClaudeComposerText", () => {
  it("한 줄 입력란의 글을 읽는다", () => {
    expect(read("❯ Reply with exactly: OK")).toBe("Reply with exactly: OK");
  });

  it("여러 줄로 그려진 입력란은 줄을 이어 읽는다", () => {
    expect(read("❯ first line", "  second line is here", "  third")).toBe("first line\nsecond line is here\nthird");
  });

  it("문단 사이의 빈 줄과 글 끝의 빈 줄은 화면에서 빈 줄로 그려진다. 그 줄도 입력란의 줄로 읽는다", () => {
    expect(read("❯\u00a0first paragraph", "", "  second paragraph")).toBe("first paragraph\n\nsecond paragraph");
    expect(read("❯\u00a0ends with a newline", "")).toBe("ends with a newline");
  });

  it("셸 모드에서는 표시 기호가 된 !를 글 앞에 되돌려 읽는다", () => {
    expect(read("! ls -la")).toBe("!ls -la");
  });

  it.each([
    ["커서가 선택 화면의 선택지 위에 있을 때", ["● reply", BORDER, " Do you want to proceed?", " ❯ 1. Yes", "   2. No"], 3],
    ["입력란 위가 경계선이 아닐 때", ["❯ an earlier prompt shown in the conversation", "  continued", BORDER], 1],
    ["커서 줄이 입력란의 줄이 아닐 때", [BORDER, "❯ text", BORDER, "  ? for shortcuts"], 3],
  ])("%s는 입력란으로 읽지 않는다", (_name, lines, cursorRow) => {
    expect(readClaudeComposerText(lines, cursorRow)).toBeNull();
  });
});

describe("isClaudeComposerShowingPaste", () => {
  it("입력란의 글이 붙여넣은 글과 공백·줄바꿈만 다르면 같은 글로 본다", () => {
    expect(isClaudeComposerShowingPaste("word000 word001\nword002", "word000 word001 word002", 0)).toBe(true);
  });

  it("다른 글이면 같은 글로 보지 않는다", () => {
    expect(isClaudeComposerShowingPaste("Reply with exactly: RACE", "Reply with exactly: OK", 0)).toBe(false);
    expect(isClaudeComposerShowingPaste("", "Reply with exactly: OK", 0)).toBe(false);
  });

  it("여러 줄을 접은 표시는 줄 수가 붙여넣은 글과 맞을 때만 받아들인다", () => {
    const fiveLines = "a\nb\nc\nd\ne";

    expect(isClaudeComposerShowingPaste("[Pasted text #2 +4 lines]", fiveLines, 0)).toBe(true);
    expect(isClaudeComposerShowingPaste("[Pasted text #2 +9 lines]", fiveLines, 0)).toBe(false);
    expect(isClaudeComposerShowingPaste("[Pasted text #2 +1 lines]", "a\nb", 0)).toBe(false);
  });

  it("줄 수 없는 접은 표시는 한 줄짜리 긴 글에만 받아들인다", () => {
    expect(isClaudeComposerShowingPaste("[Pasted text #5]", "x".repeat(1_100), 0)).toBe(true);
    expect(isClaudeComposerShowingPaste("[Pasted text #5]", "short text", 0)).toBe(false);
  });

  it("이미지를 붙였으면 그 수만큼의 이미지 표시가 글 앞에 있어야 한다", () => {
    expect(isClaudeComposerShowingPaste("[Image #7]describe this", "describe this", 1)).toBe(true);
    expect(isClaudeComposerShowingPaste("[Image #1][Image #2]", "", 2)).toBe(true);
    expect(isClaudeComposerShowingPaste("describe this", "describe this", 1)).toBe(false);
    expect(isClaudeComposerShowingPaste("[Image #7]describe this", "describe this", 0)).toBe(false);
  });

  it("셸 모드로 들어간 글도 같은 글로 본다", () => {
    expect(isClaudeComposerShowingPaste("!ls -la", "!ls -la", 0)).toBe(true);
  });
});
