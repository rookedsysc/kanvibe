// @vitest-environment node
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({ openPty: vi.fn(), listPtySessions: vi.fn(async () => [] as { key: string; sessionId: string; pid: number }[]), closePtySession: vi.fn(async () => {}), spawn: vi.fn(), execGit: vi.fn(async () => "") }));
vi.mock("@/lib/remoteHostConnection", () => ({ getRemoteHostConnection: async () => ({ openPty: mocks.openPty, listPtySessions: mocks.listPtySessions, closePtySession: mocks.closePtySession }) }));
vi.mock("node-pty", () => ({ spawn: mocks.spawn }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.execGit }));

function socket() {
  const handlers = new Map<string, (data?: unknown) => void>();
  return { readyState: 1, OPEN: 1, bufferedAmount: 0, send: vi.fn(), close: vi.fn(), on: vi.fn((event: string, handler: (data?: unknown) => void) => handlers.set(event, handler)), trigger: (event: string, data?: unknown) => handlers.get(event)?.(data) };
}
function pty() {
  let exit: (event: { exitCode: number; detached?: boolean }) => void = () => {};
  let output: (data: string) => void = () => {};
  return { sessionId: "daemon-issued", pid: 4321, process: "remote-shell", cols: 80, rows: 24,
    write: vi.fn(), resize: vi.fn(), kill: vi.fn(), closeSession: vi.fn(),
    onData: vi.fn((handler) => { output = handler; return { dispose() {} }; }),
    onExit: vi.fn((handler) => { exit = handler; return { dispose() {} }; }),
    output: (data: string) => output(data), exit: (event: { exitCode: number; detached?: boolean }) => exit(event),
  };
}
beforeEach(() => { vi.resetModules(); vi.clearAllMocks(); mocks.listPtySessions.mockResolvedValue([]); });
const ssh = { host: "fixture", hostname: "fixture", port: 22, username: "fixture", privateKeyPath: "/unused" };

describe("ordinary remote terminals consume daemon PTYs", () => {
  it("keeps an active tab when the daemon close acknowledgement fails", async () => {
    const terminal = await import("../terminal"), remote = pty(); mocks.openPty.mockResolvedValue(remote);
    const tabId = terminal.listLocalTerminalTabs("task")[0].id;
    await terminal.attachRemoteSession("task", tabId, "fixture", SessionType.TERMINAL, "shell", socket() as never, ssh);
    mocks.closePtySession.mockRejectedValueOnce(new Error("transport write failed"));
    await expect(terminal.closeRemoteTerminalTab("task", tabId, "fixture", "shell")).rejects.toThrow("transport write failed");
    expect(terminal.listLocalTerminalTabs("task").map(tab => tab.id)).toContain(tabId);
    expect(remote.kill).not.toHaveBeenCalled(); terminal.killAllTerminalSessions();
  });
  it("restores multiple daemon-owned tabs after app detach and closes an unattached tab by session ID", async () => {
    const terminal = await import("../terminal");
    mocks.listPtySessions.mockResolvedValue([
      { key: JSON.stringify(["task#task-1", SessionType.TERMINAL, "shell"]), sessionId: "first", pid: 10 },
      { key: JSON.stringify(["task#task-3", SessionType.TERMINAL, "shell"]), sessionId: "third", pid: 30 },
      { key: JSON.stringify(["other#other-1", SessionType.TERMINAL, "shell"]), sessionId: "other", pid: 50 },
    ]);
    expect((await terminal.listRemoteTerminalTabs("task", "fixture", "shell")).map(tab => tab.id)).toEqual(["task-1", "task-3"]);
    expect(terminal.createLocalTerminalTab("task").id).toBe("task-4");
    await terminal.closeRemoteTerminalTab("task", "task-3", "fixture", "shell");
    expect(mocks.closePtySession).toHaveBeenCalledWith("third");
    expect(terminal.listLocalTerminalTabs("task").map(tab => tab.id)).toEqual(["task-1", "task-4"]);
    terminal.killAllTerminalSessions();
    expect((await terminal.listRemoteTerminalTabs("task", "fixture", "shell")).map(tab => tab.id)).toEqual(["task-1", "task-3"]);
  });
  it("shares one daemon attachment, keeps it on window/app detach and closes only explicitly", async () => {
    const terminal = await import("../terminal");
    const remote = pty(); mocks.openPty.mockResolvedValue(remote);
    const tab = terminal.listLocalTerminalTabs("task")[0].id;
    const a = socket(), b = socket();
    await Promise.all([a,b].map(ws => terminal.attachRemoteSession("task", tab, "fixture", SessionType.TERMINAL, "shell", ws as never, ssh, 80, 24, "/work")));
    expect(mocks.openPty).toHaveBeenCalledTimes(1); expect(mocks.spawn).not.toHaveBeenCalled();
    const key = mocks.openPty.mock.calls[0][0].key;
    a.trigger("close"); b.trigger("close"); expect(remote.kill).not.toHaveBeenCalled();
    terminal.killAllTerminalSessions(); expect(remote.kill).toHaveBeenCalledOnce(); expect(remote.closeSession).not.toHaveBeenCalled();
    const again = pty(); mocks.openPty.mockResolvedValue(again);
    await terminal.attachRemoteSession("task", tab, "fixture", SessionType.TERMINAL, "shell", socket() as never, ssh);
    expect(mocks.openPty.mock.calls[1][0].key).toBe(key);
    terminal.closeLocalTerminalTab("task", tab); expect(again.closeSession).toHaveBeenCalledOnce();
  });

  it("preserves the tab on transport loss and applies bounded output to a stalled WebSocket", async () => {
    const terminal = await import("../terminal"), remote = pty(); mocks.openPty.mockResolvedValue(remote);
    const tab = terminal.listLocalTerminalTabs("task")[0].id, ws = socket();
    await terminal.attachRemoteSession("task", tab, "fixture", SessionType.TERMINAL, "shell", ws as never, ssh);
    ws.bufferedAmount = 8 * 1024 * 1024 + 1; remote.output("output");
    expect(ws.close).toHaveBeenCalledWith(1013, expect.stringContaining("backpressure"));
    remote.exit({ exitCode: 255, detached: true });
    expect(terminal.listLocalTerminalTabs("task").map(tab => tab.id)).toContain(tab);
    expect(remote.closeSession).not.toHaveBeenCalled();
  });

  it.each([SessionType.TMUX, SessionType.ZELLIJ])("runs existing %s attach commands inside the daemon, without a new SSH PTY", async (type) => {
    const terminal = await import("../terminal"), remote = pty(); mocks.openPty.mockResolvedValue(remote);
    const ws = socket(); await terminal.attachRemoteSession("task", null, "fixture", type, "existing-session", ws as never, ssh, 80, 24, "/work");
    const request = mocks.openPty.mock.calls[0][0];
    expect(request.command).toContain(type === SessionType.TMUX ? "tmux attach-session" : "zellij attach");
    expect(request.command).toContain("existing-session"); expect(mocks.spawn).not.toHaveBeenCalled();
    ws.trigger("close"); expect(remote.kill).toHaveBeenCalledOnce(); expect(remote.closeSession).not.toHaveBeenCalled();
  });
});
