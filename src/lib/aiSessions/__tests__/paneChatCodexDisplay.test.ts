import { describe, expect, it } from "vitest";
import { Terminal } from "@xterm/headless";
import { expectCodexComposerTexts, isDimHintEmptyComposer, isCodexMainComposerReady, isCodexThreadFooterSupported, isSameComposerText, readCodexDisplay } from "@/lib/aiSessions/paneChatCodexDisplay";

const DISPLAYED_THREAD = "0190aaaa-0000-7000-8000-00000000000a";
const OTHER_THREAD = "0190bbbb-0000-7000-8000-00000000000b";

describe("native empty composer appearance", () => {
  it.each(["Ask Codex to do anything", "Summarize recent commits"])("recognizes a dim native hint with Herdr's ordinary padding: %s", async (hint) => {
    const terminal = new Terminal({ cols: 80, rows: 4, allowProposedApi: true });
    try {
      await new Promise<void>((resolve) => terminal.write(`› \x1b[2m${hint}\x1b[0m${" ".repeat(40)}\x1b[1;3H`, resolve));
      expect(isDimHintEmptyComposer(terminal.buffer.active.getLine(0), 2)).toBe(true);
      const viewport = [`› ${hint}`, "", `  ${DISPLAYED_THREAD}`, "  ← for agents · ? for shortcuts"];
      expect(readCodexDisplay(viewport, 0, true)).toEqual({ composerText: "", threadId: DISPLAYED_THREAD });
    } finally { terminal.dispose(); }
  });

  it("refuses a user-written hint with identical visible characters", async () => {
    const terminal = new Terminal({ cols: 80, rows: 4, allowProposedApi: true });
    try {
      await new Promise<void>((resolve) => terminal.write("› Ask Codex to do anything\x1b[1;3H", resolve));
      expect(isDimHintEmptyComposer(terminal.buffer.active.getLine(0), 2)).toBe(false);
    } finally { terminal.dispose(); }
  });
});
const HINT_ROW = "  ← for agents · ? for shortcuts";

