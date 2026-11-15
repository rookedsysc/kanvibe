import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({ surface: vi.fn(), layout: vi.fn(), exec: vi.fn(), write: vi.fn(), binding: vi.fn(), claude: vi.fn(), codex: vi.fn(), rollout: vi.fn(), emptyThread: vi.fn(), focusHerdr: vi.fn(), tabs: vi.fn(), catalog: vi.fn() }));
const modelMocks = vi.hoisted(() => ({ tail: vi.fn(), readTextFile: vi.fn() }));
vi.mock("../paneChatSurface", () => ({ readPaneChatSurface: mocks.surface, readPaneChatLayout: mocks.layout }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.exec }));
vi.mock("@/lib/terminal", () => ({ focusHerdrTaskPane: mocks.focusHerdr, listLocalTerminalTabs: mocks.tabs }));
vi.mock("../mobileBridgeService", () => ({ writeToPane: mocks.write }));
vi.mock("../paneChatComposerCatalog", () => ({ readPaneChatSuggestions: mocks.catalog }));
vi.mock("@/lib/aiSessions/paneChatSessionBindings", () => ({ findPaneChatSessionBinding: mocks.binding }));
vi.mock("@/lib/aiSessions/readClaudeSessions", () => ({ readClaudeSessionDetail: mocks.claude }));
vi.mock("@/lib/aiSessions/readCodexSessions", () => ({ readCodexSessionDetail: mocks.codex, findCodexMainThreadRollout: mocks.rollout }));
vi.mock("@/lib/aiSessions/codexThreadMetadata", () => ({ readCodexEmptyThread: mocks.emptyThread }));
vi.mock("@/lib/aiSessions/shared", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/aiSessions/shared")>(), readJsonLinesTail: modelMocks.tail }));
vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/hostFileAccess")>(), readTextFile: modelMocks.readTextFile }));
import { focusPaneChatTerminal, getPaneChatLayout, getPaneChatSnapshot, getPaneChatSuggestions, stagePaneChatAttachment, removePaneChatAttachment, sendPaneChatPrompt, interruptPaneChat, openPaneChatModelPicker, changePaneChatModelSetting, sendPaneChatKey } from "../paneChatService";

const processTable = (start = "Thu Nov 12 19:00:00 2026") => `100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh\n200 100 200 200 ttys010 ${start} /usr/bin/python3 python3 /tmp/codex\n300 100 300 200 ttys010 Thu Nov 12 18:00:00 2026 /tmp/claude /tmp/claude`;
const surface = { task: { id: "task-chat", sessionType: SessionType.TMUX, sessionName: "kv-task", sshHost: null, worktreePath: "/repo", project: { repoPath: "/repo" } }, paneId: "%3", rootPid: 100, screen: "CLI screen", inputLine: "› ", cursorColumn: 2 };

beforeEach(() => {
  vi.clearAllMocks();
  mocks.emptyThread.mockResolvedValue(null);
  mocks.surface.mockResolvedValue(surface);
  mocks.exec.mockImplementation(async (command: string) => { if (command.startsWith("ps ")) return processTable(); throw new Error("No open transcript"); });
  mocks.binding.mockResolvedValue({ sessionId: "exact-session", sourceRef: "/allowed/exact.jsonl" });
  mocks.codex.mockResolvedValue({ sessionId: "exact-session", provider: "codex", messages: [], nextCursor: null });
  mocks.claude.mockResolvedValue({ sessionId: "exact-session", provider: "claude", messages: [], nextCursor: null });
  mocks.write.mockResolvedValue(undefined);
  modelMocks.tail.mockRejectedValue(new Error("No transcript tail"));
  modelMocks.readTextFile.mockResolvedValue(JSON.stringify({ models: [
    { slug: "gpt-sol", display_name: "GPT Sol", visibility: "list", priority: 1, supported_reasoning_levels: [{ effort: "low" }, { effort: "high" }], service_tiers: [{ id: "priority" }] },
    { slug: "gpt-luna", display_name: "GPT Luna", visibility: "list", priority: 2, supported_reasoning_levels: [], service_tiers: [] },
  ] }));
});

/** hook이 묶어 준 Codex 기록에서 현재 모델 설정을 읽을 수 있는 상태로 만든다 */
function bindCodexSession(model = "gpt-sol") {
  mocks.binding.mockResolvedValue({ sessionId: "exact-session", sourceRef: "/allowed/exact.jsonl" });
  mocks.codex.mockResolvedValue({ sessionId: "exact-session", provider: "codex", messages: [], nextCursor: null });
  modelMocks.tail.mockResolvedValue([{ type: "event_msg", payload: { type: "thread_settings_applied", thread_settings: { model, reasoning_effort: "low", service_tier: null } } }]);
}

describe("pane chat exact CLI control", () => {
  it("pins catalog results and rejects a CLI change while suggestions are loading", async () => {
    const target = await getPaneChatSnapshot("task-chat", "%3");
    mocks.catalog.mockResolvedValue({ items: [{ id: "skill:review", label: "review", description: "Review", insertText: "$review" }], truncated: false });
    expect(await getPaneChatSuggestions("task-chat", target.targetToken!, "skill", "review")).toMatchObject({ targetToken: target.targetToken, items: [{ insertText: "$review" }] });
    mocks.catalog.mockImplementationOnce(async () => { mocks.exec.mockResolvedValue(processTable("Thu Nov 12 20:00:00 2026")); return { items: [], truncated: false }; });
    await expect(getPaneChatSuggestions("task-chat", target.targetToken!, "file", "readme")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("permits the same request to retry after a definite pre-write refusal", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    mocks.surface.mockResolvedValue({ ...surface, inputLine: "› native text", cursorColumn: 13 });
    await expect(sendPaneChatPrompt("task-chat", target.targetToken!, "retry", [], "pre-write-retry")).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
    mocks.surface.mockResolvedValue(surface);
    await sendPaneChatPrompt("task-chat", target.targetToken!, "retry", [], "pre-write-retry");
    expect(mocks.write).toHaveBeenCalledTimes(2);
  });

  it("stages an actual image and pastes image-only input as a native attachment", async () => {
    const target = await getPaneChatSnapshot("task-chat", "%3");
    const png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aOK0AAAAASUVORK5CYII=";
    const attachment = await stagePaneChatAttachment("task-chat", target.targetToken!, { name: "image.png", mimeType: "image/png", dataUrl: `data:image/png;base64,${png}` });
    expect(attachment).toMatchObject({ kind: "image", mimeType: "image/png", sizeBytes: 68 });
    await sendPaneChatPrompt("task-chat", target.targetToken!, "", [attachment.id], "image-only");
    expect(mocks.write.mock.calls).toHaveLength(2);
    expect(mocks.write.mock.calls[0]).toEqual(["task-chat", "%3", expect.stringMatching(/^\x1b\[200~\/.+\.png\x1b\[201~$/)]);
    expect(mocks.write.mock.calls[1]).toEqual(["task-chat", "%3", "\r"]);
    await removePaneChatAttachment("task-chat", target.targetToken!, attachment.id);
  });

  it("rejects an attachment from another live CLI before writing any bytes", async () => {
    const first = await getPaneChatSnapshot("task-chat", "%3");
    const attachment = await stagePaneChatAttachment("task-chat", first.targetToken!, { name: "notes.txt", mimeType: "text/plain", dataUrl: "data:text/plain;base64,aGVsbG8=" });
    const second = await getPaneChatSnapshot("task-chat");
    await expect(sendPaneChatPrompt("task-chat", second.targetToken!, "read", [attachment.id])).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
    await removePaneChatAttachment("task-chat", first.targetToken!, attachment.id);
  });

  it("replays one request receipt and rejects the same id with another payload", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    await sendPaneChatPrompt("task-chat", target.targetToken!, "once", [], "retry-one");
    await sendPaneChatPrompt("task-chat", target.targetToken!, "once", [], "retry-one");
    expect(mocks.write).toHaveBeenCalledTimes(2);
    await expect(sendPaneChatPrompt("task-chat", target.targetToken!, "different", [], "retry-one")).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).toHaveBeenCalledTimes(2);
  });

  it("does not repeat a possibly accepted paste after an interrupted delivery", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    mocks.write.mockRejectedValueOnce(new Error("socket closed after write"));
    await expect(sendPaneChatPrompt("task-chat", target.targetToken!, "uncertain", [], "uncertain-one")).rejects.toThrow();
    await expect(sendPaneChatPrompt("task-chat", target.targetToken!, "uncertain", [], "uncertain-one")).rejects.toThrow();
    expect(mocks.write).toHaveBeenCalledTimes(1);
  });

  it("closes the native Codex skill-completion token before its one submit Enter", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    await sendPaneChatPrompt("task-chat", target.targetToken!, "Use $qa-skill");
    expect(mocks.write.mock.calls).toEqual([["task-chat", "%3", "\x1b[200~Use $qa-skill \x1b[201~"], ["task-chat", "%3", "\r"]]);
  });

  it.each(["", "Read this file"])("closes a staged Claude file reference before one submit Enter (caption=%s)", async (text) => {
    mocks.exec.mockResolvedValue(processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude"));
    mocks.surface.mockResolvedValue({ ...surface, inputLine: "❯ " });
    const target = await getPaneChatSnapshot("task-chat", "%3");
    const attachment = await stagePaneChatAttachment("task-chat", target.targetToken!, { name: "file with spaces.txt", mimeType: "text/plain", dataUrl: "data:text/plain;base64,aGVsbG8=" });
    await sendPaneChatPrompt("task-chat", target.targetToken!, text, [attachment.id]);
    expect(mocks.write.mock.calls).toHaveLength(2);
    const paste = mocks.write.mock.calls[0][2] as string;
    const mention = paste.slice("\x1b[200~".length + (text ? text.length + 2 : 0), -" \x1b[201~".length);
    expect(mention).toMatch(/^@(?:"[^\n]+\.txt"|\/[^\n]+\.txt)$/);
    expect(mocks.write.mock.calls).toEqual([["task-chat", "%3", `\x1b[200~${text ? `${text}\n\n` : ""}${mention} \x1b[201~`], ["task-chat", "%3", "\r"]]);
    await removePaneChatAttachment("task-chat", target.targetToken!, attachment.id);
  });

  it.each([
    ["Read @/repo/file.txt", "Read @/repo/file.txt "],
    ['Read @"/repo/file with spaces.txt"', 'Read @"/repo/file with spaces.txt" '],
    ['Read @"/repo/file \\"quoted\\".txt"', 'Read @"/repo/file \\"quoted\\".txt" '],
    ["Read @/repo/file.txt ", "Read @/repo/file.txt "],
    ["Read @/repo/file.txt please", "Read @/repo/file.txt please"],
    ["Contact person@example.com", "Contact person@example.com"],
  ])("preserves Claude text while terminating only a final file mention (%s)", async (text, expected) => {
    mocks.exec.mockResolvedValue(processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude"));
    mocks.surface.mockResolvedValue({ ...surface, inputLine: "❯ " });
    const target = await getPaneChatSnapshot("task-chat", "%3");
    await sendPaneChatPrompt("task-chat", target.targetToken!, text);
    expect(mocks.write.mock.calls).toEqual([["task-chat", "%3", `\x1b[200~${expected}\x1b[201~`], ["task-chat", "%3", "\r"]]);
  });

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

  it("keeps the exact CLI visible but refuses input while conversation mapping is unavailable", async () => {
    mocks.binding.mockResolvedValue(null);
    const first = await getPaneChatSnapshot("task-chat");
    const next = await getPaneChatSnapshot("task-chat");
    expect(first).toMatchObject({ canSend: false, paneId: "%3", provider: "codex", screen: "CLI screen", transcript: { status: "unavailable", messages: [] } });
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

  it("reports the current model settings and selectable options from the exact session", async () => {
    bindCodexSession();
    expect((await getPaneChatSnapshot("task-chat")).modelSettings).toEqual({
      model: "gpt-sol", effort: "low", isFast: false,
      models: [
        { id: "gpt-sol", label: "GPT Sol", effortLevels: [{ id: "low", label: "Light" }, { id: "high", label: "High" }], supportsFast: true },
        { id: "gpt-luna", label: "GPT Luna", effortLevels: [], supportsFast: false },
      ],
    });
    expect(modelMocks.tail).toHaveBeenCalledWith("/allowed/exact.jsonl", 200, null);
  });

  it("omits model settings when the transcript tail cannot be read", async () => {
    mocks.binding.mockResolvedValue({ sessionId: "exact-session", sourceRef: "/allowed/exact.jsonl" });
    mocks.codex.mockResolvedValue({ sessionId: "exact-session", provider: "codex", messages: [], nextCursor: null });
    const snapshot = await getPaneChatSnapshot("task-chat");
    expect(snapshot.transcript.status).toBe("available");
    expect(snapshot).not.toHaveProperty("modelSettings");
  });

  it("changes a model setting by typing the slash command built from the selectable options", async () => {
    bindCodexSession();
    const target = await getPaneChatSnapshot("task-chat");
    await changePaneChatModelSetting("task-chat", target.targetToken!, { kind: "effort", effort: "high" });
    expect(mocks.write.mock.calls.map((call) => call[2])).toEqual([..."/model --reasoning=high", "\r"]);
    mocks.write.mockClear();
    await changePaneChatModelSetting("task-chat", target.targetToken!, { kind: "fast", isFast: true });
    expect(mocks.write.mock.calls.map((call) => call[2]).join("")).toBe("/model --speed=priority\r");
  });

  it.each([
    ["a value outside the selectable options", { kind: "model", model: "gpt-sol\rrm -rf /" }],
    ["an effort level the current model lacks", { kind: "effort", effort: "ultra" }],
    ["a malformed change", { kind: "effort" }],
    ["an unknown kind", { kind: "theme", value: "dark" }],
  ])("refuses %s without writing to the pane", async (_, change) => {
    bindCodexSession();
    const target = await getPaneChatSnapshot("task-chat");
    await expect(changePaneChatModelSetting("task-chat", target.targetToken!, change)).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("refuses a fast-mode toggle for a model without that option and while the CLI input is occupied", async () => {
    bindCodexSession("gpt-luna");
    const target = await getPaneChatSnapshot("task-chat");
    await expect(changePaneChatModelSetting("task-chat", target.targetToken!, { kind: "fast", isFast: true })).rejects.toMatchObject({ name: "PaneChatInputError" });
    bindCodexSession();
    mocks.surface.mockResolvedValue({ ...surface, inputLine: "› unfinished draft", cursorColumn: 18 });
    await expect(changePaneChatModelSetting("task-chat", target.targetToken!, { kind: "effort", effort: "high" })).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
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


describe("conversation lifetime fences", () => {
  it("retires a token when the same CLI changes conversation", async () => {
    bindCodexSession();
    const first = await getPaneChatSnapshot("task-chat", "%3");
    mocks.binding.mockResolvedValue({ sessionId: "session-b", sourceRef: "/allowed/b.jsonl" });
    mocks.codex.mockResolvedValue({ sessionId: "session-b", provider: "codex", messages: [], nextCursor: null });
    await expect(sendPaneChatPrompt("task-chat", first.targetToken!, "Must not cross sessions")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.write).not.toHaveBeenCalled();
    const next = await getPaneChatSnapshot("task-chat", "%3");
    expect(next.targetToken).not.toBe(first.targetToken);
    expect(next.transcript.sessionId).toBe("session-b");
  });
  it("refuses sending when exact conversation identity is unavailable", async () => {
    mocks.binding.mockResolvedValue(null);
    const target = await getPaneChatSnapshot("task-chat", "%3");
    expect(target.canSend).toBe(false);
    expect(target.targetToken).toBeNull();
    expect(target.transcript.status).toBe("unavailable");
  });
  it("discards transcript that completed after a same-process session change", async () => {
    bindCodexSession();
    mocks.codex.mockImplementationOnce(async () => {
      mocks.binding.mockResolvedValue({ sessionId: "session-b", sourceRef: "/allowed/b.jsonl" });
      return { sessionId: "exact-session", messages: [{ text: "Must not leak A" }], nextCursor: null };
    });
    const target = await getPaneChatSnapshot("task-chat", "%3");
    expect(target.transcript.messages).toEqual([]);
    expect(target.canSend).toBe(false);
  });
});


it("does not resurrect a retired token after A to B to A in the same process", async () => {
  bindCodexSession();
  const a = await getPaneChatSnapshot("task-chat", "%3");
  mocks.binding.mockResolvedValue({ sessionId: "session-b", sourceRef: "/allowed/b.jsonl" });
  mocks.codex.mockResolvedValue({ sessionId: "session-b", provider: "codex", messages: [], nextCursor: null });
  const b = await getPaneChatSnapshot("task-chat", "%3");
  bindCodexSession();
  const returned = await getPaneChatSnapshot("task-chat", "%3");
  expect(returned.targetToken).not.toBe(a.targetToken);
  expect(returned.targetToken).not.toBe(b.targetToken);
  expect(returned.conversationKey).toBe(a.conversationKey);
  expect(b.conversationKey).not.toBe(a.conversationKey);
  await expect(sendPaneChatPrompt("task-chat", a.targetToken!, "Stale A")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
  expect(mocks.write).not.toHaveBeenCalled();
});

it.each(["host", "close"])("rejects old input after %s changes", async (change) => {
  const a = await getPaneChatSnapshot("task-chat", "%3");
  mocks.surface.mockResolvedValue(change === "close" ? null : { ...surface, task: { ...surface.task, sshHost: "another-host" } });
  await expect(sendPaneChatPrompt("task-chat", a.targetToken!, "Do not deliver")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
  expect(mocks.write).not.toHaveBeenCalled();
});


it("does not share a conversation draft key across hosts with the same session id", async () => {
  const local = await getPaneChatSnapshot("task-chat", "%3");
  mocks.surface.mockResolvedValue({ ...surface, task: { ...surface.task, sshHost: "another-host" } });
  const remote = await getPaneChatSnapshot("task-chat", "%3");
  expect(local.conversationKey).toBeTruthy();
  expect(remote.conversationKey).toBeTruthy();
  expect(remote.conversationKey).not.toBe(local.conversationKey);
});


const FOOTER_COMMAND = "/statusline thread-id";
const THREAD_X = "0190aaaa-0000-7000-8000-00000000000a";
const THREAD_Y = "0190bbbb-0000-7000-8000-00000000000b";
const THREAD_Z = "0190cccc-0000-7000-8000-00000000000c";
const rolloutFile = (threadId: string) => `/home/.codex/sessions/2026/11/14/rollout-2026-11-14T10-00-00-${threadId}.jsonl`;
const codexProcessTable = [
  "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh",
  `200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 codex codex resume ${THREAD_X} --yolo`,
  "110 1 110 210 ttys011 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh",
  "210 110 210 210 ttys011 Thu Nov 12 19:00:00 2026 codex codex --yolo",
].join("\n");

/** 실제 Codex TUI가 입력란과 footer를 그리고 입력을 받는 방식만 흉내 내는 터미널 경계 대역 */
class FakeCodexPane {
  composer = "";
  readonly threadsShowingId = new Set<string>();
  readonly submissions: { threadId: string; text: string }[] = [];
  readonly commands: string[] = [];
  ignoresFooterCommand = false;
  /** 사용자가 여러 줄 입력의 첫 줄로 커서를 올려 둔 상태 */
  isCursorOnFirstComposerRow = false;
  private stalledInput: string[] | null = null;
  private stallsOnNextWrite = false;

  constructor(readonly paneId: string, readonly rootPid: number, public threadId: string) {}

  /** `/new`나 `/resume`처럼 같은 process 안에서 입력을 받는 대화가 바뀐다. footer 설정은 새 대화로 따라가지 않는다 */
  switchThread(threadId: string) {
    this.threadId = threadId;
    this.composer = "";
  }

  /** 대화 전환이 늦어져 화면은 그대로이고 그 뒤에 쓴 입력은 전환이 끝나야 처리되는 상태 */
  stallInputFromNextWrite() {
    this.stallsOnNextWrite = true;
  }

  completeTransition(threadId: string) {
    const queuedInput = this.stalledInput ?? [];
    this.stalledInput = null;
    this.switchThread(threadId);
    queuedInput.forEach((bytes) => this.apply(bytes));
  }

  write(bytes: string) {
    if (this.stallsOnNextWrite) {
      this.stallsOnNextWrite = false;
      this.stalledInput = [];
    }
    if (this.stalledInput) this.stalledInput.push(bytes);
    else this.apply(bytes);
  }

  private apply(bytes: string) {
    const pasted = bytes.match(/^\x1b\[200~([\s\S]*)\x1b\[201~$/)?.[1];
    if (pasted !== undefined) {
      // 실제 TUI는 이미지 경로를 순번 자리표시로 바꾸고, 긴 글은 글자 수만 적힌 자리표시로 접는다.
      if (/\.png$/.test(pasted)) this.composer += `[Image #${(this.composer.match(/\[Image #\d+\]/g)?.length ?? 0) + 1}] `;
      else this.composer += pasted.length > 1000 ? `[Pasted Content ${pasted.length} chars]` : pasted;
      return;
    }
    if (bytes !== "\r") { this.composer += bytes; return; }
    const text = this.composer;
    this.composer = "";
    if (!text.startsWith("/")) { this.submissions.push({ threadId: this.threadId, text }); return; }
    this.commands.push(text);
    if (text.startsWith("/statusline ") && text.split(" ").includes("thread-id") && !this.ignoresFooterCommand) this.threadsShowingId.add(this.threadId);
  }

  surface(task: object) {
    const [firstLine, ...moreLines] = (this.composer || "Ask Codex to do anything").split("\n");
    // 명령 이름을 치는 동안에는 선택 표시가 붙은 자동완성 줄이 입력란 위에 뜬다.
    const commandPopup = /^\/\S*$/.test(this.composer) ? ["› /model  choose what model and reasoning effort to use", ""] : [];
    const rows = ["  >_ OpenAI Codex (v0.169.0)", "", ...commandPopup, `› ${firstLine}`, ...moreLines.map((line) => line ? `  ${line}` : "")];
    const cursorRow = this.isCursorOnFirstComposerRow ? rows.length - 1 - moreLines.length : rows.length - 1;
    const footer = this.threadsShowingId.has(this.threadId) ? `  ${this.threadId}` : "  gpt-sol high · ~/repo";
    rows.push("", footer, ...(this.composer ? [] : ["  ← for agents · ? for shortcuts"]));
    return { observation: ++codexObservationSequence, task, paneId: this.paneId, rootPid: this.rootPid, screen: rows.join("\n"), inputLine: rows[cursorRow], cursorColumn: this.composer ? rows[cursorRow].length : 2, viewportLines: rows, cursorRow, viewportColumns: 160, inputIsPlaceholder: !this.composer };
  }
}

let codexTaskSequence = 0;
let codexObservationSequence = 0;

/** 같은 cwd에서 공유 서버에 붙은 Codex TUI 두 개를 띄운 태스크. 실행 상태가 테스트끼리 섞이지 않게 태스크마다 새 ID를 쓴다 */
function createCodexTask(version = "codex-cli 0.169.0") {
  const taskId = `task-codex-footer-${++codexTaskSequence}`;
  const task = { ...surface.task, id: taskId };
  const a = new FakeCodexPane("%A", 100, THREAD_X);
  const b = new FakeCodexPane("%B", 110, THREAD_Z);
  const panes: Record<string, FakeCodexPane> = { "%A": a, "%B": b };
  mocks.surface.mockImplementation(async (_taskId: string, paneId?: string) => panes[paneId ?? "%A"]?.surface(task) ?? null);
  mocks.write.mockImplementation(async (_taskId: string, paneId: string, bytes: string) => panes[paneId].write(bytes));
  mocks.exec.mockImplementation(async (command: string) => {
    if (command.startsWith("ps ")) return codexProcessTable;
    if (/^lsof -a -p \d+ -d txt -Fn$/.test(command)) return "p200\nftxt\nn/opt/codex/bin/codex\nftxt\nn/usr/lib/dyld";
    if (command === "'/opt/codex/bin/codex' --version") return version;
    throw new Error("No open transcript");
  });
  // 공유 app-server의 hook은 pane을 증명하지 못해 연결을 남기지 않는다.
  mocks.binding.mockResolvedValue(null);
  mocks.rollout.mockImplementation(async (_context: unknown, threadId: string) => rolloutFile(threadId));
  mocks.codex.mockImplementation(async (_context: unknown, sessionId: string) => ({ sessionId, provider: "codex", messages: [], nextCursor: null }));
  return { taskId, a, b };
}

/** 가짜 타이머를 흘려 보내 서비스의 글자 간격과 화면 확인 대기를 끝까지 진행시킨다 */
async function settle<T>(action: Promise<T>): Promise<T> {
  let isSettled = false;
  const observed = action.finally(() => { isSettled = true; });
  observed.catch(() => undefined);
  for (let step = 0; !isSettled; step++) {
    if (step > 2_000) throw new Error("pane chat action did not settle");
    await vi.advanceTimersByTimeAsync(50);
  }
  return observed;
}

describe("Codex displayed-thread footer binding", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("types the session-only thread-id footer command into an empty composer and binds the displayed thread", async () => {
    const { taskId, a } = createCodexTask();
    const snapshot = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(mocks.write.mock.calls.map(([, paneId, bytes]) => [paneId, bytes])).toEqual([...FOOTER_COMMAND, "\r"].map((character) => ["%A", character]));
    expect(a.commands).toEqual([FOOTER_COMMAND]);
    expect(snapshot).toMatchObject({ canSend: true, provider: "codex", inputState: "ready", reason: null, transcript: { status: "available", sessionId: THREAD_X } });
    expect(snapshot.targetToken).toBeTruthy();
    expect(mocks.rollout).toHaveBeenCalledWith(expect.objectContaining({ worktreePath: "/repo", sshHost: null }), THREAD_X);
  });

  it("types the footer command once when two snapshot reads race", async () => {
    const { taskId, a } = createCodexTask();
    await settle(Promise.all([getPaneChatSnapshot(taskId, "%A"), getPaneChatSnapshot(taskId, "%A")]));
    expect(a.commands).toEqual([FOOTER_COMMAND]);
    expect((await settle(getPaneChatSnapshot(taskId, "%A"))).transcript).toMatchObject({ status: "available", sessionId: THREAD_X });
    expect(a.commands).toEqual([FOOTER_COMMAND]);
  });

  it("discards an old snapshot whose scoped lookup finishes after a newer thread was bound", async () => {
    const { taskId, a } = createCodexTask();
    const original = await settle(getPaneChatSnapshot(taskId, "%A"));
    let release!: (value: string) => void;
    let started!: () => void;
    const lookupStarted = new Promise<void>((resolve) => { started = resolve; });
    mocks.rollout.mockImplementationOnce(() => { started(); return new Promise<string>((resolve) => { release = resolve; }); });
    const olderRead = getPaneChatSnapshot(taskId, "%A");
    await lookupStarted;
    a.switchThread(THREAD_Y);
    const current = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(current.transcript.sessionId).toBe(THREAD_Y);
    release(rolloutFile(THREAD_X));
    expect(await settle(olderRead)).toMatchObject({ canSend: false, reason: "target-changed", transcript: { sessionId: null } });
    await expect(settle(sendPaneChatPrompt(taskId, original.targetToken!, "Old"))).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    await settle(sendPaneChatPrompt(taskId, current.targetToken!, "Current Y"));
    expect(a.submissions).toEqual([{ threadId: THREAD_Y, text: "Current Y" }]);
  });

  it("checks a fresh frame after an asynchronous lookup before writing any input", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    let release!: (value: string) => void;
    let started!: () => void;
    const lookupStarted = new Promise<void>((resolve) => { started = resolve; });
    mocks.rollout.mockImplementation(async (_context: unknown, threadId: string) => {
      if (a.composer === "Must not reach Y") { started(); return new Promise<string>((resolve) => { release = resolve; }); }
      return rolloutFile(threadId);
    });
    mocks.write.mockClear();
    const sending = sendPaneChatPrompt(taskId, target.targetToken!, "Must not reach Y");
    const rejected = expect(settle(sending)).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    await lookupStarted;
    a.switchThread(THREAD_Y);
    a.threadsShowingId.add(THREAD_Y);
    release(rolloutFile(THREAD_X));
    await rejected;
    expect(mocks.write.mock.calls.map(([, , bytes]) => bytes)).toEqual(["\x1b[200~Must not reach Y\x1b[201~"]);
    expect(a.submissions).toEqual([]);
  });

  it("keeps an already-bound transcript while the native composer queues messages and refuses a new prompt", async () => {
    const { taskId, a } = createCodexTask();
    const bound = await settle(getPaneChatSnapshot(taskId, "%A"));
    mocks.surface.mockImplementation(async () => {
      const frame = a.surface({ ...surface.task, id: taskId });
      frame.viewportLines[frame.viewportLines.length - 1] = "  tab to queue message";
      return frame;
    });
    const busy = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(busy).toMatchObject({ canSend: true, inputState: "occupied", targetToken: bound.targetToken, transcript: { sessionId: THREAD_X } });
    mocks.write.mockClear();
    await expect(settle(sendPaneChatPrompt(taskId, busy.targetToken!, "Busy"))).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("never bootstraps the real side-view hint even when the composer is natively dim and empty", async () => {
    const { taskId, a } = createCodexTask();
    mocks.surface.mockImplementation(async () => {
      const frame = a.surface({ ...surface.task, id: taskId });
      frame.inputLine = "› Ask a follow-up question";
      frame.viewportLines[frame.cursorRow] = frame.inputLine;
      frame.viewportLines[frame.viewportLines.length - 1] = "  Side from main thread · ⌃/ to switch · ⌃c to close";
      return frame;
    });
    const side = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(side).toMatchObject({ canSend: false, targetToken: null, inputState: "occupied" });
    expect(mocks.write).not.toHaveBeenCalled();
    expect(mocks.rollout).not.toHaveBeenCalled();
  });

  it("bootstraps an unarmed main footer that wraps its default fields onto two rows", async () => {
    const { taskId, a } = createCodexTask();
    mocks.surface.mockImplementation(async () => {
      const frame = a.surface({ ...surface.task, id: taskId });
      if (!a.composer && !a.threadsShowingId.has(a.threadId)) frame.viewportLines.splice(frame.cursorRow + 3, 0, "  long-worktree-path-wrapped-from-default-footer");
      return frame;
    });
    const bound = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(bound).toMatchObject({ canSend: true, transcript: { sessionId: THREAD_X } });
    expect(a.commands).toEqual([FOOTER_COMMAND]);
  });

  it("re-arms after a same-process conversation change, never revives the earlier token, and leaves the sibling pane bound", async () => {
    const { taskId, a, b } = createCodexTask();
    const first = await settle(getPaneChatSnapshot(taskId, "%A"));
    const sibling = await settle(getPaneChatSnapshot(taskId, "%B"));
    expect(sibling.transcript.sessionId).toBe(THREAD_Z);

    a.switchThread(THREAD_Y);
    await expect(settle(sendPaneChatPrompt(taskId, first.targetToken!, "Stale X"))).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    const second = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(second.transcript).toMatchObject({ status: "available", sessionId: THREAD_Y });
    expect(second.targetToken).not.toBe(first.targetToken);
    expect(a.commands).toEqual([FOOTER_COMMAND, FOOTER_COMMAND]);

    a.switchThread(THREAD_X);
    const returned = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(returned.transcript.sessionId).toBe(THREAD_X);
    expect(returned.targetToken).not.toBe(first.targetToken);
    expect(returned.targetToken).not.toBe(second.targetToken);
    expect(returned.conversationKey).toBe(first.conversationKey);
    // X로 돌아오면 이전에 설정한 footer가 복원되므로 명령을 다시 치지 않는다.
    expect(a.commands).toEqual([FOOTER_COMMAND, FOOTER_COMMAND]);
    await expect(settle(sendPaneChatPrompt(taskId, first.targetToken!, "Stale X after return"))).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });

    await settle(sendPaneChatPrompt(taskId, returned.targetToken!, "For X"));
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "For X" }]);
    expect((await settle(getPaneChatSnapshot(taskId, "%B"))).targetToken).toBe(sibling.targetToken);
    expect(b.commands).toEqual([FOOTER_COMMAND]);
    expect(b.submissions).toEqual([]);
  });

  it("submits a multi-line prompt only after the same capture shows it in the bound thread's composer", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    await settle(sendPaneChatPrompt(taskId, target.targetToken!, "First line\n\nThird line"));
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "First line\n\nThird line" }]);
  });

  it("withholds Enter when a delayed switch keeps the old footer on screen and the pasted prompt never appears", async () => {
    const { taskId, a } = createCodexTask();
    // hook 연결이 남아 있어도 화면의 입력란이 보낸 글을 보여 주지 않으면 제출하지 않아야 한다.
    mocks.binding.mockResolvedValue({ sessionId: THREAD_X, sourceRef: rolloutFile(THREAD_X) });
    a.threadsShowingId.add(THREAD_X);
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    a.stallInputFromNextWrite();
    await expect(settle(sendPaneChatPrompt(taskId, target.targetToken!, "Must stay unsubmitted"))).rejects.toMatchObject({ name: "PaneChatDeliveryUncertainError" });
    a.completeTransition(THREAD_Y);
    expect(a.submissions).toEqual([]);
    expect(mocks.write.mock.calls.map(([, , bytes]) => bytes)).toEqual(["\x1b[200~Must stay unsubmitted\x1b[201~"]);
  });

  it("types a model command one echoed character at a time below the command popup", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    mocks.write.mockClear();
    await settle(openPaneChatModelPicker(taskId, target.targetToken!));
    expect(mocks.write.mock.calls.map(([, , bytes]) => bytes)).toEqual([..."/model", "\r"]);
    expect(a.commands).toEqual([FOOTER_COMMAND, "/model"]);
  });

  it("leaves a terminal draft untouched and reports that the composer is busy", async () => {
    const { taskId, a } = createCodexTask();
    a.composer = "unfinished terminal draft";
    const snapshot = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(mocks.write).not.toHaveBeenCalled();
    expect(a.composer).toBe("unfinished terminal draft");
    expect(snapshot).toMatchObject({ canSend: false, targetToken: null, reason: "codex-thread-input-busy", transcript: { status: "unavailable", reason: "codex-thread-input-busy" } });
  });

  it("never treats a literal placeholder or a forged footer in a terminal draft as native empty input", async () => {
    const { taskId, a } = createCodexTask();
    a.composer = "Ask Codex to do anything";
    const viewportLines = ["› Ask Codex to do anything", "", `  ${THREAD_Z}`, "  ← for agents · ? for shortcuts"];
    mocks.surface.mockImplementation(async () => ({ ...a.surface({ ...surface.task, id: taskId }), viewportLines, inputLine: viewportLines[0], cursorRow: 0, cursorColumn: 2 }));
    const snapshot = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(snapshot).toMatchObject({ canSend: false, targetToken: null, reason: "codex-thread-input-busy", transcript: { sessionId: null } });
    expect(mocks.rollout).not.toHaveBeenCalled();
    expect(mocks.write).not.toHaveBeenCalled();
    expect(a.composer).toBe("Ask Codex to do anything");
  });

  it("keeps the bound thread and its token while the terminal holds a draft under the same footer", async () => {
    const { taskId, a } = createCodexTask();
    const bound = await settle(getPaneChatSnapshot(taskId, "%A"));
    a.composer = "terminal draft\nsecond line";
    const drafting = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(drafting).toMatchObject({ canSend: true, inputState: "occupied", transcript: { status: "available", sessionId: THREAD_X } });
    expect(drafting.targetToken).toBe(bound.targetToken);
    expect(a.commands).toEqual([FOOTER_COMMAND]);
  });

  it("does not adopt a UUID from a multi-line terminal draft as the displayed thread", async () => {
    const { taskId, a } = createCodexTask();
    // 표시 줄을 꺼 둔 CLI에서 입력 본문의 마지막 줄이 footer와 같은 모양으로 화면 맨 아래에 놓인다.
    a.composer = `draft\n\n${THREAD_Z}`;
    a.isCursorOnFirstComposerRow = true;
    mocks.surface.mockImplementation(async () => {
      const shown = a.surface({ ...surface.task, id: taskId });
      const viewportLines = shown.viewportLines.slice(0, -2);
      return { ...shown, viewportLines, screen: viewportLines.join("\n"), inputLine: viewportLines[shown.cursorRow] };
    });
    const snapshot = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(snapshot).toMatchObject({ canSend: false, targetToken: null, reason: "codex-thread-input-busy", transcript: { status: "unavailable", sessionId: null } });
    expect(mocks.rollout).not.toHaveBeenCalled();
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("stops typing the footer command after two attempts fail to show a thread id", async () => {
    const { taskId, a } = createCodexTask();
    a.ignoresFooterCommand = true;
    const reads = [];
    for (let read = 0; read < 5; read++) reads.push(await settle(getPaneChatSnapshot(taskId, "%A")));
    expect(a.commands).toEqual([FOOTER_COMMAND, FOOTER_COMMAND]);
    expect(reads.at(-1)).toMatchObject({ canSend: false, targetToken: null, reason: "codex-thread-unconfirmed" });
  });

  it("refuses a displayed thread without a main-thread record in this task and types nothing", async () => {
    const { taskId, a } = createCodexTask();
    a.threadsShowingId.add(THREAD_X);
    // 하위 에이전트 화면이거나 다른 작업에서 시작한 대화다.
    mocks.rollout.mockResolvedValue(null);
    const snapshot = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(mocks.write).not.toHaveBeenCalled();
    expect(snapshot).toMatchObject({ canSend: false, targetToken: null, reason: "codex-thread-record-missing", transcript: { status: "unavailable", messages: [] } });
    expect(mocks.codex).not.toHaveBeenCalled();
  });

  it.each(["caption after image", ""])("submits a staged image once its numbered placeholder is echoed (caption=%s)", async (caption) => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    const png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aOK0AAAAASUVORK5CYII=";
    const attachment = await settle(stagePaneChatAttachment(taskId, target.targetToken!, { name: "image.png", mimeType: "image/png", dataUrl: `data:image/png;base64,${png}` }));
    await settle(sendPaneChatPrompt(taskId, target.targetToken!, caption, [attachment.id]));
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: `[Image #1] ${caption}` }]);
    await removePaneChatAttachment(taskId, target.targetToken!, attachment.id);
  });

  it("submits a long prompt that the composer folds into a character-count placeholder", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    await settle(sendPaneChatPrompt(taskId, target.targetToken!, "L".repeat(1500)));
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "[Pasted Content 1500 chars]" }]);
  });

  it.each(["codex-cli 0.168.0", "codex-cli 0.170.0"])("keeps the hook path and types nothing for %s, whose footer command was not verified", async (version) => {
    const { taskId } = createCodexTask(version);
    mocks.binding.mockResolvedValue({ sessionId: "hook-session", sourceRef: "/allowed/hook.jsonl" });
    const snapshot = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(mocks.write).not.toHaveBeenCalled();
    expect(snapshot.transcript).toMatchObject({ status: "available", sessionId: "hook-session" });
  });
});
