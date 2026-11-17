import { describe, expect, it } from "vitest";
import { readClaudePrompt } from "@/lib/aiSessions/paneChatClaudePrompt";

const BORDER = "─".repeat(40);

/** Claude Code 2.1.326이 실제로 그린 화면을 옮긴 것. 커서는 강조된 선택지의 ❯ 위에 놓인다 */
const singleQuestion = [
  "❯ Ask me a question",
  BORDER,
  " ☐ Color",
  "",
  "Which color do you prefer?",
  "",
  "❯ 1. Red (warm)",
  "     Warm",
  "  2. Green (calm)",
  "     Calm",
  "  3. Blue (cool)",
  "     Cool",
  "  4. Type something.",
  BORDER,
  "  5. Chat about this",
  "",
  "Enter to select · ↑/↓ to navigate · Esc to cancel",
];

const permission = [
  BORDER,
  " Bash command",
  " Create empty probe file",
  "╌".repeat(40),
  " touch kanvibe-probe.txt",
  "╌".repeat(40),
  " Do you want to proceed?",
  " ❯ 1. Yes",
  "   2. Yes, and always allow access to /home/user/kanvibe__worktrees/fix-lo",
  "      t-of-issues/workspace from this project",
  "   3. No",
  "",
  " Esc to cancel · Tab to amend",
];

const modelSwitch = [
  "▔".repeat(40),
  "   Switch model?",
  "   Your next response will be slower and use more tokens",
  "   ❯ 1. Yes, switch to Opus 5.5",
  "     2. No, go back",
  "   Enter to confirm · Esc to cancel",
];

const multiSelect = [
  BORDER,
  "←  ☒ Size  ☐ Toppings  ✔ Submit  →",
  "",
  "Which toppings?",
  "",
  "❯ 1. [✔] Cheese",
  "         Cheese",
  "  2. [ ] Olives",
  "         Olives",
  "  3. [ ] Type something",
  "     Submit",
  BORDER,
  "  4. Chat about this",
];

const review = [
  BORDER,
  "←  ☒ Size  ☒ Toppings  ✔ Submit  →",
  "",
  "Review your answers",
  "",
  " ● Which size?",
  "   → Medium",
  "",
  "Ready to submit your answers?",
  "",
  "❯ 1. Submit answers",
  "  2. Cancel",
];

