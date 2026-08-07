/**
 * @vitest-environment node
 */
import { spawnSync } from "child_process";
import { describe, it, expect } from "vitest";
import {
  buildTmuxKillWindowCommand,
  buildTmuxListWindowsCommand,
  buildTmuxMoveWindowCommands,
  buildTmuxNewWindowCommand,
  buildTmuxRenameWindowCommand,
  buildTmuxSelectWindowCommand,
  buildZellijCloseFocusedTabCommands,
  buildZellijCloseTabByIdCommand,
  buildZellijGoToTabByIdCommand,
  buildZellijListTabsCommand,
  buildZellijMoveTabByIdCommands,
  buildZellijMoveTabCommands,
  buildZellijNewTabCommand,
  buildZellijRenameFocusedTabCommands,
  buildZellijRenameTabByIdCommand,
  parseTmuxWindowList,
  parseZellijFocusedTabName,
  parseZellijTabList,
  parseZellijTabNamesWithFocus,
  supportsZellijTabIdCommands,
} from "@/lib/terminalTabs";

const INJECTION_SESSION_NAME = "a'; rm -rf /; '";

describe("tmux window 목록 파싱", () => {
  it("id·인덱스·이름·활성 여부를 읽는다", () => {
    const output = ["@1\t0\t0\tzsh", "@2\t1\t1\tvim"].join("\n");

    expect(parseTmuxWindowList(output)).toEqual([
      { id: "@1", nativeIndex: 0, name: "zsh", isActive: false },
      { id: "@2", nativeIndex: 1, name: "vim", isActive: true },
    ]);
  });

  it("이름에 구분자로 쓰이지 않는 특수문자가 있어도 그대로 보존한다", () => {
    expect(parseTmuxWindowList("@1\t0\t0\ta|b:c")).toEqual([
      { id: "@1", nativeIndex: 0, name: "a|b:c", isActive: false },
    ]);
  });

  it("이름에 탭 문자가 섞여 들어와도 이름을 자르지 않는다", () => {
    expect(parseTmuxWindowList("@1\t0\t0\ta\tb")).toEqual([
      { id: "@1", nativeIndex: 0, name: "a\tb", isActive: false },
    ]);
  });

  it("빈 출력과 필드가 모자란 줄은 건너뛴다", () => {
    expect(parseTmuxWindowList("")).toEqual([]);
    expect(parseTmuxWindowList("@1\t0\t0")).toEqual([]);
    expect(parseTmuxWindowList("\n\n")).toEqual([]);
  });

  it("CRLF 줄바꿈에서도 마지막 필드가 오염되지 않는다", () => {
    expect(parseTmuxWindowList("@1\t0\t1\tzsh\r\n")).toEqual([
      { id: "@1", nativeIndex: 0, name: "zsh", isActive: true },
    ]);
  });
});

describe("tmux 탭 명령", () => {
  it("세션 이름을 POSIX 인용해 단일 인자로 넘긴다", () => {
    const command = buildTmuxListWindowsCommand(INJECTION_SESSION_NAME);
    const sessionArgument = command.match(/^tmux list-windows -t (.+) -F /)?.[1] ?? "";

    /** 실제 셸에 통과시켜, 인용이 세션 이름을 인자 하나로 되돌리는지 확인한다 */
    const echoed = spawnSync("sh", ["-c", `printf '%s' ${sessionArgument}`], { encoding: "utf-8" });

    expect(echoed.status).toBe(0);
    expect(echoed.stdout).toBe(INJECTION_SESSION_NAME);
  });

  it("window id를 세션 접두사 없이 대상으로 쓴다", () => {
    expect(buildTmuxSelectWindowCommand("@3")).toBe("tmux select-window -t '@3'");
    expect(buildTmuxKillWindowCommand("@3")).toBe("tmux kill-window -t '@3'");
  });

  it("새 window는 세션 대상 뒤 콜론과 작업 디렉터리를 붙인다", () => {
    expect(buildTmuxNewWindowCommand("proj-main", "/work/tree")).toBe(
      "tmux new-window -t 'proj-main': -P -F '#{window_id}' -c '/work/tree'",
    );
  });

  it("작업 디렉터리를 모르면 -c를 붙이지 않는다", () => {
    expect(buildTmuxNewWindowCommand("proj-main", null)).toBe(
      "tmux new-window -t 'proj-main': -P -F '#{window_id}'",
    );
  });

  it("이름 변경은 -- 뒤에 이름을 둬 대시로 시작하는 이름을 옵션으로 읽지 않는다", () => {
    expect(buildTmuxRenameWindowCommand("@3", "-n")).toBe("tmux rename-window -t '@3' -- '-n'");
  });

  /** -k는 목표 인덱스의 window를 죽인다. 순서만 바꾸려다 남의 탭을 지우면 안 된다 */
  it("왼쪽으로 옮길 때는 목표 window 앞에 끼우고 -r로 번호를 다시 채운다", () => {
    expect(buildTmuxMoveWindowCommands("proj-main", "@3", 2, 0)).toEqual([
      "tmux move-window -b -s '@3' -t 'proj-main':0",
      "tmux move-window -r -t 'proj-main'",
    ]);
  });

  it("오른쪽으로 옮길 때는 목표 window 뒤에 끼운다", () => {
    expect(buildTmuxMoveWindowCommands("proj-main", "@3", 0, 2)).toEqual([
      "tmux move-window -a -s '@3' -t 'proj-main':2",
      "tmux move-window -r -t 'proj-main'",
    ]);
  });

  it("제자리 이동은 명령을 내지 않는다", () => {
    expect(buildTmuxMoveWindowCommands("proj-main", "@3", 1, 1)).toEqual([]);
  });

  it("어떤 방향으로도 -k를 쓰지 않는다", () => {
    const allCommands = [
      ...buildTmuxMoveWindowCommands("proj-main", "@3", 2, 0),
      ...buildTmuxMoveWindowCommands("proj-main", "@3", 0, 2),
    ];

    expect(allCommands.some((command) => / -k( |$)/.test(command))).toBe(false);
  });
});

