import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";
const mocks = vi.hoisted(() => ({ task: vi.fn(), exec: vi.fn(), tabs: vi.fn(), snapshot: vi.fn(), attachment: vi.fn(), herdrFocus: vi.fn(), herdrFrame: vi.fn(), herdrPanes: vi.fn(), herdrTabs: vi.fn(), herdr: vi.fn() }));
vi.mock("../kanbanService", () => ({ getTaskById: mocks.task }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.exec }));
vi.mock("@/lib/terminal", () => ({ listLocalTerminalTabs: mocks.tabs, readLocalTerminalTabSnapshot: mocks.snapshot, readTaskTerminalAttachment: mocks.attachment, getHerdrTaskActivePaneId: mocks.herdrFocus }));
vi.mock("../mobileBridgeService", () => ({ readHerdrPaneFrameForChat: mocks.herdrFrame }));
vi.mock("@/lib/herdr", () => ({ listHerdrMirrorPanes: mocks.herdrPanes, readHerdrTabs: mocks.herdrTabs, runHerdrCommand: mocks.herdr }));
import { readPaneChatSurface } from "../paneChatSurface";
function withAttachedClient(resolve: (command: string) => string) {
  return async (command: string) => {
    if (command.startsWith("tmux list-clients")) return "9000\tkv\t$1\t\t%3";
    if (command.startsWith("tmux display-message -p -t 'kv'")) return "$1\t";
    return resolve(command);
  };
}
const task = { id: "task", sessionType: SessionType.TMUX, sessionName: "kv", sshHost: null };
beforeEach(() => {
  vi.clearAllMocks(); mocks.task.mockResolvedValue(task);
  mocks.attachment.mockReturnValue({ pid: 9000, sessionType: SessionType.TMUX, sessionName: "kv" });
});

