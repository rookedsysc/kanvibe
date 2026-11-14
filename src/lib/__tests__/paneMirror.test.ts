/**
 * @vitest-environment node
 */
import { describe, it, expect } from "vitest";
import { buildHerdrCommand } from "@/lib/worktree";
import {
  buildHerdrObservePaneCommand,
  buildHerdrControlPaneCommand,
  buildHerdrPaneShortcutCommand,
  buildHerdrRecentScreenCommand,
  buildHerdrWritePaneCommand,
  parseHerdrObserveRecord,
  buildScreenRedraw,
  buildTmuxPaneShortcutCommand,
  buildZellijPaneShortcutCommand,
  buildHerdrClosePaneCommand,
  buildTmuxClosePaneCommand,
  buildZellijClosePaneCommand,
  buildTmuxCancelPipePaneCommand,
  buildTmuxCapturePaneCommand,
  buildTmuxPaneStateCommand,
  buildTmuxListMirrorPanesCommand,
  buildTmuxPaneStreamCommand,
  buildTmuxSendKeysCommand,
  buildZellijDumpPaneCommand,
  buildZellijListMirrorPanesCommand,
  buildZellijReadViewportCommand,
  buildZellijSelectTabCommand,
  buildZellijResizeFloatingPaneCommand,
  buildZellijWritePaneCommand,
  parseTmuxPaneState,
  buildHerdrInputModeSequences,
  parseTmuxMirrorPaneList,
  parseZellijMirrorPaneList,
  parseZellijViewportState,
} from "@/lib/paneMirror";

const INJECTION_PANE_ID = "%1'; rm -rf /; '";

describe("Zellij viewport geometry", () => {
  it("선택한 session과 tab을 명시해 탭을 활성화한다", () => {
    expect(buildZellijSelectTabCommand("task", "2"))
      .toBe("zellij --session 'task' action go-to-tab-by-id '2'");
  });
  it("floating 좌표와 테두리 포함 크기를 구분해 복원에 사용한다", () => {
    const output = JSON.stringify([[{ id: 7, is_plugin: false, is_floating: true, tab_id: 0,
      pane_x: 30, pane_y: 7, pane_columns: 60, pane_rows: 14, pane_content_columns: 58, pane_content_rows: 12 }],
      [{ tab_id: 0, viewport_columns: 120, viewport_rows: 28, display_area_columns: 120, display_area_rows: 30 }]]);
    expect(parseZellijViewportState(output, "terminal_7")).toMatchObject({ floating: { left: 30, top: 7 }, paneCols: 60, paneRows: 14 });
    expect(buildZellijResizeFloatingPaneCommand("task", "terminal_7", { left: 30, top: 7, cols: 46, rows: 72 }))
      .toBe("zellij --session 'task' action change-floating-pane-coordinates --pane-id 'terminal_7' --x 30 --y 7 --width 46 --height 72");
  });

  const pane = { id: 7, is_plugin: false, tab_id: 2, pane_columns: 60, pane_rows: 28, pane_content_columns: 59, pane_content_rows: 27 };
  const tab = { tab_id: 2, viewport_columns: 120, viewport_rows: 28, display_area_columns: 120, display_area_rows: 30 };

  it("선택 pane의 프로그램 격자와 분할·장식 크기를 구분한다", () => {
    expect(parseZellijViewportState(JSON.stringify([[pane, { ...pane, is_plugin: true }], [tab]]), "terminal_7")).toEqual({
      tabId: "2",
      cols: 59, rows: 27, paneCols: 60, paneRows: 28,
      viewportCols: 120, viewportRows: 28, displayCols: 120, displayRows: 30,
    });
  });

  it.each(["bad json", "[]", JSON.stringify([[pane], []]), JSON.stringify([[{ ...pane, pane_content_rows: 0 }], [tab]])])("불완전한 geometry는 크기 변경에 쓰지 않는다: %s", (output) => {
    expect(parseZellijViewportState(output, "terminal_7")).toBeNull();
  });

  it("다른 pane의 geometry를 대신 쓰지 않는다", () => {
    expect(parseZellijViewportState(JSON.stringify([[pane], [tab]]), "terminal_8")).toBeNull();
  });

  it("세션을 인용하고 pane과 tab JSON을 한 응답으로 읽는다", () => {
    const command = buildZellijReadViewportCommand("task's session");
    expect(command).toContain(buildZellijListMirrorPanesCommand("task's session"));
    expect(command).toContain("action list-tabs --json -a");
    expect(command).toContain("printf '['");
  });
});