describe("zellij 버전 판정", () => {
  it("0.44.0부터 탭 id 명령을 지원한다", () => {
    expect(supportsZellijTabIdCommands("zellij 0.43.1")).toBe(false);
    expect(supportsZellijTabIdCommands("zellij 0.44.0")).toBe(true);
    expect(supportsZellijTabIdCommands("zellij 0.44.0 (main)")).toBe(true);
    expect(supportsZellijTabIdCommands("zellij 1.0.0")).toBe(true);
  });

  it("버전을 읽지 못하면 구버전으로 본다", () => {
    expect(supportsZellijTabIdCommands("")).toBe(false);
    expect(supportsZellijTabIdCommands("command not found")).toBe(false);
  });
});

describe("zellij 탭 목록 파싱", () => {
  it("list-tabs --json의 tab_id·position·active를 읽는다", () => {
    const output = JSON.stringify([
      { position: 0, tab_id: 0, name: "shell", active: false },
      { position: 1, tab_id: 1, name: "logs", active: true },
    ]);

    expect(parseZellijTabList(output)).toEqual([
      { id: "0", nativeIndex: 0, name: "shell", isActive: false },
      { id: "1", nativeIndex: 1, name: "logs", isActive: true },
    ]);
  });

  /**
   * zellij 0.44.3 실측: 가운데 탭을 닫으면 position은 다시 매겨지지만 tab_id는 유지된다.
   * position을 식별자로 쓰면 `close-tab-by-id`가 엉뚱한 탭을 지운다.
   */
  it("탭을 닫아 position이 밀려도 식별자는 tab_id를 따른다", () => {
    const outputAfterMiddleTabClosed = JSON.stringify([
      { position: 0, tab_id: 0, name: "Tab #1", active: false },
      { position: 1, tab_id: 2, name: "logs", active: true },
    ]);

    expect(parseZellijTabList(outputAfterMiddleTabClosed)).toEqual([
      { id: "0", nativeIndex: 0, name: "Tab #1", isActive: false },
      { id: "2", nativeIndex: 1, name: "logs", isActive: true },
    ]);
  });

  it("JSON이 아니거나 배열이 아니면 빈 목록을 준다", () => {
    expect(parseZellijTabList("not json")).toEqual([]);
    expect(parseZellijTabList('{"position":0}')).toEqual([]);
  });

  it("tab_id나 이름이 없는 항목은 건너뛴다", () => {
    const output = JSON.stringify([
      { position: 0, tab_id: 0 },
      { position: 1, name: "no-id" },
      { position: 2, tab_id: 2, name: "logs" },
    ]);

    expect(parseZellijTabList(output)).toEqual([
      { id: "2", nativeIndex: 2, name: "logs", isActive: false },
    ]);
  });
});

