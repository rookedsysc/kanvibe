import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({ surface: vi.fn(), layout: vi.fn(), exec: vi.fn(), write: vi.fn(), binding: vi.fn(), claude: vi.fn(), codex: vi.fn(), focusHerdr: vi.fn(), tabs: vi.fn() }));
vi.mock("../paneChatSurface", () => ({ readPaneChatSurface: mocks.surface, readPaneChatLayout: mocks.layout }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.exec }));
vi.mock("@/lib/terminal", () => ({ focusHerdrTaskPane: mocks.focusHerdr, listLocalTerminalTabs: mocks.tabs }));
vi.mock("../mobileBridgeService", () => ({ writeToPane: mocks.write }));
vi.mock("@/lib/aiSessions/paneChatSessionBindings", () => ({ findPaneChatSessionBinding: mocks.binding }));
vi.mock("@/lib/aiSessions/readClaudeSessions", () => ({ readClaudeSessionDetail: mocks.claude }));
vi.mock("@/lib/aiSessions/readCodexSessions", () => ({ readCodexSessionDetail: mocks.codex }));
import { focusPaneChatTerminal, getPaneChatLayout, getPaneChatSnapshot, sendPaneChatPrompt, interruptPaneChat, openPaneChatModelPicker, sendPaneChatKey } from "../paneChatService";

const processTable = (start = "Thu Nov 12 19:00:00 2026") => `100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh\n200 100 200 200 ttys010 ${start} /usr/bin/python3 python3 /tmp/codex\n300 100 300 200 ttys010 Thu Nov 12 18:00:00 2026 /tmp/claude /tmp/claude`;
const surface = { task: { id: "task-chat", sessionType: SessionType.TMUX, sessionName: "kv-task", sshHost: null, worktreePath: "/repo", project: { repoPath: "/repo" } }, paneId: "%3", rootPid: 100, screen: "CLI screen", inputLine: "› ", cursorColumn: 2 };

beforeEach(() => {
  vi.clearAllMocks();
  mocks.surface.mockResolvedValue(surface);
  mocks.exec.mockImplementation(async (command: string) => { if (command.startsWith("ps ")) return processTable(); throw new Error("No open transcript"); });
  mocks.binding.mockResolvedValue(null);
  mocks.write.mockResolvedValue(undefined);
});