describe("tmux pane 목록", () => {
  it("세션 전체 window의 pane을 좌표와 함께 읽는다", () => {
    const output = [
      "%19\t@10\tbash\t0\t0\t50\t30\tbash",
      "%20\t@10\tclaude\t51\t0\t49\t30\tbash",
      "%21\t@11\tbash\t0\t0\t100\t30\tsecond win",
    ].join("\n");

    expect(parseTmuxMirrorPaneList(output)).toEqual([
      { id: "%19", tabId: "@10", tabName: "bash", command: "bash", left: 0, top: 0, width: 50, height: 30 },
      { id: "%20", tabId: "@10", tabName: "bash", command: "claude", left: 51, top: 0, width: 49, height: 30 },
      { id: "%21", tabId: "@11", tabName: "second win", command: "bash", left: 0, top: 0, width: 100, height: 30 },
    ]);
  });

  it("탭 이름에 탭 문자가 들어와도 이름을 자르지 않는다", () => {
    const output = "%1\t@1\tbash\t0\t0\t80\t24\tname\twith\ttabs";

    expect(parseTmuxMirrorPaneList(output)[0].tabName).toBe("name\twith\ttabs");
  });

  it("필드가 모자란 줄은 건너뛰고 나머지는 남긴다", () => {
    const output = ["%1\t@1\tbash", "%2\t@1\tbash\t0\t0\t80\t24\tbash"].join("\n");

    expect(parseTmuxMirrorPaneList(output).map((pane) => pane.id)).toEqual(["%2"]);
  });

  it("크기가 0인 pane은 비출 수 없으므로 버린다", () => {
    const output = "%1\t@1\tbash\t0\t0\t0\t24\tbash";

    expect(parseTmuxMirrorPaneList(output)).toEqual([]);
  });

  it("빈 출력은 빈 목록이 된다", () => {
    expect(parseTmuxMirrorPaneList("")).toEqual([]);
  });
});

describe("tmux pane 명령", () => {
  it("세션 하나의 pane만 좌표 포함해 조회한다", () => {
    expect(buildTmuxListMirrorPanesCommand("kanvibe-task")).toBe(
      "tmux list-panes -s -t 'kanvibe-task' -F '#{pane_id}\t#{window_id}\t#{pane_current_command}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}\t#{window_name}'",
    );
  });

  it("스냅샷은 ANSI를 살려서 뜬다", () => {
    expect(buildTmuxCapturePaneCommand("%19")).toBe("tmux capture-pane -pe -S -2000 -t '%19'");
  });

  it("스트림은 파이프를 걸고 임시 파일을 따라간다", () => {
    const command = buildTmuxPaneStreamCommand("%19");

    expect(command).toContain("tmux pipe-pane -o -t '%19' \"cat >> '$sink'\"");
    expect(command).toContain('tail -n +1 -f "$sink"');
  });

  it("스트림이 끊기면 파이프와 임시 파일을 정리한다", () => {
    expect(buildTmuxPaneStreamCommand("%19")).toContain(
      `trap 'tmux pipe-pane -t '%19'; rm -f "$sink"; kill $tailpid 2>/dev/null' EXIT INT TERM`,
    );
  });

  it("tail을 배경으로 돌리고 기다려야 셸이 자신을 exec로 치환하지 않아 트랩이 남는다", () => {
    const command = buildTmuxPaneStreamCommand("%19");

    expect(command).toContain("tail -n +1 -f \"$sink\" & tailpid=$!");
    expect(command).toContain("wait $tailpid");
    expect(command).not.toMatch(/tail -n \+1 -f "\$sink"$/);
  });

  it("파이프 해제는 명령 인자를 주지 않는다", () => {
    expect(buildTmuxCancelPipePaneCommand("%19")).toBe("tmux pipe-pane -t '%19'");
  });

  it("입력은 키 이름으로 해석되지 않게 글자 그대로 보낸다", () => {
    expect(buildTmuxSendKeysCommand("%19", "Enter")).toBe("tmux send-keys -t '%19' -l -- 'Enter'");
  });

  it.each([
    ["\0", "tmux send-keys -t '%19' -- C-Space"],
    ["\0\0", "tmux send-keys -t '%19' -- C-Space && tmux send-keys -t '%19' -- C-Space"],
    ["한\0글", "tmux send-keys -t '%19' -l -- '한' && tmux send-keys -t '%19' -- C-Space && tmux send-keys -t '%19' -l -- '글'"],
    ["\0--version\0", "tmux send-keys -t '%19' -- C-Space && tmux send-keys -t '%19' -l -- '--version' && tmux send-keys -t '%19' -- C-Space"],
  ])("NUL 입력 %s도 프로세스 인자에 NUL 없이 원래 순서로 보낸다", (input, expected) => {
    const command = buildTmuxSendKeysCommand("%19", input);
    expect(command).not.toContain("\0");
    expect(command).toBe(expected);
  });

  it("대시로 시작하는 입력이 tmux 옵션으로 먹히지 않는다", () => {
    expect(buildTmuxSendKeysCommand("%19", "--version")).toContain("-l -- '--version'");
  });

  it("pane id에 셸 메타문자가 들어와도 명령이 끊기지 않는다", () => {
    const command = buildTmuxCapturePaneCommand(INJECTION_PANE_ID);

    expect(command).toBe("tmux capture-pane -pe -S -2000 -t '%1'\"'\"'; rm -rf /; '\"'\"''");
    expect(command).not.toMatch(/;\s*rm -rf \/;?\s*$/);
  });
});