describe("Codex displayed-thread footer reading", () => {
  it.each(["  ← for agents", "  ← for agents        ⚠ 2 warnings · f2 to view"])("accepts the verified narrow main footer %s without relaxing its position", (hint) => {
    const rows = ["› Ask Codex to do anything", "", `  ${DISPLAYED_THREAD}`, hint];
    expect(readCodexDisplay(rows, 0, true)?.threadId).toBe(DISPLAYED_THREAD);
    expect(isCodexMainComposerReady(rows, 0)).toBe(true);
    expect(readCodexDisplay([...rows, "  more"], 0, true)?.threadId).toBeNull();
    rows[3] = "  Side from main thread · ⌃/ to switch · ⌃c to close";
    expect(readCodexDisplay(rows, 0, true)?.threadId).toBeNull();
    expect(isCodexMainComposerReady(rows, 0)).toBe(false);
  });

  it("keeps the native warning badge separate from the exact thread id", () => {
    const rows = ["› Ask Codex to do anything", "", `  ${DISPLAYED_THREAD}`, `${HINT_ROW}                       ⚠ 1 warning · f2 to view`];
    expect(readCodexDisplay(rows, 0, true)).toEqual({ composerText: "", threadId: DISPLAYED_THREAD });
    expect(isCodexMainComposerReady(rows, 0)).toBe(true);
    rows[3] = "  tab to queue message";
    expect(readCodexDisplay(rows, 0, true)?.threadId).toBe(DISPLAYED_THREAD);
    expect(isCodexMainComposerReady(rows, 0)).toBe(false);
  });
  it("reads the thread id from the last footer block below an empty composer", () => {
    expect(readCodexDisplay(["  >_ OpenAI Codex (v0.169.0)", "", "› Ask Codex to do anything", "", `  ${DISPLAYED_THREAD}`, HINT_ROW], 2))
      .toEqual({ composerText: "Ask Codex to do anything", threadId: DISPLAYED_THREAD });
  });

  it("accepts the native main-thread label after the id", () => {
    expect(readCodexDisplay(["› Ask Codex to do anything", "", `  ${DISPLAYED_THREAD} · Main [default]`, `${HINT_ROW} · ⌃/ for side`], 0)?.threadId).toBe(DISPLAYED_THREAD);
  });

  it("reads the real footer, not a UUID inside a multi-line draft, when the cursor sits on an earlier composer row", () => {
    const display = readCodexDisplay(["› draft", "", `  ${OTHER_THREAD}`, "", `  ${DISPLAYED_THREAD} · Main [default]`], 0);
    expect(display).toEqual({ composerText: `draft\n\n${OTHER_THREAD}`, threadId: DISPLAYED_THREAD });
  });

  it("reads a whole multi-row draft with its blank line above the footer", () => {
    expect(readCodexDisplay(["› first line", "", "  third line", "", `  ${DISPLAYED_THREAD}`], 2))
      .toEqual({ composerText: "first line\n\nthird line", threadId: DISPLAYED_THREAD });
  });

  it.each([
    ["the default footer without an id", "  gpt-sol high · ~/repo"],
    ["an id followed by other footer items", `  ${DISPLAYED_THREAD} · gpt-sol high · ~/repo`],
    ["an id cut off by a narrow pane", `  ${DISPLAYED_THREAD.slice(0, 30)}…`],
    ["an id that does not start the footer", `  thread ${DISPLAYED_THREAD}`],
    ["an id with trailing text", `  ${DISPLAYED_THREAD}0`],
  ])("reports no thread id for %s", (_, footerRow) => {
    expect(readCodexDisplay(["› Ask Codex to do anything", "", footerRow, HINT_ROW], 0)).toEqual({ composerText: "Ask Codex to do anything", threadId: null });
  });

  it("does not read a UUID from the transcript above the composer", () => {
    expect(readCodexDisplay([`  ${OTHER_THREAD}`, "", "› Ask Codex to do anything", "", "  gpt-sol high · ~/repo", HINT_ROW], 2)?.threadId).toBeNull();
  });

  it("reads the typed command while a popup covers the footer", () => {
    expect(readCodexDisplay(["› /mod", "", "  /model       choose what model and reasoning effort to use"], 0)).toEqual({ composerText: "/mod", threadId: null });
  });

  it("reads the composer below a command popup whose selected row carries the same marker", () => {
    expect(readCodexDisplay(["› /model  choose what model and reasoning effort to use", "", "› /mo", "", `  ${DISPLAYED_THREAD}`], 2))
      .toEqual({ composerText: "/mo", threadId: DISPLAYED_THREAD });
  });

  it("reads a draft taller than twelve rows when its first row is still on screen", () => {
    const draftRows = Array.from({ length: 14 }, (_, index) => `line ${index + 1}`);
    const display = readCodexDisplay([`› ${draftRows[0]}`, ...draftRows.slice(1).map((row) => `  ${row}`), "", `  ${DISPLAYED_THREAD}`], 13);
    expect(display).toEqual({ composerText: draftRows.join("\n"), threadId: DISPLAYED_THREAD });
  });

  it("returns nothing when the first composer row has scrolled out of the viewport", () => {
    expect(readCodexDisplay(["  line 11", "  line 12", "  line 13", "  line 14"], 3)).toBeNull();
  });

  it("returns nothing when the cursor is not on a composer", () => {
    expect(readCodexDisplay(["• Working (3s • esc to interrupt)", "", `  ${DISPLAYED_THREAD}`], 0)).toBeNull();
    expect(readCodexDisplay([], 0)).toBeNull();
  });
});

