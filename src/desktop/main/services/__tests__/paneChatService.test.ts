import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({ surface: vi.fn(), layout: vi.fn(), exec: vi.fn(), write: vi.fn(), binding: vi.fn(), claude: vi.fn(), codex: vi.fn(), focusHerdr: vi.fn(), tabs: vi.fn(), catalog: vi.fn() }));
const modelMocks = vi.hoisted(() => ({ tail: vi.fn(), readTextFile: vi.fn() }));
vi.mock("../paneChatSurface", () => ({ readPaneChatSurface: mocks.surface, readPaneChatLayout: mocks.layout }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.exec }));
vi.mock("@/lib/terminal", () => ({ focusHerdrTaskPane: mocks.focusHerdr, listLocalTerminalTabs: mocks.tabs }));
vi.mock("../mobileBridgeService", () => ({ writeToPane: mocks.write }));
vi.mock("../paneChatComposerCatalog", () => ({ readPaneChatSuggestions: mocks.catalog }));
vi.mock("@/lib/aiSessions/paneChatSessionBindings", () => ({ findPaneChatSessionBinding: mocks.binding }));
vi.mock("@/lib/aiSessions/readClaudeSessions", () => ({ readClaudeSessionDetail: mocks.claude }));
vi.mock("@/lib/aiSessions/readCodexSessions", () => ({ readCodexSessionDetail: mocks.codex }));
vi.mock("@/lib/aiSessions/shared", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/aiSessions/shared")>(), readJsonLinesTail: modelMocks.tail }));
vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/hostFileAccess")>(), readTextFile: modelMocks.readTextFile }));
import { focusPaneChatTerminal, getPaneChatLayout, getPaneChatSnapshot, getPaneChatSuggestions, stagePaneChatAttachment, removePaneChatAttachment, sendPaneChatPrompt, interruptPaneChat, openPaneChatModelPicker, changePaneChatModelSetting, sendPaneChatKey } from "../paneChatService";

const processTable = (start = "Thu Nov 12 19:00:00 2026") => `100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh\n200 100 200 200 ttys010 ${start} /usr/bin/python3 python3 /tmp/codex\n300 100 300 200 ttys010 Thu Nov 12 18:00:00 2026 /tmp/claude /tmp/claude`;
const surface = { task: { id: "task-chat", sessionType: SessionType.TMUX, sessionName: "kv-task", sshHost: null, worktreePath: "/repo", project: { repoPath: "/repo" } }, paneId: "%3", rootPid: 100, screen: "CLI screen", inputLine: "› ", cursorColumn: 2 };

beforeEach(() => {
  vi.clearAllMocks();
  mocks.surface.mockResolvedValue(surface);
  mocks.exec.mockImplementation(async (command: string) => { if (command.startsWith("ps ")) return processTable(); throw new Error("No open transcript"); });
  mocks.binding.mockResolvedValue(null);
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