describe("화면 다시 그리기", () => {
  const CLEAR = "\x1b[H\x1b[2J\x1b[3J";

  it("화면과 스크롤백을 지우고, 줄바꿈마다 줄 처음으로 돌아가게 한 뒤 커서를 제자리에 둔다", () => {
    expect(buildScreenRedraw("$ ls\nREADME.md\n", { x: 2, y: 2, paneHeight: 3 })).toBe(`${CLEAR}$ ls\r\nREADME.md\r\n\x1b[3G`);
  });

  it("스크롤백이 앞에 붙어도 커서는 보이는 영역 맨 아랫줄에서 위로 올라가 찾는다", () => {
    const redraw = buildScreenRedraw("old-1\nold-2\n$ \n\n", { x: 2, y: 0, paneHeight: 3 });
    expect(redraw).toBe(`${CLEAR}old-1\r\nold-2\r\n$ \r\n\r\n\x1b[2A\x1b[3G`);
  });

  it("모바일 스크롤백 상한을 넘는 덤프는 마지막 2000줄만 전달한다", () => {
    const screen = Array.from({ length: 2100 }, (_, i) => `line-${i}`).join("\n");
    const redraw = buildScreenRedraw(screen, null);
    expect(redraw).toBe(`${CLEAR}${Array.from({ length: 2000 }, (_, i) => `line-${i + 100}`).join("\r\n")}`);
  });

  it("이미 CRLF인 줄은 CR을 겹쳐 넣지 않는다", () => {
    expect(buildScreenRedraw("a\r\nb", null)).toBe(`${CLEAR}a\r\nb`);
  });

  it("커서를 모르면 옮기지 않는다", () => {
    expect(buildScreenRedraw("a\nb", null)).toBe(`${CLEAR}a\r\nb`);
  });

  it("tmux 커서 위치는 포커스를 바꾸지 않는 display-message로 읽는다", () => {
    expect(buildTmuxPaneStateCommand("%19")).toBe("tmux display-message -p -t '%19' '#{cursor_x} #{cursor_y} #{pane_height} #{alternate_on} #{mouse_standard_flag} #{mouse_button_flag} #{mouse_all_flag} #{mouse_sgr_flag} #{mouse_utf8_flag}'");
  });

  it("pane 상태를 읽고, 필드가 모자라거나 형식이 다르면 모르는 것으로 본다", () => {
    expect(parseTmuxPaneState("8 1 24 0 0 0 0 0 0")).toEqual({
      cursor: { x: 8, y: 1, paneHeight: 24 },
      restoreSequences: "\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l",
    });
    expect(parseTmuxPaneState("8 1 0 0 0 0 0 0")).toBeNull();
    expect(parseTmuxPaneState("8 1")).toBeNull();
    expect(parseTmuxPaneState("PANE_A")).toBeNull();
    expect(parseTmuxPaneState("")).toBeNull();
  });
});