describe("Codex footer trust boundary", () => {
  const DRAFT_UUID = "11111111-1111-7111-8111-111111111111";

  it("does not take a draft UUID for the footer when arbitrary text sits where the shortcut hint belongs", () => {
    expect(readCodexDisplay(["› Ask Codex to do anything", "", `  ${DRAFT_UUID}`, "  this is still user input"], 0))
      .toEqual({ composerText: "Ask Codex to do anything", threadId: null });
  });

  it("does not accept an invented label after the id", () => {
    expect(readCodexDisplay(["› Ask Codex to do anything", "", `  ${DRAFT_UUID} · invented-label`], 0)?.threadId).toBeNull();
    expect(readCodexDisplay(["› draft", "", `  ${DRAFT_UUID} · invented-label`], 0)?.threadId).toBeNull();
  });

  it.each([
    ["no shortcut hint row below the footer", [`  ${DISPLAYED_THREAD}`]],
    ["a side-conversation hint", [`  ${DISPLAYED_THREAD}`, "  Side from main thread · ⌃/ to switch · ⌃c to close"]],
    ["a disconnected-session hint", [`  ${DISPLAYED_THREAD}`, "  ⌃c quit"]],
    ["more content below the hint row", [`  ${DISPLAYED_THREAD}`, HINT_ROW, "  more"]],
    ["two blank rows above the footer", ["", `  ${DISPLAYED_THREAD}`, HINT_ROW]],
  ])("does not prove an empty composer's thread with %s", (_, rowsBelowSeparator) => {
    expect(readCodexDisplay(["› Ask Codex to do anything", "", ...rowsBelowSeparator], 0)).toEqual({ composerText: "Ask Codex to do anything", threadId: null });
  });

  it("does not read a footer under a composer whose first row is blank or repeats the empty-composer hint above more rows", () => {
    expect(readCodexDisplay(["›", "", `  ${DISPLAYED_THREAD}`], 0)?.threadId).toBeNull();
    expect(readCodexDisplay(["› Ask Codex to do anything", "  second row", "", `  ${DISPLAYED_THREAD}`], 1)?.threadId).toBeNull();
  });

  it("reports a draft-layout id only with a composer text that differs from the empty-composer hint", () => {
    // 화면만으로는 초안의 마지막 줄과 footer를 구분할 수 없어, 이 배치의 ID는 이미 연결된 대화와 같은지 견주는 데만 쓴다.
    const display = readCodexDisplay(["› draft", "", `  ${DRAFT_UUID}`], 0);
    expect(display).toEqual({ composerText: "draft", threadId: DRAFT_UUID });
  });
});

describe("Codex thread footer support", () => {
  it("allows only the version whose inline footer command was verified on a real TUI", () => {
    expect(isCodexThreadFooterSupported("codex-cli 0.169.0")).toBe(true);
    expect(isCodexThreadFooterSupported("codex-cli 0.169.0\n")).toBe(true);
  });

  it.each(["codex-cli 0.168.0", "codex-cli 0.170.0", "codex-cli 0.169.0-alpha.1", "codex-cli 0.169.01", "Python 3.9.6", ""])("refuses %s", (versionOutput) => {
    expect(isCodexThreadFooterSupported(versionOutput)).toBe(false);
  });
});