describe("pane chat surface ownership", () => {
  it("selects the focused pane in the active tmux window instead of the first pane", async () => {
    mocks.exec.mockImplementation(withAttachedClient((command) => command.startsWith("tmux list-panes") ? "%1\t100\t0\t1\n%2\t200\t1\t0\n%3\t300\t1\t1" : "exact pane screen"));
    expect(await readPaneChatSurface("task")).toMatchObject({ paneId: "%3", rootPid: 300, screen: "exact pane screen" });
    expect(mocks.exec).toHaveBeenLastCalledWith(expect.stringContaining(" -t '%3'"), null, expect.anything());
  });
  it("rejects a requested tmux pane outside the task session", async () => {
    mocks.exec.mockResolvedValue("%1\t100\t1\t1");
    expect(await readPaneChatSurface("task", "%999")).toBeNull();
    expect(mocks.exec).toHaveBeenCalledTimes(1);
  });
  it("does not create a missing session while inspecting chat", async () => {
    mocks.task.mockResolvedValue({ ...task, sessionName: null });
    expect(await readPaneChatSurface("task")).toBeNull();
    expect(mocks.exec).not.toHaveBeenCalled();
  });
  it("reads the active owned plain terminal tab and its existing PTY", async () => {
    mocks.task.mockResolvedValue({ ...task, sessionType: SessionType.TERMINAL });
    mocks.tabs.mockReturnValue([{ id: "task-1", isActive: false }, { id: "task-2", isActive: true }]);
    mocks.snapshot.mockReturnValue({ pid: 400, screen: "plain CLI screen" });
    expect(await readPaneChatSurface("task")).toMatchObject({ paneId: "task-2", rootPid: 400, screen: "plain CLI screen" });
    expect(mocks.snapshot).toHaveBeenCalledWith("task", "task-2");
  });
  it("keeps a Zellij screen available when the installed version has no process identity", async () => {
    mocks.task.mockResolvedValue({ ...task, sessionType: SessionType.ZELLIJ });
    mocks.exec.mockImplementation(async (command: string) => command.includes("list-panes") ? JSON.stringify([{ id: 7, is_plugin: false, is_focused: true }]) : "zellij screen");
    expect(await readPaneChatSurface("task")).toMatchObject({ paneId: "terminal_7", rootPid: null, screen: "zellij screen" });
  });
  it("tmux와 Zellij 캡처의 ANSI 색상과 OSC 링크를 plain text 화면에서 제거한다", async () => {
    mocks.exec.mockImplementation(withAttachedClient((command) => command.startsWith("tmux list-panes") ? "%3\t300\t1\t1" : "\x1b[32mSelected model\x1b[0m\n\x1b]8;;https://example.com\x07Safe link\x1b]8;;\x07"));
    expect((await readPaneChatSurface("task"))?.screen).toBe("Selected model\nSafe link");
    mocks.task.mockResolvedValue({ ...task, sessionType: SessionType.ZELLIJ });
    mocks.exec.mockImplementation(async (command: string) => command.includes("list-panes") ? JSON.stringify([{ id: 7, is_plugin: false, is_focused: true }]) : "\x1b[32mZellij model\x1b[0m");
    expect((await readPaneChatSurface("task"))?.screen).toBe("Zellij model");
  });

  it("grouped tmux client의 실제 활성 pane을 canonical session의 활성 pane보다 우선한다", async () => {
    mocks.exec.mockImplementation(async (command: string) => {
      if (command.startsWith("tmux list-clients")) return "9000\tkv-group\t$2\tgroup-1\t%3";
      if (command.startsWith("tmux display-message -p -t 'kv'")) return "$1\tgroup-1";
      if (command.startsWith("tmux list-panes")) return "%2\t200\t1\t1\n%3\t300\t0\t1";
      return "exact client pane";
    });
    expect(await readPaneChatSurface("task")).toMatchObject({ paneId: "%3", rootPid: 300 });
  });
  it("attached tmux client가 관계없는 세션으로 전환됐으면 canonical pane에 쓰지 않는다", async () => {
    mocks.exec.mockImplementation(async (command: string) => {
      if (command.startsWith("tmux list-clients")) return "9000\tunrelated\t$9\tother-group\t%90";
      if (command.startsWith("tmux display-message -p -t 'kv'")) return "$1\tgroup-1";
      return "%2\t200\t1\t1";
    });
    expect(await readPaneChatSurface("task")).toBeNull();
  });

  it("attached client가 없으면 desktop 추정을 거부하지만 명시한 모바일 pane은 소유권으로 확인한다", async () => {
    mocks.attachment.mockReturnValue(null);
    expect(await readPaneChatSurface("task")).toBeNull();
    expect(mocks.exec).not.toHaveBeenCalled();
    mocks.exec.mockImplementation(async (command: string) => command.startsWith("tmux list-panes") ? "%3\t300\t1\t1" : "mobile selected pane");
    expect(await readPaneChatSurface("task", "%3")).toMatchObject({ paneId: "%3", rootPid: 300 });
    expect(mocks.exec.mock.calls.some((call) => call[0].startsWith("tmux list-clients"))).toBe(false);
  });

  it("Herdr task client가 보고한 pane의 native frame에서 입력 줄과 커서를 읽는다", async () => {
    mocks.task.mockResolvedValue({ ...task, sessionType: SessionType.HERDR });
    mocks.herdrFocus.mockReturnValue("w1:p2");
    mocks.herdrPanes.mockResolvedValue([{ id: "w1:p1", tabId: "w1:t1" }, { id: "w1:p2", tabId: "w1:t2" }]);
    mocks.herdr.mockResolvedValue({ process_info: { pane_id: "w1:p2", shell_pid: 300, foreground_processes: [{ pid: 300 }] } });
    mocks.herdrFrame.mockResolvedValue({ output: "\x1b[2J\x1b[HOpenAI Codex\r\nModel: native\r\n› \x1b[3;3H\x1b[?25h", width: 80, height: 24 });
    expect(await readPaneChatSurface("task")).toMatchObject({ paneId: "w1:p2", rootPid: 300, inputLine: "› ", cursorColumn: 2, screen: "OpenAI Codex\nModel: native\n›" });
    expect(mocks.herdrFrame).toHaveBeenCalledWith("task", "w1:p2");
  });
  it("Herdr desktop client의 정확한 focus가 없으면 서버 global focus로 대신하지 않는다", async () => {
    mocks.task.mockResolvedValue({ ...task, sessionType: SessionType.HERDR });
    mocks.herdrPanes.mockResolvedValue([{ id: "w1:p1", tabId: "w1:t1" }]);
    mocks.herdrFocus.mockReturnValue(null);
    expect(await readPaneChatSurface("task")).toBeNull();
    expect(mocks.herdrFrame).not.toHaveBeenCalled();
  });

});