describe("zellij pane 목록", () => {
  const paneEntry = (overrides: Record<string, unknown>) => ({
    id: 0,
    is_plugin: false,
    tab_id: 0,
    tab_name: "Tab #1",
    title: "Pane #1",
    terminal_command: null,
    pane_x: 0,
    pane_y: 0,
    pane_rows: 24,
    pane_columns: 40,
    ...overrides,
  });

  it("terminal pane을 좌표와 함께 읽는다", () => {
    const output = JSON.stringify([
      paneEntry({ id: 0, terminal_command: "claude", pane_x: 0, pane_columns: 40 }),
      paneEntry({ id: 1, terminal_command: "zsh", pane_x: 40, pane_columns: 40 }),
    ]);

    expect(parseZellijMirrorPaneList(output)).toEqual([
      { id: "terminal_0", tabId: "0", tabName: "Tab #1", command: "claude", left: 0, top: 0, width: 40, height: 24 },
      { id: "terminal_1", tabId: "0", tabName: "Tab #1", command: "zsh", left: 40, top: 0, width: 40, height: 24 },
    ]);
  });

  it("zellij 자신의 plugin pane은 비추지 않는다", () => {
    const output = JSON.stringify([
      paneEntry({ id: 0, is_plugin: true, title: "About Zellij" }),
      paneEntry({ id: 1, is_plugin: false }),
    ]);

    expect(parseZellijMirrorPaneList(output).map((pane) => pane.id)).toEqual(["terminal_1"]);
  });

  it("명령이 비어 있으면 zellij가 붙인 제목을 대신 쓴다", () => {
    const output = JSON.stringify([paneEntry({ terminal_command: null, title: "Pane #1" })]);

    expect(parseZellijMirrorPaneList(output)[0].command).toBe("Pane #1");
  });

  it("JSON이 아니면 빈 목록이 된다", () => {
    expect(parseZellijMirrorPaneList("TAB_ID  TAB_POS  PANE_ID")).toEqual([]);
  });
});

describe("zellij pane 명령", () => {
  it("표 형식은 파싱할 수 없으므로 JSON으로 조회한다", () => {
    expect(buildZellijListMirrorPanesCommand("kanvibe-task")).toBe(
      "zellij --session 'kanvibe-task' action list-panes --json -a",
    );
  });

  it("포커스를 바꾸지 않고 지정한 pane만 뜬다", () => {
    expect(buildZellijDumpPaneCommand("kanvibe-task", "terminal_1")).toBe(
      "kanvibe_mobile_dump=$(zellij --session 'kanvibe-task' action dump-screen -p 'terminal_1' -a --full) && printf '%s\\n' \"$kanvibe_mobile_dump\" | tail -n 2000",
    );
  });

  it("입력은 UTF-8 바이트 십진수로 펼쳐 보낸다", () => {
    expect(buildZellijWritePaneCommand("kanvibe-task", "terminal_1", "hi\n")).toBe(
      "zellij --session 'kanvibe-task' action write -p 'terminal_1' 104 105 10",
    );
  });

  it("한글 입력도 바이트 단위로 나뉜다", () => {
    expect(buildZellijWritePaneCommand("s", "terminal_0", "한")).toBe(
      "zellij --session 's' action write -p 'terminal_0' 237 149 156",
    );
  });
});