describe("composer echo comparison without a viewport width", () => {
  it("keeps whitespace inside a row literal", () => {
    expect(isSameComposerText('print("ab")', 'print("a b")')).toBe(false);
    expect(isSameComposerText("a  b", "a b")).toBe(false);
    expect(isSameComposerText("  indented", "indented")).toBe(false);
  });

  it("never reads a row break as a folded line", () => {
    expect(isSameComposerText("ab\ncd", "ab cd")).toBe(false);
    expect(isSameComposerText("ab\ncd", "abcd")).toBe(false);
    expect(isSameComposerText(`${"x".repeat(33)}\ny`, `${"x".repeat(33)} y`)).toBe(false);
    expect(isSameComposerText(`${"wrap ".repeat(29).trim()}\n${"wrap ".repeat(16).trim()}`, "wrap ".repeat(45).trim())).toBe(false);
  });

  it("matches exact multi-line text, including wide characters", () => {
    expect(isSameComposerText("ab\ncd", "ab\ncd")).toBe(true);
    expect(isSameComposerText("가나다 라마바\n\n사아자 ✓", "가나다 라마바\n\n사아자 ✓")).toBe(true);
  });

  it("ignores only the whitespace a terminal row cannot show at the end of a line", () => {
    expect(isSameComposerText("Use $qa-skill", "Use $qa-skill ")).toBe(true);
    expect(isSameComposerText("first\n\nthird", "first \n\nthird")).toBe(true);
  });

  it("requires every displayed row and every expected line", () => {
    expect(isSameComposerText("first\nsecond", "first")).toBe(false);
    expect(isSameComposerText("first", "first\nsecond")).toBe(false);
    expect(isSameComposerText("first\n\nthird", "first\nthird")).toBe(false);
  });

  it("distinguishes different text and a partial echo", () => {
    expect(isSameComposerText("Ask Codex to do anything", "Must stay unsubmitted")).toBe(false);
    expect(isSameComposerText("/statusline thr", "/statusline thread-id")).toBe(false);
    expect(isSameComposerText("/statusline thr", "/statusline thr")).toBe(true);
  });
});

describe("composer echo comparison with the viewport width of the same capture", () => {
  it("accepts the observed 80-column Korean word wrap and forced syllable split", () => {
    expect(isSameComposerText("REAL_KOREAN_1794666776347 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글\n입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입\n력 확인 한글 입력 확인", "REAL_KOREAN_1794666776347 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인 한글 입력 확인", 80)).toBe(true);
    expect(isSameComposerText("한글\n입력", "한글 입력", 80)).toBe(false);
  });

  const VIEWPORT_COLUMNS = 150;
  // 150칸 화면의 입력란은 접두 두 칸을 뺀 148칸이다. 144칸을 채운 줄에는 공백과 네 글자 낱말이 더 들어가지 못한다.
  const FULL_ROW = "wrap ".repeat(29).trim();

  it("does not take a row break for a fold when the next word would have fit", () => {
    expect(isSameComposerText(`${"x".repeat(33)}\ny`, `${"x".repeat(33)} y`, VIEWPORT_COLUMNS)).toBe(false);
    expect(isSameComposerText("ab\ncd", "ab cd", VIEWPORT_COLUMNS)).toBe(false);
    expect(isSameComposerText(`${FULL_ROW}\nfit`, `${FULL_ROW} fit`, VIEWPORT_COLUMNS)).toBe(false);
  });

  it("joins the rows of a line folded at a space, as observed on a 150-column pane", () => {
    expect(isSameComposerText(`${FULL_ROW}\n${"wrap ".repeat(16).trim()}`, "wrap ".repeat(45).trim(), VIEWPORT_COLUMNS)).toBe(true);
    expect(isSameComposerText(`first line\n\n${FULL_ROW}\nwrap tail`, `first line\n\n${FULL_ROW}  wrap tail`, VIEWPORT_COLUMNS)).toBe(true);
  });

  it("joins the rows of a path folded after a hyphen, as observed on a 150-column pane", () => {
    const rowEndingAtHyphen = `@/${"d".repeat(128)}/desktop-pane-`;
    const display = `read this\n\n${rowEndingAtHyphen}\nchat-session/notes.md`;
    expect(isSameComposerText(display, `read this\n\n${rowEndingAtHyphen}chat-session/notes.md`, VIEWPORT_COLUMNS)).toBe(true);
    // 더 넓은 화면이었다면 다음 조각이 같은 줄에 들어갔을 것이므로 접힌 자리가 아니다.
    expect(isSameComposerText(display, `read this\n\n${rowEndingAtHyphen}chat-session/notes.md`, 160)).toBe(false);
    expect(isSameComposerText(`${rowEndingAtHyphen}\n2026/notes.md`, `${rowEndingAtHyphen}2026/notes.md`, VIEWPORT_COLUMNS)).toBe(false);
  });

  it("joins a token cut exactly at the text width and refuses a shorter cut", () => {
    expect(isSameComposerText(`${"a".repeat(148)}\naaa`, "a".repeat(151), VIEWPORT_COLUMNS)).toBe(true);
    expect(isSameComposerText(`${"a".repeat(147)}\naaaa`, "a".repeat(151), VIEWPORT_COLUMNS)).toBe(false);
  });

  it("refuses a row wider than the text width", () => {
    expect(isSameComposerText(`${"a".repeat(149)}\naa`, "a".repeat(151), VIEWPORT_COLUMNS)).toBe(false);
  });

  it.each([
    ["a closing bracket", `${FULL_ROW}\n) tail`, `${FULL_ROW} ) tail`],
    ["a slash", `${FULL_ROW}\n/usr/local/bin`, `${FULL_ROW} /usr/local/bin`],
    ["a comma", `${FULL_ROW}\n, tail`, `${FULL_ROW} , tail`],
  ])("refuses a fold before %s, where a line breaker may not separate the rows", (_, display, expected) => {
    expect(isSameComposerText(display, expected, VIEWPORT_COLUMNS)).toBe(false);
  });

  it("refuses a fold right after an opening bracket", () => {
    const rowEndingWithBracket = `${"wrap ".repeat(28)}wra(`;
    expect(isSameComposerText(`${rowEndingWithBracket}\nlonger`, `${rowEndingWithBracket} longer`, VIEWPORT_COLUMNS)).toBe(false);
  });

  it.each([
    ["an ambiguous-width symbol", `${"wrap ".repeat(28)}✓✓✓✓`, "wrap"],
    ["a combining mark", `${"wrap ".repeat(28)}éeee`, "wrap"],
    ["an emoji joined by zero-width joiners", `${"wrap ".repeat(27)}👩‍💻 wrap`, "wrap"],
  ])("refuses to restore a fold in %s, whose column width is not certain", (_, row, nextWord) => {
    expect(isSameComposerText(`${row}\n${nextWord}`, `${row} ${nextWord}`, VIEWPORT_COLUMNS)).toBe(false);
    expect(isSameComposerText(`${row}\n${nextWord}`, `${row}\n${nextWord}`, VIEWPORT_COLUMNS)).toBe(true);
  });

  it("keeps whitespace literal and line counts exact", () => {
    expect(isSameComposerText('print("ab")', 'print("a b")', VIEWPORT_COLUMNS)).toBe(false);
    expect(isSameComposerText("first\nsecond", "first", VIEWPORT_COLUMNS)).toBe(false);
    expect(isSameComposerText("Use $qa-skill", "Use $qa-skill ", VIEWPORT_COLUMNS)).toBe(true);
  });
});