describe("zellij 구버전 폴백 파싱", () => {
  const layoutWithFocusOnSecondTab = [
    "layout {",
    '    tab name="first" {',
    "        pane",
    "    }",
    '    tab name="second" focus=true {',
    "        pane",
    "    }",
    "}",
  ].join("\n");

  it("레이아웃 덤프의 focus=true 탭을 활성으로 표시한다", () => {
    expect(parseZellijTabNamesWithFocus("first\nsecond", layoutWithFocusOnSecondTab)).toEqual([
      { id: "0", nativeIndex: 0, name: "first", isActive: false },
      { id: "1", nativeIndex: 1, name: "second", isActive: true },
    ]);
  });

  it("레이아웃 덤프가 비면 첫 탭을 활성으로 가정하고 목록은 유지한다", () => {
    expect(parseZellijTabNamesWithFocus("first\nsecond", "")).toEqual([
      { id: "0", nativeIndex: 0, name: "first", isActive: true },
      { id: "1", nativeIndex: 1, name: "second", isActive: false },
    ]);
  });

  it("이름 목록이 비면 빈 목록을 준다", () => {
    expect(parseZellijTabNamesWithFocus("", layoutWithFocusOnSecondTab)).toEqual([]);
  });

  it("이스케이프된 따옴표가 든 탭 이름을 복원한다", () => {
    const layout = 'tab name="say \\"hi\\"" focus=true {';

    expect(parseZellijFocusedTabName(layout)).toBe('say "hi"');
  });

  it("focus 표시가 없으면 null을 준다", () => {
    expect(parseZellijFocusedTabName('tab name="only" {')).toBeNull();
  });

  /** zellij 0.44.3 실측 덤프에는 swap 레이아웃의 이름 없는 tab 노드와 pane의 focus=true가 함께 들어 있다 */
  it("swap 레이아웃 tab 노드와 pane focus 표시에 속지 않는다", () => {
    const realDumpLayout = [
      "layout {",
      '    tab name="Tab #1" {',
      "        pane",
      "    }",
      '    tab name="logs" focus=true hide_floating_panes=true {',
      "        pane focus=true",
      "    }",
      "    swap_tiled_layout name=\"vertical\" {",
      "        tab max_panes=5 {",
      "            pane split_direction=\"vertical\"",
      "        }",
      "    }",
      "}",
    ].join("\n");

    expect(parseZellijFocusedTabName(realDumpLayout)).toBe("logs");
  });
});

describe("zellij 탭 명령", () => {
  it("모든 명령이 --session으로 대상 세션을 지정한다", () => {
    expect(buildZellijListTabsCommand("proj")).toBe("zellij --session 'proj' action list-tabs --json");
    expect(buildZellijGoToTabByIdCommand("proj", "2")).toBe(
      "zellij --session 'proj' action go-to-tab-by-id '2'",
    );
    expect(buildZellijCloseTabByIdCommand("proj", "2")).toBe(
      "zellij --session 'proj' action close-tab-by-id '2'",
    );
    expect(buildZellijRenameTabByIdCommand("proj", "2", "logs")).toBe(
      "zellij --session 'proj' action rename-tab-by-id '2' 'logs'",
    );
  });

  it("새 탭은 작업 디렉터리를 넘기고, 모르면 생략한다", () => {
    expect(buildZellijNewTabCommand("proj", "/work/tree")).toBe(
      "zellij --session 'proj' action new-tab --cwd '/work/tree'",
    );
    expect(buildZellijNewTabCommand("proj", null)).toBe("zellij --session 'proj' action new-tab");
  });

  it("구버전 폴백은 대상 탭으로 이동한 뒤 활성 탭 명령을 실행한다", () => {
    expect(buildZellijCloseFocusedTabCommands("proj", "logs")).toEqual([
      "zellij --session 'proj' action go-to-tab-name 'logs'",
      "zellij --session 'proj' action close-tab",
    ]);
    expect(buildZellijRenameFocusedTabCommands("proj", "logs", "build")).toEqual([
      "zellij --session 'proj' action go-to-tab-name 'logs'",
      "zellij --session 'proj' action rename-tab 'build'",
    ]);
  });

  it("순서 변경은 방향 명령을 칸 수만큼 반복한다", () => {
    expect(buildZellijMoveTabCommands("proj", "logs", 0, 2)).toEqual([
      "zellij --session 'proj' action go-to-tab-name 'logs'",
      "zellij --session 'proj' action move-tab right",
      "zellij --session 'proj' action move-tab right",
    ]);
    expect(buildZellijMoveTabCommands("proj", "logs", 2, 1)).toEqual([
      "zellij --session 'proj' action go-to-tab-name 'logs'",
      "zellij --session 'proj' action move-tab left",
    ]);
  });

  it("제자리 이동은 명령을 만들지 않는다", () => {
    expect(buildZellijMoveTabCommands("proj", "logs", 1, 1)).toEqual([]);
    expect(buildZellijMoveTabByIdCommands("proj", "2", 1, 1)).toEqual([]);
  });

  it("탭 id 경로는 포커스를 옮기지 않고 대상 탭만 이동시킨다", () => {
    const commands = buildZellijMoveTabByIdCommands("proj", "2", 0, 2);

    expect(commands).toEqual([
      "zellij --session 'proj' action move-tab right -t '2'",
      "zellij --session 'proj' action move-tab right -t '2'",
    ]);
    expect(commands.some((command) => command.includes("go-to-tab"))).toBe(false);
  });
});