describe("readClaudePrompt", () => {
  it("AI 질문 화면에서 질문·설명이 붙은 선택지·직접 입력 줄을 읽는다", () => {
    expect(readClaudePrompt(singleQuestion, 6, 0)).toEqual({
      kind: "question",
      title: "Which color do you prefer?",
      detail: "☐ Color",
      stepsLine: null,
      highlightedNumber: 1,
      isMultiSelect: false,
      isTextEntryActive: false,
      hasHiddenOptions: false,
      options: [
        { number: 1, label: "Red (warm)", description: "Warm", isChecked: false, isTextEntry: false },
        { number: 2, label: "Green (calm)", description: "Calm", isChecked: false, isTextEntry: false },
        { number: 3, label: "Blue (cool)", description: "Cool", isChecked: false, isTextEntry: false },
        { number: 4, label: "Type something.", description: "", isChecked: false, isTextEntry: true },
        { number: 5, label: "Chat about this", description: "", isChecked: false, isTextEntry: false },
      ],
    });
  });

  it("권한 확인 화면에서 실행하려는 명령을 상세로, 줄이 넘어간 선택지의 나머지를 설명으로 읽는다", () => {
    const prompt = readClaudePrompt(permission, 7, 1);

    expect(prompt).toMatchObject({ kind: "permission", title: "Do you want to proceed?", detail: "Bash command\nCreate empty probe file\n\ntouch kanvibe-probe.txt", highlightedNumber: 1 });
    expect(prompt?.options.map((option) => option.label)).toEqual(["Yes", "Yes, and always allow access to /home/user/kanvibe__worktrees/fix-lo", "No"]);
    expect(prompt?.options[1].description).toBe("t-of-issues/workspace from this project");
  });

  it("모델 전환 확인처럼 경계선 바로 아래에서 시작하는 화면은 안내문 전체를 제목으로 읽는다", () => {
    expect(readClaudePrompt(modelSwitch, 3, 3)).toMatchObject({
      kind: "confirmation",
      title: "Switch model?\nYour next response will be slower and use more tokens",
      detail: "",
      options: [{ number: 1, label: "Yes, switch to Opus 5.5" }, { number: 2, label: "No, go back" }],
    });
  });

  it("다중 선택 화면에서 체크 상태와 단계 줄을 읽고, 번호 없는 Submit 줄은 선택지 설명에 넣지 않는다", () => {
    expect(readClaudePrompt(multiSelect, 5, 0)).toMatchObject({
      stepsLine: "←  ☒ Size  ☐ Toppings  ✔ Submit  →",
      detail: "",
      isMultiSelect: true,
      options: [
        { number: 1, label: "Cheese", description: "Cheese", isChecked: true, isTextEntry: false },
        { number: 2, label: "Olives", description: "Olives", isChecked: false, isTextEntry: false },
        { number: 3, label: "Type something", description: "", isChecked: false, isTextEntry: true },
        { number: 4, label: "Chat about this", description: "", isChecked: false, isTextEntry: false },
      ],
    });
  });

  it("답 확인 화면은 지금까지 고른 답을 상세로 읽는다", () => {
    expect(readClaudePrompt(review, 10, 0)).toMatchObject({
      kind: "question",
      title: "Ready to submit your answers?",
      detail: "Review your answers\n\n ● Which size?\n   → Medium",
      stepsLine: "←  ☒ Size  ☒ Toppings  ✔ Submit  →",
    });
  });

  it("이미 고른 단일 선택지는 ✔ 표시를 떼고 체크된 것으로 읽는다", () => {
    const revisited = singleQuestion.map((line) => line === "  2. Green (calm)" ? "  2. Green (calm) ✔" : line);

    expect(readClaudePrompt(revisited, 6, 0)?.options[1]).toMatchObject({ label: "Green (calm)", isChecked: true });
  });

  it("커서가 강조된 줄의 글 위에 있으면 그 줄에 글을 입력받는 중으로 읽는다", () => {
    const typing = singleQuestion.map((line) => line === "❯ 1. Red (warm)" ? "  1. Red (warm)" : line === "  4. Type something." ? "❯ 4. Extra large" : line);

    expect(readClaudePrompt(typing, 12, 16)).toMatchObject({
      highlightedNumber: 4,
      isTextEntryActive: true,
      options: expect.arrayContaining([{ number: 4, label: "Extra large", description: "", isChecked: false, isTextEntry: true }]),
    });
  });

  it("계획 승인처럼 Would you like to로 묻는 화면도 권한 확인으로 읽는다", () => {
    expect(readClaudePrompt([BORDER, " Ready to code?", "", " Would you like to proceed?", "", " ❯ 1. Yes", "   2. No, keep planning"], 5, 1)?.kind).toBe("permission");
  });

  it("다중 선택의 직접 입력 줄은 강조되면 글을 입력받는 중으로 읽는다", () => {
    const typing = multiSelect.map((line) => line === "❯ 1. [✔] Cheese" ? "  1. [✔] Cheese" : line === "  3. [ ] Type something" ? "❯ 3. [ ] Type something" : line);

    expect(readClaudePrompt(typing, 9, 9)).toMatchObject({ highlightedNumber: 3, isTextEntryActive: true, isMultiSelect: true });
  });

  it("글을 입력한 줄 뒤에 칸을 띄우고 붙는 ✔도 떼어 내, 강조가 옮겨 가도 같은 선택지로 읽는다", () => {
    const withText = (row: string) => singleQuestion.map((line) => line === "  4. Type something." ? row : line);

    const whileTyping = readClaudePrompt(withText("  4. term  ✔"), 6, 0)?.options[3];
    const afterLeaving = readClaudePrompt(withText("  4. term ✔"), 6, 0)?.options[3];

    expect(whileTyping).toEqual({ number: 4, label: "term", description: "", isChecked: true, isTextEntry: false });
    expect(afterLeaving).toEqual(whileTyping);
  });

  it("모델 목록처럼 ✔가 줄 가운데에 있는 선택지도 고른 것으로 읽는다", () => {
    const modelList = ["▔".repeat(40), "   Select model", "     1.  Default (recommended)  Opus", "   ❯ 2.  Sonnet 5.5 ✔           Most efficient", "     ▸   More models"];

    expect(readClaudePrompt(modelList, 3, 3)?.options).toEqual([
      { number: 1, label: "Default (recommended)  Opus", description: "", isChecked: false, isTextEntry: false },
      { number: 2, label: "Sonnet 5.5           Most efficient", description: "", isChecked: true, isTextEntry: false },
    ]);
  });

  it("pane이 짧아 CLI가 선택지를 스크롤해 보여 주면, 보이는 선택지만 싣고 가려진 것이 있음을 알린다", () => {
    const scrolled = [BORDER, " ☐ Season", "", "Pick a season?", "", "❯ 1. Spring", "     mild days", "  2. Summer", "     hot days", "↓ 3. Autumn", "     cool days", BORDER, "  6. Chat about this", "Enter to select · ↑/↓ to navigate · Esc to cancel"];

    const prompt = readClaudePrompt(scrolled, 5, 0);

    expect(prompt?.hasHiddenOptions).toBe(true);
    expect(prompt?.options.map((option) => option.number)).toEqual([1, 2, 3, 6]);
    expect(prompt?.options[2]).toMatchObject({ label: "Autumn", description: "cool days" });
  });

  it("위쪽 경계선 자리에 CLI가 안내 글을 넣어도 그 글과 그 위의 대화를 제목에 섞지 않는다", () => {
    const noticeOnBorder = ["● earlier reply", "tmux focus-events off · add 'set -g focus-events on' to ~/.tmux.conf and re… ▔", ...modelSwitch.slice(1)];

    expect(readClaudePrompt(noticeOnBorder, 4, 3)).toMatchObject({ title: "Switch model?\nYour next response will be slower and use more tokens", detail: "" });
  });

  it.each([
    ["빈 입력란", [BORDER, "❯ ", BORDER, "  ? for shortcuts"], 1, 2],
    ["번호 목록을 쓰고 있는 입력란", [BORDER, "❯ 1. first", "  2. second", BORDER], 1, 10],
    ["대화 본문의 번호 목록 아래에 있는 입력란", ["Pick one:", "  1. first", "  2. second", BORDER, "❯ ", BORDER], 4, 2],
    ["선택지가 하나뿐인 화면", ["Continue?", "❯ 1. Yes"], 1, 0],
    ["스크롤 표시 없이 번호가 1부터 이어지지 않는 화면", ["Continue?", "❯ 2. Yes", "  3. No"], 1, 0],
    ["스크롤 표시 없이 번호가 건너뛰는 화면", ["Continue?", "❯ 1. Yes", "  3. No"], 1, 0],
    ["제목 없이 잘린 화면", ["❯ 1. Yes", "  2. No"], 0, 0],
  ])("%s은 선택 화면으로 읽지 않는다", (_name, lines, cursorRow, cursorColumn) => {
    expect(readClaudePrompt(lines, cursorRow, cursorColumn)).toBeNull();
  });
});