describe("expected composer echo of a pasted prompt", () => {
  it("offers the literal text and its folded character-count placeholder", () => {
    expect(expectCodexComposerTexts("한글 ✓ text", 0)).toEqual(["한글 ✓ text", "[Pasted Content 9 chars]"]);
  });

  it("matches the composer rows observed after pasting an image and a caption", () => {
    expect(expectCodexComposerTexts("caption after image", 1).some((expected) => isSameComposerText("[Image #1] caption after image", expected))).toBe(true);
    expect(expectCodexComposerTexts("", 1).some((expected) => isSameComposerText("[Image #1]", expected))).toBe(true);
    expect(expectCodexComposerTexts("L".repeat(1500), 0).some((expected) => isSameComposerText("[Pasted Content 1500 chars]", expected))).toBe(true);
  });

  it("does not match another caption or a missing image placeholder", () => {
    expect(expectCodexComposerTexts("caption after image", 1).some((expected) => isSameComposerText("[Image #1] another caption", expected))).toBe(false);
    expect(expectCodexComposerTexts("caption after image", 1).some((expected) => isSameComposerText("caption after image", expected))).toBe(false);
    expect(expectCodexComposerTexts("", 2).some((expected) => isSameComposerText("[Image #1]", expected))).toBe(false);
  });
});
