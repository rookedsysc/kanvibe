import { describe, expect, it } from "vitest";
import { readCodexPrompt } from "@/lib/aiSessions/paneChatCodexPrompt";

/** Codex CLI 0.169.0이 실제로 그린 화면을 옮긴 것. 선택 화면은 화면 아래에 붙고, 위쪽은 빈 줄 둘로 대화 내용과 떨어진다 */
const HISTORY = ["• Ran git tag probe-approval-1", "  └ (no output)", "", "  Worked for 28s • 6:23 PM", "", ""];
const approval = [
  ...HISTORY,
  "  Would you like to run the following command?",
  "",
  "  Environment: local",
  "",
  "  Reason: Allow creation of the requested Git tag?",
  "",
  "  $ git tag probe-approval-1",
  "",
  "",
  "› 1. Yes, proceed (y)",
  "  2. Yes, and don't ask again for commands that start with `git tag probe-approval-1` (p)",
  "  3. No, and tell Codex what to do differently (esc)",
  "",
  "  Press enter to confirm or esc to cancel",
];
const rateLimit = [
  ...HISTORY,
  "  Approaching rate limits",
  "  Switch to gpt-6-luna for lower credit usage?",
  "",
  "",
  "  1. Switch to gpt-6-luna                   Fast and affordable model for easier tasks.",
  "› 2. Keep current model",
  "  3. Keep current model (never show again)  Hide future rate limit reminders about switching models",
  "",
  "  enter select · esc back",
];
const composer = [
  ...HISTORY,
  "• Working (5s • esc to interrupt)",
  "",
  "› Ask Codex to do anything",
  "",
  "  GPT-6.1 Terra medium · ~/repo",
  "  ← for agents · ? for shortcuts",
];

describe("readCodexPrompt", () => {
  it("명령 승인 화면을 권한 확인으로 읽고, 실행하려는 명령을 상세로 싣는다", () => {
    expect(readCodexPrompt(approval)).toEqual({
      kind: "permission",
      title: "Would you like to run the following command?",
      detail: "Environment: local\n\nReason: Allow creation of the requested Git tag?\n\n$ git tag probe-approval-1",
      stepsLine: null,
      highlightedNumber: 1,
      isMultiSelect: false,
      isTextEntryActive: false,
      hasHiddenOptions: false,
      options: [
        { number: 1, label: "Yes, proceed (y)", description: "", isChecked: false, isTextEntry: false },
        { number: 2, label: "Yes, and don't ask again for commands that start with `git tag probe-approval-1` (p)", description: "", isChecked: false, isTextEntry: false },
        { number: 3, label: "No, and tell Codex what to do differently (esc)", description: "", isChecked: false, isTextEntry: false },
      ],
    });
  });

  it("사용량 경고 화면을 CLI의 확인으로 읽고, 같은 줄에 떨어져 있는 글을 선택지의 설명으로 읽는다", () => {
    expect(readCodexPrompt(rateLimit)).toMatchObject({
      kind: "confirmation",
      title: "Approaching rate limits\nSwitch to gpt-6-luna for lower credit usage?",
      detail: "",
      highlightedNumber: 2,
      options: [
        { number: 1, label: "Switch to gpt-6-luna", description: "Fast and affordable model for easier tasks." },
        { number: 2, label: "Keep current model", description: "" },
        { number: 3, label: "Keep current model (never show again)", description: "Hide future rate limit reminders about switching models" },
      ],
    });
  });

  describe("pane이 좁아 선택지가 여러 줄로 그려질 때", () => {
    it("넘어간 라벨을 한 라벨로 잇는다. 붙임표 뒤에서 끊긴 낱말은 공백 없이 잇는다", () => {
      const narrowApproval = approval.flatMap((line) => line.startsWith("  2. ") ? ["  2. Yes, and don't ask again for commands that start with `git tag qa-approval-", "     yes` (p)"] : [line]);

      expect(readCodexPrompt(narrowApproval)?.options).toMatchObject([
        { number: 1, label: "Yes, proceed (y)" },
        { number: 2, label: "Yes, and don't ask again for commands that start with `git tag qa-approval-yes` (p)", description: "" },
        { number: 3, label: "No, and tell Codex what to do differently (esc)" },
      ]);
    });

    it("설명 칸에서 넘어간 줄은 그 선택지의 설명에 잇는다", () => {
      const narrowRateLimit = [
        ...HISTORY,
        "  Approaching rate limits",
        "  Switch to gpt-6-luna for lower credit usage?",
        "",
        "",
        "› 1. Switch to gpt-6-luna                   Fast and affordable model for easier",
        "                                            tasks.",
        "  2. Keep current model",
        "  3. Keep current model (never show again)  Hide future rate limit reminders",
        "                                            about switching models",
        "",
        "  enter select · esc back",
      ];

      expect(readCodexPrompt(narrowRateLimit)).toMatchObject({
        title: "Approaching rate limits\nSwitch to gpt-6-luna for lower credit usage?",
        highlightedNumber: 1,
        options: [
          { number: 1, label: "Switch to gpt-6-luna", description: "Fast and affordable model for easier tasks." },
          { number: 2, label: "Keep current model", description: "" },
          { number: 3, label: "Keep current model (never show again)", description: "Hide future rate limit reminders about switching models" },
        ],
      });
    });
  });

  it("선택 화면 아래에 빈 줄이 남아 있어도 읽는다", () => {
    expect(readCodexPrompt([...rateLimit, "", ""])?.title).toContain("Approaching rate limits");
  });

  it("선택 화면 위의 대화 내용을 제목이나 상세에 섞지 않는다", () => {
    const prompt = readCodexPrompt(approval);

    expect(`${prompt?.title}\n${prompt?.detail}`).not.toContain("Worked for");
  });

  it.each([
    ["입력란이 보이는 화면", composer],
    ["안내 줄에 esc가 없는 번호 목록", rateLimit.map((line) => line === "  enter select · esc back" ? "  enter select" : line)],
    ["선택지가 하나뿐인 화면", ["", "", "  Continue?", "", "", "› 1. Yes", "", "  enter select · esc back"]],
    ["번호가 1부터 이어지지 않는 화면", ["", "", "  Continue?", "", "", "› 2. Yes", "  3. No", "", "  enter select · esc back"]],
    ["강조된 선택지가 없는 화면", ["", "", "  Continue?", "", "", "  1. Yes", "  2. No", "", "  enter select · esc back"]],
    ["제목이 없는 화면", ["› 1. Yes", "  2. No", "", "  enter select · esc back"]],
    ["선택지와 안내 줄 사이에 빈 줄이 없는 화면", ["", "", "  Continue?", "", "", "› 1. Yes", "  2. No", "  enter select · esc back"]],
  ])("%s은 선택 화면으로 읽지 않는다", (_name, lines) => {
    expect(readCodexPrompt(lines)).toBeNull();
  });
});
