/**
 * @vitest-environment node
 */
import { describe, it, expect } from "vitest";
import {
  buildTmuxCancelPipePaneCommand,
  buildTmuxCapturePaneCommand,
  buildTmuxListMirrorPanesCommand,
  buildTmuxPaneStreamCommand,
  buildTmuxSendKeysCommand,
  buildZellijDumpPaneCommand,
  buildZellijListMirrorPanesCommand,
  buildZellijWritePaneCommand,
  parseTmuxMirrorPaneList,
  parseZellijMirrorPaneList,
} from "@/lib/paneMirror";

const INJECTION_PANE_ID = "%1'; rm -rf /; '";

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
    expect(buildTmuxCapturePaneCommand("%19")).toBe("tmux capture-pane -pe -t '%19'");
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

  it("대시로 시작하는 입력이 tmux 옵션으로 먹히지 않는다", () => {
    expect(buildTmuxSendKeysCommand("%19", "--version")).toContain("-l -- '--version'");
  });

  it("pane id에 셸 메타문자가 들어와도 명령이 끊기지 않는다", () => {
    const command = buildTmuxCapturePaneCommand(INJECTION_PANE_ID);

    expect(command).toBe("tmux capture-pane -pe -t '%1'\"'\"'; rm -rf /; '\"'\"''");
    expect(command).not.toMatch(/;\s*rm -rf \/;?\s*$/);
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
      "zellij --session 'kanvibe-task' action dump-screen -p 'terminal_1' -a",
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