describe("pane chat exact CLI control", () => {
  it("restores the requested tmux pane before returning to raw terminal input", async () => {
    mocks.exec.mockResolvedValue("");
    expect(await focusPaneChatTerminal("task-chat", "%3")).toEqual({ success: true });
    expect(mocks.surface).toHaveBeenCalledWith("task-chat", "%3", false);
    expect(mocks.exec).toHaveBeenCalledWith("tmux select-pane -t '%3'", null, expect.anything());
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("never focuses another pane when a return target is invalid or missing", async () => {
    await expect(focusPaneChatTerminal("task-chat", "")).rejects.toMatchObject({ name: "PaneChatInputError" });
    mocks.surface.mockResolvedValue(null);
    await expect(focusPaneChatTerminal("task-chat", "%foreign")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.exec).not.toHaveBeenCalled();
    expect(mocks.focusHerdr).not.toHaveBeenCalled();
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("targets the selected Zellij session and pane on return", async () => {
    mocks.surface.mockResolvedValue({ ...surface, paneId: "terminal_7", task: { ...surface.task, sessionType: SessionType.ZELLIJ, sessionName: "task's zellij" } });
    mocks.exec.mockResolvedValue("");
    await focusPaneChatTerminal("task-chat", "terminal_7");
    expect(mocks.exec).toHaveBeenCalledWith("zellij --session 'task'\"'\"'s zellij' action focus-pane-id 'terminal_7'", null, expect.anything());
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("restores Herdr through the task client and rejects a stale plain tab", async () => {
    mocks.surface.mockResolvedValue({ ...surface, paneId: "w1:p2", task: { ...surface.task, sessionType: SessionType.HERDR } });
    await focusPaneChatTerminal("task-chat", "w1:p2");
    expect(mocks.focusHerdr).toHaveBeenCalledWith("task-chat", "w1:p2");
    mocks.surface.mockResolvedValue({ ...surface, paneId: "term-a", task: { ...surface.task, sessionType: SessionType.TERMINAL } });
    mocks.tabs.mockReturnValue([{ id: "term-a", isActive: true }]);
    await focusPaneChatTerminal("task-chat", "term-a");
    mocks.tabs.mockReturnValue([{ id: "term-a", isActive: false }, { id: "term-b", isActive: true }]);
    await expect(focusPaneChatTerminal("task-chat", "term-a")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.exec).not.toHaveBeenCalled();
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("rejects invalid layout requests and hides unavailable runtime geometry", async () => {
    await expect(getPaneChatLayout("")).rejects.toMatchObject({ name: "PaneChatInputError" });
    await expect(getPaneChatLayout("task-chat", "")).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.layout).not.toHaveBeenCalled();
    mocks.layout.mockRejectedValue(new Error("closed session"));
    expect(await getPaneChatLayout("task-chat")).toEqual({ activePaneId: null, panes: [] });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("preserves the selected terminal identity and cell geometry at the service boundary", async () => {
    mocks.layout.mockResolvedValue({ activePaneId: "%3", panes: [{ id: "%3", tabId: "@1", bounds: { left: 41, top: 1, width: 59, height: 24 } }] });
    expect(await getPaneChatLayout("task-chat", "term-1")).toEqual({ activePaneId: "%3", panes: [{ id: "%3", tabId: "@1", bounds: { left: 41, top: 1, width: 59, height: 24 } }] });
    expect(mocks.layout).toHaveBeenCalledWith("task-chat", "term-1");
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("keeps a pinned chat on its pane when the runtime focus moves to a sibling", async () => {
    const target = await getPaneChatSnapshot("task-chat", "%3");
    mocks.surface.mockImplementation(async (_taskId: string, requestedPaneId?: string) => requestedPaneId === "%3" ? surface : { ...surface, paneId: "%9" });
    await interruptPaneChat("task-chat", target.targetToken!);
    expect(mocks.surface).toHaveBeenLastCalledWith("task-chat", "%3", false);
    expect(mocks.write.mock.calls).toEqual([["task-chat", "%3", "\x03"]]);
  });

  it("keeps a stable token for the foreground CLI while transcript mapping is unavailable", async () => {
    const first = await getPaneChatSnapshot("task-chat");
    const next = await getPaneChatSnapshot("task-chat");
    expect(first).toMatchObject({ canSend: true, paneId: "%3", provider: "codex", screen: "CLI screen", transcript: { status: "unavailable", messages: [] } });
    expect(first.targetToken).toBe(next.targetToken);
  });

  it("never substitutes recent history for a shell or background provider", async () => {
    mocks.exec.mockResolvedValue("100 1 100 100 ttys010 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh\n200 100 200 100 ttys010 Thu Nov 12 19:00:00 2026 /tmp/codex /tmp/codex");
    expect(await getPaneChatSnapshot("task-chat")).toMatchObject({ canSend: false, targetToken: null, transcript: { status: "unavailable", messages: [] } });
    expect(mocks.codex).not.toHaveBeenCalled();
  });

  it("pastes multiline text and submits a separate CR to the same fenced process", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    await sendPaneChatPrompt("task-chat", target.targetToken!, "First line\nSecond line");
    expect(mocks.write.mock.calls).toEqual([["task-chat", "%3", "\x1b[200~First line\nSecond line\x1b[201~"], ["task-chat", "%3", "\r"]]);
  });

  it("rejects a reused PID before any write", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    mocks.exec.mockResolvedValue(processTable("Thu Nov 12 19:01:00 2026"));
    await expect(sendPaneChatPrompt("task-chat", target.targetToken!, "Hello")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("does not submit CR if the CLI exits after paste", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    mocks.write.mockImplementationOnce(async () => { mocks.exec.mockResolvedValue(processTable("Thu Nov 12 19:02:00 2026")); });
    await expect(sendPaneChatPrompt("task-chat", target.targetToken!, "Hello")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.write).toHaveBeenCalledTimes(1);
  });

  it("rejects control-sequence injection and cross-task token reuse", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    await expect(sendPaneChatPrompt("task-chat", target.targetToken!, "Hi\x1b[201~\rcommand")).rejects.toMatchObject({ name: "PaneChatInputError" });
    await expect(interruptPaneChat("different-task", target.targetToken!)).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("opens the real Codex picker by typing its command, and permits only known navigation keys", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    await openPaneChatModelPicker("task-chat", target.targetToken!);
    expect(mocks.write.mock.calls.map((call) => call[2])).toEqual([..."/model", "\r"]);
    await sendPaneChatKey("task-chat", target.targetToken!, "down");
    expect(mocks.write).toHaveBeenLastCalledWith("task-chat", "%3", "\x1b[B");
    await expect(sendPaneChatKey("task-chat", target.targetToken!, "command" as "down")).rejects.toMatchObject({ name: "PaneChatInputError" });
  });

  it("reads only the hook-bound exact session and retains newest-first order", async () => {
    mocks.binding.mockResolvedValue({ sessionId: "exact-session", sourceRef: "/allowed/exact.jsonl" });
    const messages = [{ role: "assistant", text: "New", fullText: "New", timestamp: null, isTruncated: false }, { role: "user", text: "Old", fullText: "Old", timestamp: null, isTruncated: false }];
    mocks.codex.mockResolvedValue({ sessionId: "exact-session", provider: "codex", messages, nextCursor: null });
    expect(await getPaneChatSnapshot("task-chat")).toMatchObject({ transcript: { status: "available", sessionId: "exact-session", messages } });
    expect(mocks.codex).toHaveBeenCalledWith(expect.objectContaining({ worktreePath: "/repo" }), "exact-session", "/allowed/exact.jsonl", null, 100);
  });

  it("keeps a terminal draft intact and refuses prompt/model commands while the cursor is occupied", async () => {
    mocks.surface.mockResolvedValue({ ...surface, inputLine: "› unfinished draft", cursorColumn: 18 });
    const target = await getPaneChatSnapshot("task-chat");
    expect(target).toMatchObject({ canSend: true, inputState: "occupied" });
    await expect(sendPaneChatPrompt("task-chat", target.targetToken!, "Next prompt")).rejects.toMatchObject({ name: "PaneChatInputError" });
    await expect(openPaneChatModelPicker("task-chat", target.targetToken!)).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
    await interruptPaneChat("task-chat", target.targetToken!);
    expect(mocks.write).toHaveBeenLastCalledWith("task-chat", "%3", "\x03");
  });

  it("disables ambiguous foreground provider families even when one is a child", async () => {
    mocks.exec.mockResolvedValue(`${processTable()}\n201 200 200 200 ttys010 Thu Nov 12 19:00:00 2026 claude claude`);
    expect(await getPaneChatSnapshot("task-chat")).toMatchObject({ canSend: false, targetToken: null });
  });

});