describe("pane 단축키 명령", () => {
  it.each([
    ["split-right", "tmux split-window -h -d -t '%19' -c '#{pane_current_path}'"],
    ["split-down", "tmux split-window -v -d -t '%19' -c '#{pane_current_path}'"],
    ["toggle-fullscreen", "tmux resize-pane -Z -t '%19'"],
    ["copy-mode", "tmux copy-mode -t '%19'"],
    ["scroll-up", "tmux copy-mode -t '%19' && tmux send-keys -X -t '%19' page-up"],
    ["scroll-down", "tmux copy-mode -t '%19' && tmux send-keys -X -t '%19' page-down"],
    ["scroll-bottom", "tmux copy-mode -t '%19' && tmux send-keys -X -t '%19' cancel"],
  ])("tmux %s는 prefix 대신 지정한 pane을 제어한다", (action, expected) => {
    expect(buildTmuxPaneShortcutCommand("%19", action)).toBe(expected);
  });

  it.each(["split-right", "split-down"])("zellij %s는 선택 pane의 탭에 만들고 PC 포커스를 유지한다", (action) => {
    expect(buildZellijPaneShortcutCommand("session", "terminal_3", "4", action)).toBe(
      `zellij --session 'session' action new-pane --direction ${action === "split-right" ? "right" : "down"} --tab-id '4' --no-focus`,
    );
  });

  it.each([
    ["toggle-fullscreen", "toggle-fullscreen"],
    ["toggle-floating", "toggle-pane-embed-or-floating"],
    ["scroll-up", "page-scroll-up"],
    ["scroll-down", "page-scroll-down"],
    ["scroll-bottom", "scroll-to-bottom"],
  ])("zellij %s는 다른 클라이언트 포커스 대신 pane id를 사용한다", (action, command) => {
    expect(buildZellijPaneShortcutCommand("session", "terminal_3", "4", action)).toBe(
      `zellij --session 'session' action ${command} --pane-id 'terminal_3'`,
    );
  });

  it("셸 명령과 다른 세션 전용 action을 거절한다", () => {
    for (const action of ["; rm -rf /", "toString", "unknown"]) {
      expect(buildTmuxPaneShortcutCommand("%19", action)).toBeNull();
      expect(buildZellijPaneShortcutCommand("session", "terminal_3", "4", action)).toBeNull();
    }
    expect(buildTmuxPaneShortcutCommand("%19", "toggle-floating")).toBeNull();
    expect(buildZellijPaneShortcutCommand("session", "terminal_3", "4", "copy-mode")).toBeNull();
  });

  it("대상 이름의 셸 메타문자를 이스케이프한다", () => {
    expect(buildTmuxPaneShortcutCommand(INJECTION_PANE_ID, "toggle-fullscreen"))
      .toBe(`tmux resize-pane -Z -t ${"'%1'\"'\"'; rm -rf /; '\"'\"''"}`);
    expect(buildZellijPaneShortcutCommand("a'b", "terminal_3", "4", "scroll-up"))
      .toContain(`--session 'a'\"'\"'b'`);
  });
});

describe("pane 종료 명령", () => {
  it("tmux는 지정한 pane 하나만 종료한다", () => {
    expect(buildTmuxClosePaneCommand("%19")).toBe("tmux kill-pane -t '%19'");
  });

  it("zellij는 포커스 대신 pane id로 지정한다", () => {
    expect(buildZellijClosePaneCommand("session", "terminal_3"))
      .toBe("zellij --session 'session' action close-pane --pane-id 'terminal_3'");
  });

  it("Herdr는 pane close에 pane id를 인자 하나로 넘긴다", () => {
    expect(buildHerdrClosePaneCommand("session", "w1:p1"))
      .toBe(buildHerdrCommand("session", ["pane", "close", "w1:p1"]));
  });

  it("대상 이름의 셸 메타문자를 이스케이프한다", () => {
    expect(buildTmuxClosePaneCommand(INJECTION_PANE_ID))
      .toBe(`tmux kill-pane -t ${"'%1'\"'\"'; rm -rf /; '\"'\"''"}`);
    expect(buildZellijClosePaneCommand("a'b", "terminal_3")).toContain(`--session 'a'\"'\"'b'`);
  });
});

describe("Herdr pane 명령", () => {
  const herdr = (...args: string[]) => buildHerdrCommand("session", args);

  it("제어 문자와 -로 시작하는 입력을 send-text 인자 하나로 넘긴다", () => {
    expect(buildHerdrWritePaneCommand("session", "w1:p1", "\x03")).toBe(herdr("pane", "send-text", "w1:p1", "\x03"));
    expect(buildHerdrWritePaneCommand("session", "w1:p1", "--help")).toBe(herdr("pane", "send-text", "w1:p1", "--help"));
  });

  it("프로세스 인자에 담을 수 없는 NUL은 Ctrl+Space 키로 보내고 앞뒤 순서를 지킨다", () => {
    expect(buildHerdrWritePaneCommand("session", "w1:p1", "a\0b")).toBe([
      herdr("pane", "send-text", "w1:p1", "a"),
      herdr("pane", "send-keys", "w1:p1", "ctrl+space"),
      herdr("pane", "send-text", "w1:p1", "b"),
    ].join(" && "));
  });

  it("분할은 PC 포커스를 유지하고 확대는 토글한다", () => {
    expect(buildHerdrPaneShortcutCommand("session", "w1:p1", "split-right")).toBe(herdr("pane", "split", "w1:p1", "--direction", "right", "--no-focus"));
    expect(buildHerdrPaneShortcutCommand("session", "w1:p1", "split-down")).toBe(herdr("pane", "split", "w1:p1", "--direction", "down", "--no-focus"));
    expect(buildHerdrPaneShortcutCommand("session", "w1:p1", "toggle-fullscreen")).toBe(herdr("pane", "zoom", "w1:p1", "--toggle"));
  });

  it.each(["scroll-up", "scroll-down", "scroll-bottom", "copy-mode", "toggle-floating"])("Herdr 명령이 없는 %s는 지원하지 않는다", (action) => {
    expect(buildHerdrPaneShortcutCommand("session", "w1:p1", action)).toBeNull();
  });

  it("이력은 ANSI를 살려 최대 2000줄 읽고 observe는 격자 크기를 지정한다", () => {
    expect(buildHerdrRecentScreenCommand("session", "w1:p1")).toBe(herdr("pane", "read", "w1:p1", "--source", "recent", "--lines", "2000", "--format", "ansi"));
    expect(buildHerdrObservePaneCommand("session", "w1:p1", 60, 38)).toBe(herdr("terminal", "session", "observe", "w1:p1", "--cols", "60", "--rows", "38"));
  });
});

describe("Herdr observe 레코드", () => {
  it("제어 프레임의 실제 격자를 읽어 resize ACK를 구분한다", () => {
    expect(parseHerdrObserveRecord(JSON.stringify({ type: "terminal.frame", bytes: "", full: true, width: 44, height: 70 })))
      .toMatchObject({ type: "frame", width: 44, height: 70 });
  });

  it("pane 제어 스트림은 명시한 크기로 열고 takeover를 강제하지 않는다", () => {
    expect(buildHerdrControlPaneCommand("session", "w1:p1", 44, 70))
      .toBe(buildHerdrCommand("session", ["terminal", "session", "control", "w1:p1", "--cols", "44", "--rows", "70"]));
  });

  it("프레임의 base64를 UTF-8로 풀고 전체 화면 여부를 읽는다", () => {
    const bytes = Buffer.from("\x1b[1;1H한글").toString("base64");
    expect(parseHerdrObserveRecord(JSON.stringify({ type: "terminal.frame", bytes, full: true }))).toEqual({ type: "frame", isFullScreen: true, output: "\x1b[1;1H한글" });
    expect(parseHerdrObserveRecord(JSON.stringify({ type: "terminal.frame", bytes, full: false }))).toMatchObject({ isFullScreen: false });
  });

  it("종료 레코드를 구분하고 깨진 줄과 모르는 레코드는 무시한다", () => {
    expect(parseHerdrObserveRecord('{"type":"terminal.closed","reason":"gone"}')).toEqual({ type: "closed" });
    expect(parseHerdrObserveRecord("{broken")).toBeNull();
    expect(parseHerdrObserveRecord('{"type":"terminal.other"}')).toBeNull();
  });
});

describe("buildHerdrInputModeSequences", () => {
  const RESET_MOUSE = "\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l";

  it("휠을 받는 프로그램이 앞쪽 프로세스 어디에 있든 전체 화면과 SGR 마우스 모드를 켠다", () => {
    expect(buildHerdrInputModeSequences(["codex", "node"])).toBe(`\x1b[?1049h${RESET_MOUSE}\x1b[?1000h\x1b[?1006h`);
  });

  it("휠을 받는다고 확인하지 않은 프로그램과 빈 pane은 일반 화면으로 두고 마우스 모드를 끈다", () => {
    expect(buildHerdrInputModeSequences(["claude", "serena"])).toBe(`\x1b[?1049l${RESET_MOUSE}`);
    expect(buildHerdrInputModeSequences([])).toBe(`\x1b[?1049l${RESET_MOUSE}`);
  });
});
