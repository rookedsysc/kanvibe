import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";
import type { CodexThreadInput } from "@/lib/aiSessions/codexThreadInput";

const mocks = vi.hoisted(() => ({ surface: vi.fn(), screen: vi.fn(), layout: vi.fn(), exec: vi.fn(), write: vi.fn(), apiInput: vi.fn(), threadSettings: vi.fn(), binding: vi.fn(), processBinding: vi.fn().mockResolvedValue(null), reportedBinding: vi.fn().mockResolvedValue(null), claude: vi.fn(), codex: vi.fn(), gemini: vi.fn(), opencode: vi.fn(), rollout: vi.fn(), emptyThread: vi.fn(), focusHerdr: vi.fn(), tabs: vi.fn(), catalog: vi.fn() }));
const modelMocks = vi.hoisted(() => ({ tail: vi.fn(), readTextFile: vi.fn() }));
vi.mock("../paneChatSurface", () => ({ readPaneChatSurface: mocks.surface, readPaneChatScreen: mocks.screen, readPaneChatLayout: mocks.layout }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.exec }));
vi.mock("@/lib/terminal", () => ({ focusHerdrTaskPane: mocks.focusHerdr, listLocalTerminalTabs: mocks.tabs }));
vi.mock("../mobileBridgeService", () => ({ writeToPane: mocks.write }));
vi.mock("../paneChatComposerCatalog", () => ({ readPaneChatSuggestions: mocks.catalog }));
vi.mock("@/lib/aiSessions/paneChatSessionBindings", () => ({ findPaneChatSessionBinding: mocks.binding, findPaneChatProcessSessionBinding: mocks.processBinding, findPaneChatReportedSessionBinding: mocks.reportedBinding }));
vi.mock("@/lib/aiSessions/readClaudeSessions", () => ({ readClaudeSessionDetail: mocks.claude }));
vi.mock("@/lib/aiSessions/readCodexSessions", () => ({ readCodexSessionDetail: mocks.codex, findCodexMainThreadRollout: mocks.rollout }));
vi.mock("@/lib/aiSessions/readGeminiSessions", () => ({ readGeminiSessionDetail: mocks.gemini }));
vi.mock("@/lib/aiSessions/readOpenCodeSessions", () => ({ readOpenCodeSessionDetail: mocks.opencode }));
vi.mock("@/lib/aiSessions/codexThreadMetadata", () => ({ readCodexEmptyThread: mocks.emptyThread }));
vi.mock("@/lib/aiSessions/codexThreadInput", () => ({ sendCodexThreadInput: mocks.apiInput }));
vi.mock("@/lib/aiSessions/codexThreadSettings", () => ({ readCodexThreadSettings: mocks.threadSettings }));
vi.mock("@/lib/aiSessions/shared", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/aiSessions/shared")>(), readJsonLinesTail: modelMocks.tail }));
vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({ ...await importOriginal<typeof import("@/lib/hostFileAccess")>(), readTextFile: modelMocks.readTextFile }));
import { focusPaneChatTerminal, getPaneChatLayout, getPaneChatSnapshot, readPaneChat, readRecentPaneChat, getPaneChatModelSettings, getPaneChatTranscriptPage, getPaneChatSuggestions, stagePaneChatAttachment, removePaneChatAttachment, sendPaneChatPrompt, interruptPaneChat, openPaneChatModelPicker, changePaneChatModelSetting, answerPaneChatPrompt, sendPaneChatKey, revealPaneChatCodexThread } from "../paneChatService";

/** pane root의 tty로 좁힌 프로세스 조회 명령인지. 다른 `ps` 호출(예: tty 단독 조회)과 구분한다 */
const isPaneProcessTableCommand = (command: string) => command.includes("ucomm=,args=");
const processTable = (start = "Thu Nov 12 19:00:00 2026") => `100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh\n200 100 200 200 ttys010 ${start} /usr/bin/python3 python3 /tmp/codex\n300 100 300 200 ttys010 Thu Nov 12 18:00:00 2026 /tmp/claude /tmp/claude`;
const surface = { task: { id: "task-chat", sessionType: SessionType.TMUX, sessionName: "kv-task", sshHost: null, worktreePath: "/repo", project: { repoPath: "/repo" } }, paneId: "%3", rootPid: 100, screen: "CLI screen", inputLine: "› ", cursorColumn: 2 };

beforeEach(() => {
  vi.clearAllMocks();
  mocks.emptyThread.mockResolvedValue(null);
  mocks.processBinding.mockResolvedValue(null);
  mocks.reportedBinding.mockResolvedValue(null);
  mocks.surface.mockResolvedValue(surface);
  mocks.screen.mockImplementation(async (read: { screen: string }) => read.screen);
  mocks.exec.mockImplementation(async (command: string) => { if (isPaneProcessTableCommand(command)) return processTable(); throw new Error("No open transcript"); });
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

/**
 * [emptyInput] 화면으로 시작해, 붙여넣은 글을 입력란에 그대로 그리는 Claude를 흉내 낸다.
 * 실제 Claude처럼 입력란은 경계선 아래에서 시작하고, 이어지는 줄은 두 칸 들여쓰며(빈 줄은 빈 채로), 커서는 글 끝에 있다.
 */
function showClaudeEchoingPastes(emptyInput: object) {
  mocks.surface.mockResolvedValue(emptyInput);
  mocks.write.mockImplementation(async (_taskId: string, _paneId: string, bytes: string) => {
    const pasted = bytes.match(/^\x1b\[200~([\s\S]*)\x1b\[201~$/)?.[1];
    if (pasted === undefined) return;
    const rows = pasted.trimEnd().split("\n").map((line, index) => index ? (line && `  ${line}`) : `❯\u00a0${line}`);
    mocks.surface.mockResolvedValue({ ...emptyInput, viewportLines: ["─".repeat(20), ...rows], cursorRow: rows.length, cursorColumn: rows.at(-1)!.length, inputLine: rows.at(-1)!, inputIsPlaceholder: false });
  });
}

/** hook이 묶어 준 Codex 기록에서 현재 모델 설정을 읽을 수 있는 상태로 만든다 */
function bindCodexSession(model = "gpt-sol") {
  mocks.binding.mockResolvedValue({ sessionId: "exact-session", sourceRef: "/allowed/exact.jsonl" });
  mocks.codex.mockResolvedValue({ sessionId: "exact-session", provider: "codex", messages: [], nextCursor: null });
  modelMocks.tail.mockResolvedValue([{ type: "event_msg", payload: { type: "thread_settings_applied", thread_settings: { model, reasoning_effort: "low", service_tier: null } } }]);
}

describe("pane chat exact CLI control", () => {
  it.each([
    ["gemini", "node", "node --no-warnings /opt/node_modules/@google/gemini-cli/bundle/gemini.js", " >   Type your message or @path/to/file", 3],
    ["opencode", "opencode.exe", "opencode", ' ┃  Ask anything… "Fix a TODO"', 4],
  ] as const)("%s의 현재 대화 조회·이전 기록·텍스트 전송과 대화 전환 fencing을 지원한다", async (provider, command, args, inputLine, cursorColumn) => {
    mocks.exec.mockResolvedValue(`100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 ${command} ${args}`);
    const viewport = provider === "opencode" ? { viewportLines: [inputLine, " ┃  Build · QA model", " ╹▀▀▀"], cursorRow: 0 } : { viewportLines: ["──────", inputLine, "──────"], cursorRow: 1 };
    mocks.surface.mockResolvedValue({ ...surface, ...viewport, inputLine, cursorColumn });
    const binding = provider === "opencode" ? mocks.processBinding : mocks.binding;
    binding.mockResolvedValue({ sessionId: "additional-exact", sourceRef: "/allowed/additional", revision: 1 });
    const reader = mocks[provider];
    reader.mockResolvedValue({ sessionId: "additional-exact", provider, messages: [{ role: "assistant", text: "exact reply", fullText: "exact reply" }], nextCursor: "1" });
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    expect(snapshot).toMatchObject({ provider, canSend: true, inputState: "ready", transcript: { sessionId: "additional-exact", messages: [{ text: "exact reply" }] }, composer: { attachments: false } });
    expect(snapshot.conversationKey).toBeTruthy();
    await getPaneChatTranscriptPage("task-chat", snapshot.targetToken!, "1");
    expect(reader).toHaveBeenLastCalledWith(expect.objectContaining({ worktreePath: "/repo" }), "additional-exact", "/allowed/additional", "1", 100);
    await sendPaneChatPrompt("task-chat", snapshot.targetToken!, "hello", [], `send-${provider}`);
    expect(mocks.write.mock.calls).toEqual([["task-chat", "%3", "\x1b[200~hello\x1b[201~"], ["task-chat", "%3", "\r"]]);
    mocks.write.mockClear();
    mocks.surface.mockResolvedValue({ ...surface, inputLine: inputLine.replace(/Type your message or @path\/to\/file|Ask anything….*/, "native draft"), cursorColumn: cursorColumn + 6 });
    await expect(sendPaneChatPrompt("task-chat", snapshot.targetToken!, "blocked")).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
    binding.mockResolvedValue({ sessionId: "other", sourceRef: "/allowed/other", revision: 2 });
    await expect(getPaneChatTranscriptPage("task-chat", snapshot.targetToken!, "1")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    await expect(sendPaneChatPrompt("task-chat", snapshot.targetToken!, "stale")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.write).not.toHaveBeenCalled();
  });
  it("Gemini의 빈 첫 줄 아래에 남은 native draft를 덮어쓰지 않는다", async () => {
    mocks.exec.mockResolvedValue(`100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 gemini gemini`);
    const viewportLines = ["──────", " >  ", "", "   ───", "   native draft", "──────"];
    mocks.surface.mockResolvedValue({ ...surface, inputLine: viewportLines[1], cursorColumn: 3, cursorRow: 1, viewportLines });
    mocks.binding.mockResolvedValue({ sessionId: "gemini-exact", sourceRef: "/allowed/gemini.jsonl" });
    mocks.gemini.mockResolvedValue({ sessionId: "gemini-exact", provider: "gemini", messages: [], nextCursor: null });
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    expect(snapshot.inputState).toBe("occupied");
    await expect(sendPaneChatPrompt("task-chat", snapshot.targetToken!, "new prompt")).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
    viewportLines[3] = "";
    viewportLines[4] = "";
    expect((await getPaneChatSnapshot("task-chat", "%3")).inputState).toBe("ready");
  });

  it("OpenCode의 빈 커서 행 위에 남은 여러 줄 draft를 덮어쓰지 않는다", async () => {
    mocks.exec.mockResolvedValue(`100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 opencode.exe opencode`);
    const viewportLines = ["  ┃  previous transcript", "", "  ┃", "  ┃  native draft", "  ┃   ", "  ┃  Build · QA model", "  ╹▀▀▀"];
    mocks.surface.mockResolvedValue({ ...surface, inputLine: viewportLines[4], cursorColumn: 5, cursorRow: 4, viewportLines });
    mocks.processBinding.mockResolvedValue({ sessionId: "opencode-home:200", sourceRef: "/repo/state.json", revision: 1, empty: true });
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    expect(snapshot.inputState).toBe("occupied");
    await expect(sendPaneChatPrompt("task-chat", snapshot.targetToken!, "new prompt")).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
    viewportLines[3] = "  ┃";
    const empty = await getPaneChatSnapshot("task-chat", "%3");
    expect(empty.inputState).toBe("ready");
  });

  it("OpenCode home의 빈 대화에서 첫 prompt를 보낼 수 있다", async () => {
    mocks.exec.mockResolvedValue(`100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 opencode.exe opencode`);
    mocks.surface.mockResolvedValue({ ...surface, inputLine: "                  ┃  Ask anything…", cursorColumn: 21, cursorRow: 0, viewportLines: ["                  ┃  Ask anything…", "                  ┃  Build · QA model", "                  ╹▀▀▀"] });
    mocks.processBinding.mockResolvedValue({ sessionId: "opencode-home:200", sourceRef: "/repo/.kanvibe/pane-chat/opencode-200.json", revision: 1, empty: true });
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    expect(snapshot).toMatchObject({ provider: "opencode", canSend: true, inputState: "ready", transcript: { messages: [], nextCursor: null } });
    await sendPaneChatPrompt("task-chat", snapshot.targetToken!, "first prompt");
    expect(mocks.write).toHaveBeenLastCalledWith("task-chat", "%3", "\r");
  });

  it.each(["gemini", "opencode"] as const)("%s의 세션을 확인하지 못하면 다른 기록을 추정하거나 입력을 허용하지 않는다", async (provider) => {
    mocks.exec.mockResolvedValue(`100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 ${provider} ${provider}`);
    mocks.binding.mockResolvedValue(null);
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    expect(snapshot).toMatchObject({ provider, targetToken: null, canSend: false, reason: "exact-session-unavailable" });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("pins catalog results and rejects a CLI change while suggestions are loading", async () => {
    const target = await getPaneChatSnapshot("task-chat", "%3");
    mocks.catalog.mockResolvedValue({ items: [{ id: "skill:review", label: "review", description: "Review", insertText: "$review" }], truncated: false });
    expect(await getPaneChatSuggestions("task-chat", target.targetToken!, "skill", "review")).toMatchObject({ targetToken: target.targetToken, items: [{ insertText: "$review" }] });
    mocks.catalog.mockImplementationOnce(async () => { mocks.exec.mockResolvedValue(processTable("Thu Nov 12 20:00:00 2026")); return { items: [], truncated: false }; });
    await expect(getPaneChatSuggestions("task-chat", target.targetToken!, "file", "readme")).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("passes a raised suggestion limit to the catalog and rejects limits outside 1 to 512", async () => {
    const target = await getPaneChatSnapshot("task-chat", "%3");
    mocks.catalog.mockResolvedValue({ items: [], truncated: false });
    await getPaneChatSuggestions("task-chat", target.targetToken!, "skill", "", 512);
    expect(mocks.catalog).toHaveBeenLastCalledWith(expect.anything(), "skill", "", 512);
    mocks.catalog.mockClear();
    for (const limit of [0, 513, 1.5, Number.NaN]) await expect(getPaneChatSuggestions("task-chat", target.targetToken!, "skill", "", limit)).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.catalog).not.toHaveBeenCalled();
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
    showClaudeEchoingPastes({ ...surface, inputLine: "❯ " });
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
    showClaudeEchoingPastes({ ...surface, inputLine: "❯ " });
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

  it("대화 기록을 연결했으면 CLI 화면 스크롤백을 읽지 않고, 화면을 요청했거나 기록을 연결하지 못했을 때만 읽는다", async () => {
    const withTranscript = await getPaneChatSnapshot("task-chat", "%3");
    expect(withTranscript).toMatchObject({ canSend: true, screen: "" });
    expect(mocks.screen).not.toHaveBeenCalled();

    const requested = await readPaneChat("task-chat", "%3", { includeScreen: true });
    expect(requested.snapshot).toMatchObject({ canSend: true, screen: "CLI screen" });
    expect(mocks.screen).toHaveBeenCalledTimes(1);

    mocks.binding.mockResolvedValue(null);
    expect(await getPaneChatSnapshot("task-chat", "%3")).toMatchObject({ canSend: false, screen: "CLI screen" });
    expect(mocks.screen).toHaveBeenCalledTimes(2);
  });

  it("CLI 화면을 읽지 못하면 대화를 연결했어도 그 pane을 보여 주지 않는다", async () => {
    mocks.screen.mockRejectedValue(new Error("capture failed"));
    expect((await readPaneChat("task-chat", "%3", { includeScreen: true })).snapshot).toMatchObject({ paneId: null, canSend: false, reason: "pane-unavailable" });
  });

  it("구독 감시가 1초 안에 읽은 결과를 같은 pane의 데스크톱 조회가 재사용하고, 시간이 지나거나 pane에 입력을 친 뒤에는 새로 읽는다", async () => {
    const now = vi.spyOn(Date, "now");
    try {
      now.mockReturnValue(1_000_000);
      const watched = await readPaneChat("task-chat", "%7", { isSharedWithOtherViewers: true });
      const readsByWatcher = mocks.surface.mock.calls.length;

      expect(await getPaneChatSnapshot("task-chat", "%7")).toBe(watched.snapshot);
      expect(mocks.surface.mock.calls.length).toBe(readsByWatcher);

      // 화면이 필요한 조회는 화면 없이 읽은 결과를 쓰지 않고, 다른 pane 식별자로 요청한 조회와는 lease가 달라 나누지 않는다
      expect((await readRecentPaneChat("task-chat", "%7", { includeScreen: true })).snapshot.screen).toBe("CLI screen");
      await getPaneChatSnapshot("task-chat");
      const readsAfterOtherRequests = mocks.surface.mock.calls.length;
      expect(readsAfterOtherRequests).toBeGreaterThan(readsByWatcher);

      now.mockReturnValue(1_001_001);
      await getPaneChatSnapshot("task-chat", "%7");
      expect(mocks.surface.mock.calls.length).toBeGreaterThan(readsAfterOtherRequests);

      const rewatched = await readPaneChat("task-chat", "%7", { isSharedWithOtherViewers: true });
      await interruptPaneChat("task-chat", rewatched.snapshot.targetToken!);
      const readsAfterInput = mocks.surface.mock.calls.length;
      await getPaneChatSnapshot("task-chat", "%7");
      expect(mocks.surface.mock.calls.length).toBeGreaterThan(readsAfterInput);
    } finally {
      now.mockRestore();
    }
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

  it("opens the real Codex picker by pasting its command once, and permits only known navigation keys", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    await openPaneChatModelPicker("task-chat", target.targetToken!);
    expect(mocks.write.mock.calls.map((call) => call[2])).toEqual(["\x1b[200~/model\x1b[201~", "\r"]);
    await sendPaneChatKey("task-chat", target.targetToken!, "down");
    expect(mocks.write).toHaveBeenLastCalledWith("task-chat", "%3", "\x1b[B");
    await expect(sendPaneChatKey("task-chat", target.targetToken!, "command" as "down")).rejects.toMatchObject({ name: "PaneChatInputError" });
  });

  it("does not press Enter if the CLI is replaced after the command was pasted", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    mocks.write.mockImplementationOnce(async () => { mocks.exec.mockResolvedValue(processTable("Thu Nov 12 19:02:00 2026")); });
    await expect(openPaneChatModelPicker("task-chat", target.targetToken!)).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.write.mock.calls.map((call) => call[2])).toEqual(["\x1b[200~/model\x1b[201~"]);
  });

  describe("Claude에 넣은 명령의 Enter 직전 입력란 확인", () => {
    const emptyClaudeInput = { ...surface, inputLine: "❯\u00a0", cursorColumn: 2 };
    /** 명령을 붙여넣은 뒤 Enter를 기다리는 사이 Claude 화면의 커서 줄이 [inputLine]으로 바뀐 상황을 만든다 */
    async function openPickerWithInputLineAfterPaste(inputLine: string) {
      mocks.exec.mockResolvedValue(processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude"));
      mocks.surface.mockResolvedValue(emptyClaudeInput);
      const target = await getPaneChatSnapshot("task-chat", "%3");
      mocks.write.mockImplementationOnce(async () => { mocks.surface.mockResolvedValue({ ...emptyClaudeInput, inputLine, cursorColumn: inputLine.length }); });
      return openPaneChatModelPicker("task-chat", target.targetToken!);
    }

    it("입력란이 넣은 명령 그대로이면 Enter를 누른다", async () => {
      await openPickerWithInputLineAfterPaste("❯\u00a0/model");
      expect(mocks.write.mock.calls.map((call) => call[2])).toEqual(["\x1b[200~/model\x1b[201~", "\r"]);
    });

    it.each([
      ["그사이 입력란의 글이 바뀌면", "❯\u00a0Reply with exactly: RACE"],
      ["그사이 선택 화면이 뜨면", "   ❯ 1. Yes, switch to Opus 5.5"],
      ["입력란 줄을 찾지 못하면", "  Enter to confirm · Esc to cancel"],
    ])("%s Enter를 누르지 않는다", async (_, inputLine) => {
      await expect(openPickerWithInputLineAfterPaste(inputLine)).rejects.toMatchObject({ name: "PaneChatInputError" });
      expect(mocks.write.mock.calls.map((call) => call[2])).toEqual(["\x1b[200~/model\x1b[201~"]);
    });
  });

  describe("Claude에 보낸 일반 메시지의 Enter 직전 입력란 확인", () => {
    const BORDER = "─".repeat(40);
    /** 입력란에 [composerRows]가 그려진 Claude 화면. 커서는 입력란의 마지막 줄 끝에 있다 */
    const claudeComposer = (...composerRows: string[]) => {
      const lines = ["● earlier reply", BORDER, ...composerRows, BORDER, "  ? for shortcuts"];
      return { ...surface, viewportLines: lines, cursorRow: composerRows.length + 1, cursorColumn: composerRows.at(-1)!.length, inputLine: composerRows.at(-1)! };
    };
    let sendSequence = 0;
    /** [text]를 보내고, 붙여넣은 뒤 Enter를 기다리는 사이 화면이 [afterPaste]로 바뀐 상황을 만든다 */
    async function sendWithScreenAfterPaste(text: string, afterPaste: object) {
      mocks.exec.mockResolvedValue(processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude"));
      mocks.surface.mockResolvedValue({ ...claudeComposer("❯\u00a0"), cursorColumn: 2 });
      const target = await getPaneChatSnapshot("task-chat", "%3");
      mocks.write.mockImplementationOnce(async () => { mocks.surface.mockResolvedValue(afterPaste); });
      return sendPaneChatPrompt("task-chat", target.targetToken!, text, [], `claude-message-${++sendSequence}`);
    }
    const sentBytes = () => mocks.write.mock.calls.map((call) => call[2]);

    it.each([
      ["한 줄 글이 그대로 보이면", "Reply with exactly: OK", ["❯\u00a0Reply with exactly: OK"]],
      ["긴 글이 여러 줄로 나뉘어 보이면", "word000 word001 word002 word003", ["❯\u00a0word000 word001", "  word002 word003"]],
      ["여러 줄 글이 접힌 표시로 보이면", "a\nb\nc\nd\ne", ["❯\u00a0[Pasted text #1 +4 lines]"]],
    ])("%s Enter를 누른다", async (_name, text, composerRows) => {
      await sendWithScreenAfterPaste(text, claudeComposer(...composerRows));

      expect(sentBytes()).toEqual([`\x1b[200~${text}\x1b[201~`, "\r"]);
    });

    it.each([
      ["그사이 입력란의 글이 바뀌면", claudeComposer("❯\u00a0Reply with exactly: RACE")],
      ["그사이 입력란이 비워지면", { ...claudeComposer("❯\u00a0"), cursorColumn: 2 }],
      ["그사이 선택 화면이 뜨면", { ...surface, viewportLines: [BORDER, " Do you want to proceed?", " ❯ 1. Yes", "   2. No"], cursorRow: 2, cursorColumn: 1, inputLine: " ❯ 1. Yes" }],
    ])("%s Enter를 누르지 않는다", async (_name, afterPaste) => {
      await expect(sendWithScreenAfterPaste("Reply with exactly: OK", afterPaste)).rejects.toMatchObject({ name: "PaneChatDeliveryUncertainError" });

      expect(sentBytes()).toEqual(["\x1b[200~Reply with exactly: OK\x1b[201~"]);
    });
  });

  it("verifies the target once for a navigation key and once for an interrupt", async () => {
    const target = await getPaneChatSnapshot("task-chat");
    mocks.surface.mockClear();
    await sendPaneChatKey("task-chat", target.targetToken!, "down");
    expect(mocks.surface).toHaveBeenCalledTimes(1);
    mocks.surface.mockClear();
    await interruptPaneChat("task-chat", target.targetToken!);
    expect(mocks.surface).toHaveBeenCalledTimes(1);
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

  it("원격 기록 파일이 바뀌지 않았으면 모델 설정용 끝부분을 다시 받지 않는다", async () => {
    const remoteFile = "/remote/model-cache.jsonl";
    let remoteMtimeSeconds = "1700000000";
    mocks.surface.mockResolvedValue({ ...surface, task: { ...surface.task, sshHost: "remote-host" } });
    mocks.binding.mockResolvedValue({ sessionId: "exact-session", sourceRef: remoteFile });
    mocks.exec.mockImplementation(async (command: string) => {
      if (isPaneProcessTableCommand(command)) return processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude");
      if (command.includes("stat -c %Y")) return remoteMtimeSeconds;
      throw new Error("No open transcript");
    });
    modelMocks.tail.mockResolvedValue([{ type: "assistant", message: { model: "claude-sonnet-4-6" } }]);

    expect((await readPaneChat("task-chat", "%3")).snapshot.modelSettings?.model).toBe("sonnet");
    expect((await readPaneChat("task-chat", "%3")).snapshot.modelSettings?.model).toBe("sonnet");
    expect(modelMocks.tail).toHaveBeenCalledTimes(1);

    remoteMtimeSeconds = "1700000005";
    modelMocks.tail.mockResolvedValue([{ type: "assistant", message: { model: "claude-opus-4-6" } }]);
    expect((await readPaneChat("task-chat", "%3")).snapshot.modelSettings?.model).toBe("opus");
    expect(modelMocks.tail).toHaveBeenCalledTimes(2);
    expect(modelMocks.tail).toHaveBeenLastCalledWith(remoteFile, 200, "remote-host");
  });

  it("reads the Claude picker metadata without opening a CLI picker", async () => {
    mocks.exec.mockResolvedValue(processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude"));
    modelMocks.tail.mockResolvedValue([{ type: "assistant", message: { model: "claude-sonnet-4-6" } }]);
    const target = await getPaneChatSnapshot("task-chat", "%3");
    expect(target.modelPicker.supportsApi).toBe(false);
    expect(target.composer?.canSendWhileBusy).toBe(false);
    expect(await getPaneChatModelSettings("task-chat", target.targetToken!)).toMatchObject({ model: "sonnet", models: expect.arrayContaining([expect.objectContaining({ id: "sonnet" })]) });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("reads transcript and model metadata concurrently", async () => {
    let release!: () => void;
    let started!: () => void;
    const transcriptStarted = new Promise<void>((resolve) => { started = resolve; });
    const original = mocks.codex.getMockImplementation()!;
    mocks.codex.mockImplementationOnce(async (...args: unknown[]) => {
      started();
      await new Promise<void>((resolve) => { release = resolve; });
      return original(...args);
    });
    const reading = getPaneChatSnapshot("task-chat", "%3");
    await transcriptStarted;
    // 모델 설정은 기록 파일의 수정 시각을 확인한 뒤 읽으므로, 대화 기록 읽기가 막혀 있는 동안 시작되는지를 기다려 본다
    await vi.waitFor(() => expect(modelMocks.tail).toHaveBeenCalled());
    release();
    expect((await reading).transcript.status).toBe("available");
  });

  it("loads an empty Claude descriptor session before its first transcript exists", async () => {
    mocks.exec.mockResolvedValue(processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude"));
    mocks.processBinding.mockResolvedValue({ sessionId: "empty-claude", sourceRef: "/allowed/not-created-yet.jsonl", pending: true });
    const target = await getPaneChatSnapshot("task-chat", "%3");
    expect(target).toMatchObject({ canSend: true, transcript: { status: "available", sessionId: "empty-claude", messages: [] }, modelSettings: { model: null, models: expect.arrayContaining([expect.objectContaining({ id: "sonnet" })]) } });
    expect(await getPaneChatModelSettings("task-chat", target.targetToken!)).toMatchObject({ model: null });
    expect(mocks.claude).not.toHaveBeenCalled();
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("uses exact Herdr session reports without a footer or CLI commands", async () => {
    mocks.surface.mockResolvedValue({ ...surface, agentSession: { provider: "codex", sessionId: "reported" } });
    mocks.binding.mockResolvedValue(null);
    mocks.reportedBinding.mockResolvedValue({ sessionId: "reported", sourceRef: "/allowed/reported.jsonl" });
    mocks.codex.mockResolvedValue({ sessionId: "reported", provider: "codex", messages: [], nextCursor: null });
    expect((await getPaneChatSnapshot("task-chat", "%3")).transcript.sessionId).toBe("reported");
    expect(mocks.reportedBinding).toHaveBeenCalledWith(expect.objectContaining({ worktreePath: "/repo" }), "codex", "reported");
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("uses the current process descriptor ahead of an older hook session", async () => {
    mocks.processBinding.mockResolvedValue({ sessionId: "process-session", sourceRef: "/allowed/process.jsonl" });
    mocks.codex.mockResolvedValue({ sessionId: "process-session", provider: "codex", messages: [], nextCursor: null });
    expect((await getPaneChatSnapshot("task-chat", "%3")).transcript.sessionId).toBe("process-session");
    expect(mocks.binding).not.toHaveBeenCalled();
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("omits model settings when the transcript tail cannot be read", async () => {
    mocks.binding.mockResolvedValue({ sessionId: "exact-session", sourceRef: "/allowed/exact.jsonl" });
    mocks.codex.mockResolvedValue({ sessionId: "exact-session", provider: "codex", messages: [], nextCursor: null });
    const snapshot = await getPaneChatSnapshot("task-chat");
    expect(snapshot.transcript.status).toBe("available");
    expect(snapshot).not.toHaveProperty("modelSettings");
  });

  it("changes a model setting by pasting the slash command built from the selectable options", async () => {
    bindCodexSession();
    const target = await getPaneChatSnapshot("task-chat");
    await changePaneChatModelSetting("task-chat", target.targetToken!, { kind: "effort", effort: "high" });
    expect(mocks.write.mock.calls.map((call) => call[2])).toEqual(["\x1b[200~/model --reasoning=high\x1b[201~", "\r"]);
    mocks.write.mockClear();
    await changePaneChatModelSetting("task-chat", target.targetToken!, { kind: "fast", isFast: true });
    expect(mocks.write.mock.calls.map((call) => call[2])).toEqual(["\x1b[200~/model --speed=priority\x1b[201~", "\r"]);
  });

  it("verifies the target the same number of times however long the model command is", async () => {
    bindCodexSession();
    const target = await getPaneChatSnapshot("task-chat");
    mocks.surface.mockClear();
    await changePaneChatModelSetting("task-chat", target.targetToken!, { kind: "model", model: "gpt-luna" });
    const targetReadsForShortCommand = mocks.surface.mock.calls.length;
    mocks.surface.mockClear();
    await changePaneChatModelSetting("task-chat", target.targetToken!, { kind: "effort", effort: "high" });
    expect(mocks.write.mock.calls.map((call) => call[2])).toEqual(["\x1b[200~/model gpt-luna\x1b[201~", "\r", "\x1b[200~/model --reasoning=high\x1b[201~", "\r"]);
    expect(mocks.surface.mock.calls.length).toBe(targetReadsForShortCommand);
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

  it("Claude가 응답 뒤 dim으로 그린 추천 문구는 빈 입력란으로 보고 보낸 글을 붙여넣는다", async () => {
    mocks.exec.mockResolvedValue(processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude"));
    const suggestion = { ...surface, inputLine: "❯\u00a0숨김 폴더 포함하면 몇 개야?", cursorColumn: 2, inputIsPlaceholder: true };
    showClaudeEchoingPastes(suggestion);
    const target = await getPaneChatSnapshot("task-chat", "%3");
    expect(target.inputState).toBe("ready");
    await sendPaneChatPrompt("task-chat", target.targetToken!, "hello");
    expect(mocks.write.mock.calls).toEqual([["task-chat", "%3", "\x1b[200~hello\x1b[201~"], ["task-chat", "%3", "\r"]]);
    mocks.surface.mockResolvedValue({ ...suggestion, inputIsPlaceholder: false });
    expect((await getPaneChatSnapshot("task-chat", "%3")).inputState).toBe("occupied");
  });

  it("Claude 입력란에 직접 쓴 글은 계속 막는다", async () => {
    mocks.exec.mockResolvedValue(processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude"));
    mocks.surface.mockResolvedValue({ ...surface, inputLine: "❯\u00a0abc", cursorColumn: 5, inputIsPlaceholder: false });
    const target = await getPaneChatSnapshot("task-chat", "%3");
    expect(target.inputState).toBe("occupied");
    await expect(sendPaneChatPrompt("task-chat", target.targetToken!, "hello")).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("Codex 표시 모드가 아닌 버전의 dim 동적 제안은 빈 입력란으로 본다", async () => {
    const viewportLines = ["› Summarize recent commits", "", "  gpt-sol low · /repo"];
    mocks.surface.mockResolvedValue({ ...surface, inputLine: viewportLines[0], cursorColumn: 2, cursorRow: 0, viewportLines, inputIsPlaceholder: true });
    expect((await getPaneChatSnapshot("task-chat", "%3")).inputState).toBe("ready");
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

describe("pane process lookup", () => {
  const scopedCommand = "pane_tty=$(ps -o tty= -p 100 | tr -d ' '); case \"$pane_tty\" in \"\"|\"?\"|\"??\") ;; *) ps -wwo pid=,ppid=,pgid=,tpgid=,tty=,lstart=,ucomm=,args= -t \"$pane_tty\" ;; esac";

  it.each([null, "remote-host"])("pane root의 tty에 붙은 프로세스만 같은 셸 명령으로 조회한다 (sshHost=%s)", async (sshHost) => {
    mocks.surface.mockResolvedValue({ ...surface, task: { ...surface.task, sshHost } });
    await getPaneChatSnapshot("task-chat", "%3");
    const processLookups = mocks.exec.mock.calls.filter(([command]) => isPaneProcessTableCommand(command));
    expect(processLookups.length).toBeGreaterThan(0);
    for (const [command, host] of processLookups) {
      expect(command).toBe(scopedCommand);
      expect(host).toBe(sshHost);
    }
    expect(mocks.exec.mock.calls.some(([command]) => command.includes("-wwaxo"))).toBe(false);
  });

  it("tty를 얻지 못해 조회 결과가 비면 CLI가 없는 것으로 보고 연결하지 않는다", async () => {
    mocks.exec.mockImplementation(async (command: string) => { if (isPaneProcessTableCommand(command)) return ""; throw new Error("No open transcript"); });
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    expect(snapshot).toMatchObject({ canSend: false, targetToken: null, reason: "no-foreground-ai-cli", provider: null });
  });
});

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
  showFooter = true;
  initialFooter = "  gpt-sol high · ~/repo";
  /** 상태줄 명령을 실행해도 footer에 ID가 나타나지 않는 화면(예: 좁은 pane에서 잘림) */
  ignoresStatusLine = false;
  /** 붙여넣기를 입력란에 그리지 않는 화면 */
  dropsPaste = false;
  /** 사용자가 여러 줄 입력의 첫 줄로 커서를 올려 둔 상태 */
  isCursorOnFirstComposerRow = false;
  /** 입력란과 footer 자리를 차지한 선택 화면의 줄. 숫자 키나 Esc를 받으면 닫힌다 */
  dialog: string[] | null = null;
  readonly dialogKeys: string[] = [];
  /** 선택 화면 위에 보이는 대화 내용. 새 대화를 시작하면 바뀐다 */
  history = "• Earlier reply";

  constructor(readonly paneId: string, readonly rootPid: number, public threadId: string) {}

  /** `/new`나 `/resume`처럼 같은 process 안에서 입력을 받는 대화가 바뀐다. footer 설정은 새 대화로 따라가지 않는다 */
  switchThread(threadId: string) {
    this.threadId = threadId;
    this.composer = "";
  }

  write(bytes: string) {
    if (this.dialog) {
      this.dialogKeys.push(bytes);
      this.dialog = null;
      return;
    }
    this.apply(bytes);
  }

  private apply(bytes: string) {
    const pasted = bytes.match(/^\x1b\[200~([\s\S]*)\x1b\[201~$/)?.[1];
    if (pasted !== undefined) {
      if (this.dropsPaste) return;
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
    // 실제 0.169.0 TUI는 붙여넣은 `/statusline thread-id`를 실행해 이 세션의 footer를 대화 ID로 바꾼다.
    if (text === "/statusline thread-id" && !this.ignoresStatusLine) this.threadsShowingId.add(this.threadId);
  }

  /** 화면을 읽지 않는 조회의 결과. 실제 pane 조회처럼 입력란·커서·화면 줄이 없다 */
  surfaceWithoutScreen(task: object) {
    return { task, paneId: this.paneId, rootPid: this.rootPid, screen: "", inputLine: null, cursorColumn: null };
  }

  surface(task: object) {
    if (this.dialog) {
      // 실제 TUI는 선택 화면을 화면 아래에 붙이고 커서를 마지막 줄 끝에 세워 둔다. 대화 ID가 있던 footer는 가려진다.
      const rows = ["  >_ OpenAI Codex (v0.169.0)", "", this.history, "", "", ...this.dialog];
      return { observation: ++codexObservationSequence, task, paneId: this.paneId, rootPid: this.rootPid, screen: rows.join("\n"), inputLine: rows.at(-1)!, cursorColumn: 120, viewportLines: rows, cursorRow: rows.length - 1, viewportColumns: 120, inputIsPlaceholder: false };
    }
    const [firstLine, ...moreLines] = (this.composer || "Ask Codex to do anything").split("\n");
    // 명령 이름을 치는 동안에는 선택 표시가 붙은 자동완성 줄이 입력란 위에 뜬다.
    const commandPopup = /^\/\S*$/.test(this.composer) ? ["› /model  choose what model and reasoning effort to use", ""] : [];
    const rows = ["  >_ OpenAI Codex (v0.169.0)", "", ...commandPopup, `› ${firstLine}`, ...moreLines.map((line) => line ? `  ${line}` : "")];
    const cursorRow = this.isCursorOnFirstComposerRow ? rows.length - 1 - moreLines.length : rows.length - 1;
    const footer = (this.showFooter || this.threadsShowingId.has(this.threadId)) ? `  ${this.threadId}` : this.initialFooter;
    rows.push("", ...footer.split("\n"), ...(this.composer ? [] : ["  ← for agents · ? for shortcuts"]));
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
  mocks.surface.mockImplementation(async (_taskId: string, paneId?: string, capture = true) => {
    const pane = panes[paneId ?? "%A"];
    if (!pane) return null;
    return capture ? pane.surface(task) : pane.surfaceWithoutScreen(task);
  });
  mocks.write.mockImplementation(async (_taskId: string, paneId: string, bytes: string) => panes[paneId].write(bytes));
  mocks.exec.mockImplementation(async (command: string) => {
    if (isPaneProcessTableCommand(command)) return codexProcessTable;
    if (/^lsof -a -p \d+ -d txt -Fn$/.test(command)) return "p200\nftxt\nn/opt/codex/bin/codex\nftxt\nn/usr/lib/dyld";
    if (command === "'/opt/codex/bin/codex' --version") return version;
    throw new Error("No open transcript");
  });
  // 공유 app-server의 hook은 pane을 증명하지 못해 연결을 남기지 않는다.
  mocks.binding.mockResolvedValue(null);
  mocks.rollout.mockImplementation(async (_context: unknown, threadId: string) => rolloutFile(threadId));
  mocks.codex.mockImplementation(async (_context: unknown, sessionId: string) => ({ sessionId, provider: "codex", messages: [], nextCursor: null }));
  mocks.apiInput.mockImplementation(async (options: CodexThreadInput) => {
    await options.beforeSend(); options.onWrite();
    const text = options.input.filter((entry) => entry.type === "text").map((entry) => entry.text).join("");
    const imageCount = options.input.filter((entry) => entry.type === "localImage").length;
    const pane = Object.values(panes).find((entry) => entry.threadId === options.threadId) ?? a;
    pane.submissions.push({ threadId: options.threadId, text: Array.from({ length: imageCount }, (_, index) => `[Image #${index + 1}] `).join("") + text });
  });
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

  it("이전 페이지도 현재 검증된 대화 파일에서만 읽고 바뀐 CLI 토큰은 거부한다", async () => {
    const { taskId, a } = createCodexTask();
    const bound = await settle(getPaneChatSnapshot(taskId, "%A"));
    mocks.codex.mockResolvedValueOnce({ sessionId: THREAD_X, messages: [{ role: "user", text: "Older X" }], nextCursor: "200" });
    expect(await settle(getPaneChatTranscriptPage(taskId, bound.targetToken!, "100"))).toMatchObject({ sessionId: THREAD_X, nextCursor: "200", messages: [{ text: "Older X" }] });
    expect(mocks.codex).toHaveBeenLastCalledWith(expect.anything(), THREAD_X, rolloutFile(THREAD_X), "100", 100);
    a.switchThread(THREAD_Y);
    await expect(settle(getPaneChatTranscriptPage(taskId, bound.targetToken!, "200"))).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
  });

  it("연결된 Codex의 모델 설정을 읽어도 대화가 바뀐 것으로 판정하지 않고 연결을 유지한다", async () => {
    const { taskId } = createCodexTask();
    const apiSettings = { model: "gpt-sol", effort: "low", isFast: null, models: [] };
    mocks.threadSettings.mockResolvedValue(apiSettings);
    const bound = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(await settle(getPaneChatModelSettings(taskId, bound.targetToken!))).toEqual(apiSettings);
    expect(mocks.threadSettings).toHaveBeenCalledWith(expect.objectContaining({ threadId: THREAD_X, sourceRef: rolloutFile(THREAD_X) }));
    expect(await settle(getPaneChatSnapshot(taskId, "%A"))).toMatchObject({ canSend: true, targetToken: bound.targetToken });
  });

  describe("Codex가 선택 화면을 띄운 동안", () => {
    const APPROVAL = ["  Would you like to run the following command?", "", "  $ git tag demo", "", "", "› 1. Yes, proceed (y)", "  2. No, and tell Codex what to do differently (esc)", "", "  Press enter to confirm or esc to cancel"];
    const RATE_LIMIT = ["  Approaching rate limits", "", "", "› 1. Switch to gpt-6-luna", "  2. Keep current model", "", "  enter select · esc back"];
    /**
     * 채팅이 pane을 한 번 읽는다. 대화를 유지하는 조건이 "마지막 확인 뒤 지난 시간"이라, 가짜 시계를 흘리며 기다리지 않고 바로 기다린다.
     * 흘리며 기다리면 파일 조회가 느린 환경에서 읽기 한 번 사이에 가짜 시간이 몇 초씩 지나간다.
     */
    const readChat = (taskId: string) => getPaneChatSnapshot(taskId, "%A");

    it("선택 화면이 하단의 대화 ID를 가려도 직전에 확인한 대화를 유지하고, 그 화면을 스냅샷에 싣는다", async () => {
      const { taskId, a } = createCodexTask();
      const bound = await readChat(taskId);
      a.dialog = APPROVAL;

      const asking = await readChat(taskId);

      expect(asking).toMatchObject({ canSend: true, targetToken: bound.targetToken, transcript: { status: "available", sessionId: THREAD_X } });
      expect(asking.prompt).toMatchObject({ kind: "permission", title: "Would you like to run the following command?", detail: "$ git tag demo", options: [{ number: 1, label: "Yes, proceed (y)" }, { number: 2 }] });
      a.dialog = null;
      expect(await readChat(taskId)).toMatchObject({ canSend: true, targetToken: bound.targetToken });
    });

    it("대화를 확인한 적 없는 Codex의 선택 화면은 대화로 연결하지 않는다", async () => {
      const { taskId, a } = createCodexTask();
      a.showFooter = false;
      a.dialog = APPROVAL;

      expect(await readChat(taskId)).toMatchObject({ canSend: false, targetToken: null });
    });

    it("선택 화면이 닫힌 뒤 다른 대화의 ID가 보이면 유지하던 대화를 버린다", async () => {
      const { taskId, a } = createCodexTask();
      const bound = await readChat(taskId);
      a.dialog = RATE_LIMIT;
      await readChat(taskId);
      a.dialog = null;
      a.switchThread(THREAD_Y);

      const after = await readChat(taskId);

      expect(after.targetToken).not.toBe(bound.targetToken);
      await expect(settle(interruptPaneChat(taskId, bound.targetToken!))).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    });

    it("카드에서 고른 번호를 숫자 키 하나로, 취소를 Esc로 보낸다", async () => {
      const { taskId, a } = createCodexTask();
      await readChat(taskId);
      a.dialog = APPROVAL;
      const asking = await readChat(taskId);

      await settle(answerPaneChatPrompt(taskId, asking.targetToken!, asking.prompt!.id, { type: "option", number: 2 }));
      await readChat(taskId);
      a.dialog = RATE_LIMIT;
      const warned = await readChat(taskId);
      await settle(answerPaneChatPrompt(taskId, warned.targetToken!, warned.prompt!.id, { type: "cancel" }));

      expect(a.dialogKeys).toEqual(["2", "\x1b"]);
    });

    it("카드를 본 뒤 다른 선택 화면으로 바뀌었으면 키를 보내지 않는다", async () => {
      const { taskId, a } = createCodexTask();
      await readChat(taskId);
      a.dialog = APPROVAL;
      const asking = await readChat(taskId);
      a.dialog = RATE_LIMIT;

      await expect(settle(answerPaneChatPrompt(taskId, asking.targetToken!, asking.prompt!.id, { type: "option", number: 1 }))).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });

      expect(a.dialogKeys).toEqual([]);
    });

    it("같은 선택 화면을 계속 지켜보는 동안에는 오래 떠 있어도 대화를 유지한다", async () => {
      const { taskId, a } = createCodexTask();
      const bound = await readChat(taskId);
      a.dialog = APPROVAL;

      for (let poll = 0; poll < 12; poll++) {
        await vi.advanceTimersByTimeAsync(1_500);
        expect(await readChat(taskId)).toMatchObject({ canSend: true, targetToken: bound.targetToken });
      }
    });

    it("대화 ID를 본 지 오래됐으면 그사이 대화가 바뀌었을 수 있으므로, 선택 화면이 떠 있어도 이전 대화를 보여 주지 않는다", async () => {
      const { taskId, a } = createCodexTask();
      await readChat(taskId);
      await vi.advanceTimersByTimeAsync(60_000);
      a.switchThread(THREAD_Y);
      a.dialog = RATE_LIMIT;

      const reopened = await readChat(taskId);

      expect(reopened).toMatchObject({ canSend: false, targetToken: null });
      expect(reopened.prompt).toBeUndefined();
    });

    it("글자가 같은 선택 화면이라도 그 위의 대화 내용이 바뀌었으면 새 대화에서 다시 뜬 화면이므로 유지를 끝낸다", async () => {
      const { taskId, a } = createCodexTask();
      await readChat(taskId);
      a.dialog = RATE_LIMIT;
      await readChat(taskId);
      a.switchThread(THREAD_Y);
      a.history = "  >_ New conversation";

      const reopened = await readChat(taskId);

      expect(reopened).toMatchObject({ canSend: false, targetToken: null });
      expect(reopened.prompt).toBeUndefined();
    });

    it("터미널에서 강조 줄만 옮긴 같은 선택 화면은 계속 유지한다", async () => {
      const { taskId, a } = createCodexTask();
      const bound = await readChat(taskId);
      a.dialog = RATE_LIMIT;
      await readChat(taskId);
      a.dialog = RATE_LIMIT.map((line) => line.startsWith("› 1.") ? line.replace("›", " ") : line.startsWith("  2.") ? `›${line.slice(1)}` : line);

      expect(await readChat(taskId)).toMatchObject({ canSend: true, targetToken: bound.targetToken, prompt: { highlightedNumber: 2 } });
    });

    it("대화 ID를 다시 보지 못한 채 다른 선택 화면으로 넘어가면 그사이 대화가 바뀌었을 수 있으므로 유지를 끝낸다", async () => {
      const { taskId, a } = createCodexTask();
      await readChat(taskId);
      a.dialog = APPROVAL;
      await readChat(taskId);
      a.switchThread(THREAD_Y);
      a.dialog = RATE_LIMIT;

      expect(await readChat(taskId)).toMatchObject({ canSend: false, targetToken: null });
    });
  });

  it("대화 ID 표시는 빈 입력란에 명령을 한 번 붙여넣고 Enter로 실행해 그 pane의 대화를 연결한다", async () => {
    const { taskId, a, b } = createCodexTask();
    a.showFooter = false;
    b.showFooter = false;
    expect(await settle(getPaneChatSnapshot(taskId, "%A"))).toMatchObject({ canSend: false, reason: "codex-thread-unconfirmed" });
    expect(await settle(revealPaneChatCodexThread(taskId, "%A"))).toEqual({ outcome: "revealed" });
    expect(mocks.write.mock.calls.map(([, paneId, bytes]) => [paneId, bytes])).toEqual([["%A", "\x1b[200~/statusline thread-id\x1b[201~"], ["%A", "\r"]]);
    expect(a.commands).toEqual(["/statusline thread-id"]);
    expect(b.commands).toEqual([]);
    expect(await settle(getPaneChatSnapshot(taskId, "%A"))).toMatchObject({ canSend: true, transcript: { status: "available", sessionId: THREAD_X } });
    expect(await settle(getPaneChatSnapshot(taskId, "%B"))).toMatchObject({ canSend: false, reason: "codex-thread-unconfirmed" });
  });

  it("대화 ID 표시는 입력란에 사용자 글이 있으면 아무것도 쓰지 않는다", async () => {
    const { taskId, a } = createCodexTask();
    a.showFooter = false;
    a.composer = "draft in progress";
    expect(await settle(revealPaneChatCodexThread(taskId, "%A"))).toEqual({ outcome: "input-not-empty" });
    expect(mocks.write).not.toHaveBeenCalled();
    expect(a.composer).toBe("draft in progress");
  });

  it("대화 ID 표시는 footer 배치를 확인하지 않은 Codex 버전에는 쓰지 않는다", async () => {
    const { taskId, a } = createCodexTask("codex-cli 0.170.0");
    a.showFooter = false;
    expect(await settle(revealPaneChatCodexThread(taskId, "%A"))).toEqual({ outcome: "unsupported-version" });
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("대화 ID 표시는 붙여넣은 명령이 입력란에 보이지 않으면 Enter를 보내지 않는다", async () => {
    const { taskId, a } = createCodexTask();
    a.showFooter = false;
    a.dropsPaste = true;
    await expect(settle(revealPaneChatCodexThread(taskId, "%A"))).rejects.toMatchObject({ name: "PaneChatInputError" });
    expect(mocks.write.mock.calls.map(([, , bytes]) => bytes)).toEqual(["\x1b[200~/statusline thread-id\x1b[201~"]);
    expect(a.commands).toEqual([]);
  });

  it("대화 ID 표시는 명령 실행 뒤에도 footer에 ID가 없으면 찾지 못했다고 알린다", async () => {
    const { taskId, a } = createCodexTask();
    a.showFooter = false;
    a.ignoresStatusLine = true;
    expect(await settle(revealPaneChatCodexThread(taskId, "%A"))).toEqual({ outcome: "thread-not-found" });
    expect(a.commands).toEqual(["/statusline thread-id"]);
  });

  it("대화 ID 표시는 붙여넣은 뒤 pane의 CLI 프로세스가 바뀌면 Enter를 보내지 않는다", async () => {
    const { taskId, a } = createCodexTask();
    a.showFooter = false;
    mocks.write.mockImplementationOnce(async (_taskId: string, _paneId: string, bytes: string) => {
      a.write(bytes);
      mocks.exec.mockImplementation(async (command: string) => {
        if (isPaneProcessTableCommand(command)) return codexProcessTable.replace("Thu Nov 12 19:00:00 2026 codex codex resume", "Thu Nov 12 19:30:00 2026 codex codex resume");
        if (/^lsof -a -p \d+ -d txt -Fn$/.test(command)) return "p200\nftxt\nn/opt/codex/bin/codex";
        if (command === "'/opt/codex/bin/codex' --version") return "codex-cli 0.169.0";
        throw new Error("No open transcript");
      });
    });
    await expect(settle(revealPaneChatCodexThread(taskId, "%A"))).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    expect(mocks.write).toHaveBeenCalledTimes(1);
    expect(a.commands).toEqual([]);
  });

  it("never writes statusline while passively reading an unconfigured Codex pane", async () => {
    const { taskId, a } = createCodexTask();
    a.showFooter = false;
    await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("reads an already configured thread footer without writing into the composer", async () => {
    const { taskId, a } = createCodexTask();
    const snapshot = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(mocks.write).not.toHaveBeenCalled();
    expect(a.commands).toEqual([]);
    expect(snapshot).toMatchObject({ canSend: true, provider: "codex", inputState: "ready", reason: null, transcript: { status: "available", sessionId: THREAD_X } });
    expect(snapshot.targetToken).toBeTruthy();
    expect(mocks.rollout).toHaveBeenCalledWith(expect.objectContaining({ worktreePath: "/repo", sshHost: null }), THREAD_X);
  });

  it("does not write to the CLI when two snapshot reads race", async () => {
    const { taskId, a } = createCodexTask();
    await settle(Promise.all([getPaneChatSnapshot(taskId, "%A"), getPaneChatSnapshot(taskId, "%A")]));
    expect(a.commands).toEqual([]);
    expect((await settle(getPaneChatSnapshot(taskId, "%A"))).transcript).toMatchObject({ status: "available", sessionId: THREAD_X });
    expect(a.commands).toEqual([]);
  });

  it("connects an idle Codex whose configured footer wraps to three rows in a narrow pane", async () => {
    const { taskId, a } = createCodexTask();
    a.showFooter = false;
    mocks.binding.mockResolvedValue({ sessionId: THREAD_X, sourceRef: rolloutFile(THREAD_X) });
    // feat/k3s-settings의 실제 69칸 pane에서 관측한 초기 상태줄이다.
    a.initialFooter = [
      "  GPT-6.1-Sol high · ~/Documents/github/rookedsysc/naive-first/more-",
      "  naive__worktrees/feat-k3s-settings · 열린 이슈를 워크트리별로 해결",
      "  · Main [default]",
    ].join("\n");
    const snapshot = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(a.commands).toEqual([]);
    expect(snapshot).toMatchObject({ canSend: true, inputState: "ready", transcript: { status: "available", sessionId: THREAD_X } });
    expect(a.submissions).toEqual([]);
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

  it("checks a fresh frame after API metadata lookup before admitting input", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    let release!: () => void;
    let started!: () => void;
    const lookupStarted = new Promise<void>((resolve) => { started = resolve; });
    const send = mocks.apiInput.getMockImplementation()!;
    mocks.apiInput.mockImplementationOnce(async (options: CodexThreadInput) => {
      started(); await new Promise<void>((resolve) => { release = resolve; });
      return send(options);
    });
    mocks.write.mockClear();
    const sending = sendPaneChatPrompt(taskId, target.targetToken!, "Must not reach Y");
    const rejected = expect(settle(sending)).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    await lookupStarted;
    a.switchThread(THREAD_Y);
    a.threadsShowingId.add(THREAD_Y);
    release();
    await rejected;
    expect(mocks.write).not.toHaveBeenCalled();
    expect(a.submissions).toEqual([]);
  });

  it("이미 연결한 Codex가 작업 중이어도 API로 입력을 추가하고 PTY 입력은 유지한다", async () => {
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
    await settle(sendPaneChatPrompt(taskId, busy.targetToken!, "Busy"));
    expect(mocks.apiInput).toHaveBeenCalledOnce();
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "Busy" }]);
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

  it("leaves an unconfigured wrapped footer untouched without exact session evidence", async () => {
    const { taskId, a } = createCodexTask();
    a.showFooter = false;
    mocks.surface.mockImplementation(async () => {
      const frame = a.surface({ ...surface.task, id: taskId });
      if (!a.composer && !a.threadsShowingId.has(a.threadId)) frame.viewportLines.splice(frame.cursorRow + 3, 0, "  long-worktree-path-wrapped-from-default-footer");
      return frame;
    });
    const bound = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(bound).toMatchObject({ canSend: false, transcript: { sessionId: null } });
    expect(a.commands).toEqual([]);
  });

  it("rebinds after a same-process conversation change, never revives the earlier token, and leaves the sibling pane bound", async () => {
    const { taskId, a, b } = createCodexTask();
    const first = await settle(getPaneChatSnapshot(taskId, "%A"));
    const sibling = await settle(getPaneChatSnapshot(taskId, "%B"));
    expect(sibling.transcript.sessionId).toBe(THREAD_Z);

    a.switchThread(THREAD_Y);
    await expect(settle(sendPaneChatPrompt(taskId, first.targetToken!, "Stale X"))).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });
    const second = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(second.transcript).toMatchObject({ status: "available", sessionId: THREAD_Y });
    expect(second.targetToken).not.toBe(first.targetToken);
    expect(a.commands).toEqual([]);

    a.switchThread(THREAD_X);
    const returned = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(returned.transcript.sessionId).toBe(THREAD_X);
    expect(returned.targetToken).not.toBe(first.targetToken);
    expect(returned.targetToken).not.toBe(second.targetToken);
    expect(returned.conversationKey).toBe(first.conversationKey);
    // X로 돌아오면 이전에 설정한 footer가 복원되므로 명령을 다시 치지 않는다.
    expect(a.commands).toEqual([]);
    await expect(settle(sendPaneChatPrompt(taskId, first.targetToken!, "Stale X after return"))).rejects.toMatchObject({ name: "PaneChatTargetChangedError" });

    await settle(sendPaneChatPrompt(taskId, returned.targetToken!, "For X"));
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "For X" }]);
    expect((await settle(getPaneChatSnapshot(taskId, "%B"))).targetToken).toBe(sibling.targetToken);
    expect(b.commands).toEqual([]);
    expect(b.submissions).toEqual([]);
  });

  it("submits multi-line API text to the exact bound thread without changing whitespace", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    await settle(sendPaneChatPrompt(taskId, target.targetToken!, "First line\n\nThird line"));
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "First line\n\nThird line" }]);
  });

  it("submits a modern Codex message without touching the PTY composer", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    mocks.write.mockClear();
    await settle(sendPaneChatPrompt(taskId, target.targetToken!, "Addressed message", [], "thread-api-once"));
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "Addressed message" }]);
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("admits a repeated API operation once and keeps an uncertain result from both API and PTY retries", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    mocks.write.mockClear();
    await settle(Promise.all([sendPaneChatPrompt(taskId, target.targetToken!, "Once", [], "api-once"), sendPaneChatPrompt(taskId, target.targetToken!, "Once", [], "api-once")]));
    expect(mocks.apiInput).toHaveBeenCalledOnce();
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "Once" }]);
    mocks.apiInput.mockImplementationOnce(async (options: CodexThreadInput) => { await options.beforeSend(); options.onWrite(); throw new Error("response lost"); });
    await expect(settle(sendPaneChatPrompt(taskId, target.targetToken!, "Unknown", [], "api-unknown"))).rejects.toMatchObject({ name: "PaneChatDeliveryUncertainError" });
    await expect(settle(sendPaneChatPrompt(taskId, target.targetToken!, "Unknown", [], "api-unknown"))).rejects.toMatchObject({ name: "PaneChatDeliveryUncertainError" });
    expect(mocks.apiInput).toHaveBeenCalledTimes(2);
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("retains an uncertain operation across X-Y-X token renewal and attachment restaging", async () => {
    const { taskId, a } = createCodexTask();
    const original = await settle(getPaneChatSnapshot(taskId, "%A"));
    const upload = { name: "same-note.txt", mimeType: "text/plain", dataUrl: "data:text/plain;base64,aGVsbG8=" };
    const attachment = await settle(stagePaneChatAttachment(taskId, original.targetToken!, upload));
    const send = mocks.apiInput.getMockImplementation()!;
    mocks.apiInput.mockImplementationOnce(async (options: CodexThreadInput) => { await send(options); throw new Error("accepted response lost"); });
    await expect(settle(sendPaneChatPrompt(taskId, original.targetToken!, "Once with file", [attachment.id], "across-renewal"))).rejects.toMatchObject({ name: "PaneChatDeliveryUncertainError" });
    a.switchThread(THREAD_Y); await settle(getPaneChatSnapshot(taskId, "%A"));
    a.switchThread(THREAD_X); const renewed = await settle(getPaneChatSnapshot(taskId, "%A"));
    const restaged = await settle(stagePaneChatAttachment(taskId, renewed.targetToken!, upload));
    expect(restaged.id).not.toBe(attachment.id);
    await expect(settle(sendPaneChatPrompt(taskId, renewed.targetToken!, "Once with file", [restaged.id], "across-renewal"))).rejects.toMatchObject({ name: "PaneChatDeliveryUncertainError" });
    expect(mocks.apiInput).toHaveBeenCalledOnce();
    expect(a.submissions).toHaveLength(1);
  });

  it("keeps native slash commands on the CLI and never also submits an AI API message", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    await settle(sendPaneChatPrompt(taskId, target.targetToken!, "/help"));
    expect(a.commands).toEqual(["/help"]);
    expect(mocks.apiInput).not.toHaveBeenCalled();
  });

  it("pins admitted API input to X even when the terminal switches at the final write boundary", async () => {
    const { taskId, a } = createCodexTask();
    mocks.binding.mockResolvedValue({ sessionId: THREAD_X, sourceRef: rolloutFile(THREAD_X) });
    a.threadsShowingId.add(THREAD_X);
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    mocks.apiInput.mockImplementationOnce(async (options: CodexThreadInput) => {
      await options.beforeSend(); a.switchThread(THREAD_Y); options.onWrite();
      a.submissions.push({ threadId: options.threadId, text: "Pinned to X" });
    });
    await settle(sendPaneChatPrompt(taskId, target.targetToken!, "Pinned to X"));
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "Pinned to X" }]);
    expect(a.composer).toBe("");
    expect(mocks.write).not.toHaveBeenCalled();
  });

  it("types a model command one echoed character at a time below the command popup", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    mocks.write.mockClear();
    await settle(openPaneChatModelPicker(taskId, target.targetToken!));
    expect(mocks.write.mock.calls.map(([, , bytes]) => bytes)).toEqual([..."/model", "\r"]);
    expect(a.commands).toEqual(["/model"]);
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

  it("검증된 표시 모드에서는 dim 제안이 보여도 빈 입력란 배치가 아니면 입력을 막는다", async () => {
    const { taskId, a } = createCodexTask();
    await settle(getPaneChatSnapshot(taskId, "%A"));
    const fakeSurface = a.surface.bind(a);
    a.surface = (task: object) => {
      const native = fakeSurface(task);
      const viewportLines = native.viewportLines.slice(0, -1);
      return { ...native, viewportLines, screen: viewportLines.join("\n") };
    };
    expect(await settle(getPaneChatSnapshot(taskId, "%A"))).toMatchObject({ inputState: "occupied" });
  });

  it("keeps the bound thread and its token while the terminal holds a draft under the same footer", async () => {
    const { taskId, a } = createCodexTask();
    const bound = await settle(getPaneChatSnapshot(taskId, "%A"));
    a.composer = "terminal draft\nsecond line";
    const drafting = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(drafting).toMatchObject({ canSend: true, inputState: "occupied", transcript: { status: "available", sessionId: THREAD_X } });
    expect(drafting.targetToken).toBe(bound.targetToken);
    expect(a.commands).toEqual([]);
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

  it("never attempts a footer command across repeated unconfirmed reads", async () => {
    const { taskId, a } = createCodexTask();
    a.showFooter = false;
    const reads = [];
    for (let read = 0; read < 5; read++) reads.push(await settle(getPaneChatSnapshot(taskId, "%A")));
    expect(a.commands).toEqual([]);
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

  it.each(["caption after image", ""])("submits a staged image as addressed input (caption=%s)", async (caption) => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    const png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aOK0AAAAASUVORK5CYII=";
    const attachment = await settle(stagePaneChatAttachment(taskId, target.targetToken!, { name: "image.png", mimeType: "image/png", dataUrl: `data:image/png;base64,${png}` }));
    await settle(sendPaneChatPrompt(taskId, target.targetToken!, caption, [attachment.id]));
    expect(mocks.apiInput.mock.calls[0][0].input[0]).toEqual({ type: "localImage", path: expect.stringMatching(/\.png$/) });
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: `[Image #1] ${caption}` }]);
    await removePaneChatAttachment(taskId, target.targetToken!, attachment.id);
  });

  it("sends a long prompt as exact API text without composer placeholders", async () => {
    const { taskId, a } = createCodexTask();
    const target = await settle(getPaneChatSnapshot(taskId, "%A"));
    await settle(sendPaneChatPrompt(taskId, target.targetToken!, "L".repeat(1500)));
    expect(a.submissions).toEqual([{ threadId: THREAD_X, text: "L".repeat(1500) }]);
  });

  it.each(["codex-cli 0.168.0", "codex-cli 0.170.0"])("keeps the hook path and types nothing for %s, whose footer command was not verified", async (version) => {
    const { taskId } = createCodexTask(version);
    mocks.binding.mockResolvedValue({ sessionId: "hook-session", sourceRef: "/allowed/hook.jsonl" });
    const snapshot = await settle(getPaneChatSnapshot(taskId, "%A"));
    expect(mocks.write).not.toHaveBeenCalled();
    expect(snapshot.transcript).toMatchObject({ status: "available", sessionId: "hook-session" });
  });
});

describe("pane chat agent activity", () => {
  const claudeProcesses = () => processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude");
  const rule = "─".repeat(40);
  /** Claude Code 2.1.325를 tmux에서 실제로 돌려 캡처한 화면의 아래쪽. 작업 중일 때만 footer에 중단 안내가 붙는다 */
  const claudeScreen = (footer: string, aboveBox = "· Swooping… (2s · thinking with high effort)") => [aboveBox, "                                    ● high · /effort", rule, "❯ ", rule, footer];
  const claudeWorkingFooter = "  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt · ← 1 agent";
  const claudeIdleFooter = "  ⏵⏵ auto mode on (shift+tab to cycle) · ← 1 agent";
  /** Codex 0.169를 실제로 돌려 캡처한 화면의 아래쪽. 작업 중일 때만 입력란 위에 Working 줄이 생긴다 */
  const codexScreen = (status: string) => [status, "                         ⚠ weekly limit: 10% left · /status", "› Ask Codex to do anything", "  GPT-6.1-Sol high · /repo", "  ← for agents · ? for shortcuts"];

  it.each([
    ["working", claudeScreen(claudeWorkingFooter)],
    ["idle", claudeScreen(claudeIdleFooter, "")],
    // 입력란 위 대화 본문에 같은 문구가 있어도 footer에 없으면 작업 중이 아니다
    ["idle", claudeScreen(claudeIdleFooter, "Press esc to interrupt a running agent.")],
  ] as const)("Claude 화면에서 %s로 판정한다", async (expected, viewportLines) => {
    mocks.exec.mockResolvedValue(claudeProcesses());
    mocks.surface.mockResolvedValue({ ...surface, inputLine: "❯ ", cursorColumn: 2, cursorRow: 3, viewportLines });
    expect((await getPaneChatSnapshot("task-chat", "%3")).agentActivity).toBe(expected);
  });

  it.each([
    ["working", "• Working (0s • esc to interrupt)"],
    ["idle", ""],
  ] as const)("Codex 화면에서 %s로 판정한다", async (expected, status) => {
    const viewportLines = codexScreen(status);
    mocks.surface.mockResolvedValue({ ...surface, inputLine: viewportLines[2], cursorColumn: 2, cursorRow: 2, viewportLines });
    expect((await getPaneChatSnapshot("task-chat", "%3")).agentActivity).toBe(expected);
  });

  it("화면 좌표가 없는 surface(zellij)나 확인하지 않은 CLI는 unknown이다", async () => {
    mocks.exec.mockResolvedValue(claudeProcesses());
    mocks.surface.mockResolvedValue({ ...surface, inputLine: null, cursorColumn: null, screen: claudeScreen(claudeWorkingFooter).join("\n") });
    expect((await getPaneChatSnapshot("task-chat", "%3")).agentActivity).toBe("unknown");

    mocks.exec.mockResolvedValue("100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 opencode.exe opencode");
    mocks.processBinding.mockResolvedValue({ sessionId: "open", sourceRef: "/allowed/open", revision: 1 });
    mocks.opencode.mockResolvedValue({ sessionId: "open", provider: "opencode", messages: [], nextCursor: null });
    const viewportLines = ["  ┃  Ask anything…", "  ┃  Build · esc to interrupt", "  ╹▀▀▀"];
    mocks.surface.mockResolvedValue({ ...surface, inputLine: viewportLines[0], cursorColumn: 5, cursorRow: 0, viewportLines });
    expect((await getPaneChatSnapshot("task-chat", "%3")).agentActivity).toBe("unknown");
  });
});

describe("Claude 선택 화면에 채팅에서 답하기", () => {
  const MODEL_SWITCH = ["▔".repeat(20), "   Switch model?", "   ❯ 1. Yes, switch to Opus 5.5", "     2. No, go back"];
  const QUESTION_HEAD = ["─".repeat(20), "←  ☐ Size  ☐ Notes  ✔ Submit  →", "", "Which size?", ""];
  const QUESTION = [...QUESTION_HEAD, "❯ 1. Small", "  2. Large", "  3. Type something."];
  const TOPPINGS_HEAD = ["─".repeat(20), "←  ☐ Toppings  ✔ Submit  →", "", "Which toppings?", ""];
  const INPUT_BOX = ["─".repeat(20), "❯ ", "─".repeat(20)];
  /** [lines]가 그려진 Claude 화면. 커서는 [cursorRow]번째 줄의 [cursorColumn] 칸에 있다 */
  const claudeScreen = (lines: string[], cursorRow: number, cursorColumn: number) => ({ ...surface, viewportLines: lines, cursorRow, cursorColumn, inputLine: lines[cursorRow] });
  const typedKeys = () => mocks.write.mock.calls.map((call) => call[2]);
  /** 단일 선택 질문의 직접 입력 줄이 [text]를 입력받는 중인 화면 */
  const typingScreen = (text: string) => claudeScreen([...QUESTION_HEAD, "  1. Small", "  2. Large", `❯ 3. ${text}`], 7, 5 + text.length);
  /** 다중 선택 질문. [highlighted]번 줄이 강조돼 있고 3번이 직접 입력 줄이다 */
  function toppingsScreen(highlighted: number, textEntry = "[ ] Type something", cheese = "[ ]") {
    const rows = [`1. ${cheese} Cheese`, "2. [ ] Olives", `3. ${textEntry}`].map((row, index) => `${index + 1 === highlighted ? "❯" : " "} ${row}`);
    return claudeScreen([...TOPPINGS_HEAD, ...rows, "     Submit"], 4 + highlighted, highlighted === 3 ? 9 : 0);
  }
  /** 다음에 보내는 키들이 차례로 화면을 [screens]로 바꾼다. 그 뒤의 키에는 선택 화면이 닫히고 입력란이 나온다 */
  function screenAfterEachKey(...screens: ReturnType<typeof claudeScreen>[]) {
    for (const screen of screens) mocks.write.mockImplementationOnce(async () => { mocks.surface.mockResolvedValue(screen); });
  }
  /** 답을 보내고, 화면을 다시 읽으며 기다리는 시간을 흘려보낸 뒤 그 결과를 받는다 */
  async function answer(snapshot: Awaited<ReturnType<typeof getPaneChatSnapshot>>, requested: unknown) {
    const answering = answerPaneChatPrompt("task-chat", snapshot.targetToken!, snapshot.prompt!.id, requested);
    const outcome = answering.then(() => "sent", (error: Error) => error.name);
    await vi.advanceTimersByTimeAsync(5_000);
    return outcome;
  }

  beforeEach(() => {
    vi.useFakeTimers();
    mocks.exec.mockResolvedValue(processTable().replace("/usr/bin/python3 python3 /tmp/codex", "/tmp/claude /tmp/claude"));
    mocks.surface.mockResolvedValue(claudeScreen(MODEL_SWITCH, 2, 3));
    mocks.write.mockImplementation(async () => { mocks.surface.mockResolvedValue(claudeScreen(INPUT_BOX, 1, 2)); });
  });
  afterEach(() => { vi.useRealTimers(); });

  it("화면의 선택 화면을 종류와 함께 스냅샷에 싣는다", async () => {
    const { prompt } = await getPaneChatSnapshot("task-chat", "%3");

    expect(prompt).toMatchObject({ kind: "confirmation", title: "Switch model?", options: [{ number: 1, label: "Yes, switch to Opus 5.5" }, { number: 2, label: "No, go back" }] });
    expect(prompt?.id).toEqual(expect.any(String));
  });

  it("종류는 화면으로 가린다. 대화 기록에 도구 호출이 아직 없어도 권한 확인이고, 도구가 실행 중이어도 모델 전환 확인은 CLI의 확인이다", async () => {
    mocks.surface.mockResolvedValue(claudeScreen(["─".repeat(20), " Bash command", "╌".repeat(20), " touch a", "╌".repeat(20), " Do you want to proceed?", " ❯ 1. Yes", "   2. No"], 6, 1));
    expect((await getPaneChatSnapshot("task-chat", "%3")).prompt).toMatchObject({ kind: "permission", detail: "Bash command\n\ntouch a" });

    mocks.claude.mockResolvedValue({ sessionId: "exact-session", provider: "claude", messages: [{ role: "tool", text: "sleep 30", fullText: "sleep 30", toolName: "Bash" }], nextCursor: null });
    mocks.surface.mockResolvedValue(claudeScreen(MODEL_SWITCH, 2, 3));
    expect((await getPaneChatSnapshot("task-chat", "%3")).prompt?.kind).toBe("confirmation");
  });

  it("AI의 질문은 대화 기록의 질문 원문으로 라벨과 설명을 가르되, 답을 보낼 때의 화면 식별값은 화면에서 읽은 그대로다", async () => {
    const label = "Use the shared configuration preset from the platform team with strict lint rules enabled";
    const narrowQuestion = ["Which preset?", "", "❯ 1. Use the shared configuration preset from the", "     platform team with strict lint rules enabled", "     recommended for new repositories", "  2. Minimal", "     smallest setup", "  3. Chat about this"];
    mocks.surface.mockResolvedValue({ ...claudeScreen(narrowQuestion, 2, 0), viewportColumns: 50 });
    const fromScreenOnly = await getPaneChatSnapshot("task-chat", "%3");
    modelMocks.tail.mockResolvedValue([{ type: "assistant", message: { role: "assistant", content: [{ type: "tool_use", name: "AskUserQuestion", input: { questions: [{ question: "Which preset?", options: [{ label, description: "recommended for new repositories" }, { label: "Minimal", description: "smallest setup" }] }] } }] } }]);

    const refined = await getPaneChatSnapshot("task-chat", "%3");

    expect(fromScreenOnly.prompt?.options[0].label).toBe("Use the shared configuration preset from the");
    expect(refined.prompt?.options[0]).toMatchObject({ label, description: "recommended for new repositories" });
    expect(refined.prompt?.id).toBe(fromScreenOnly.prompt?.id);
    expect(await answer(refined, { type: "option", number: 1 })).toBe("sent");
    expect(typedKeys()).toEqual(["1"]);
  });

  it("입력란이 보이는 화면과, 화면 배치를 확인하지 않은 CLI의 같은 화면은 선택 화면으로 싣지 않는다", async () => {
    mocks.surface.mockResolvedValue(claudeScreen(INPUT_BOX, 1, 2));
    expect((await getPaneChatSnapshot("task-chat", "%3")).prompt).toBeUndefined();

    mocks.exec.mockResolvedValue(processTable());
    mocks.surface.mockResolvedValue(claudeScreen(MODEL_SWITCH, 2, 3));
    expect((await getPaneChatSnapshot("task-chat", "%3")).prompt).toBeUndefined();
  });

  it("같은 선택 화면이 떠 있으면 고른 번호의 숫자 키를 한 번 보낸다", async () => {
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");

    expect(await answer(snapshot, { type: "option", number: 2 })).toBe("sent");

    expect(mocks.write.mock.calls).toEqual([["task-chat", "%3", "2"]]);
  });

  it("강조 위치만 바뀐 화면은 같은 선택 화면으로 본다", async () => {
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    mocks.surface.mockResolvedValue(claudeScreen([MODEL_SWITCH[0], MODEL_SWITCH[1], "     1. Yes, switch to Opus 5.5", "   ❯ 2. No, go back"], 3, 3));

    expect(await answer(snapshot, { type: "option", number: 1 })).toBe("sent");

    expect(typedKeys()).toEqual(["1"]);
  });

  it.each([
    ["다른 선택 화면으로 바뀌었으면", claudeScreen([MODEL_SWITCH[0], "   Do you want to proceed?", "   ❯ 1. Yes", "     2. No"], 2, 3)],
    ["선택 화면이 닫혔으면", claudeScreen(INPUT_BOX, 1, 2)],
  ])("답을 보내기 직전에 %s 키를 보내지 않는다", async (_name, changedScreen) => {
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    mocks.surface.mockResolvedValue(changedScreen);

    expect(await answer(snapshot, { type: "option", number: 1 })).toBe("PaneChatInputError");

    expect(typedKeys()).toEqual([]);
  });

  it("카드를 본 뒤 터미널에서 체크 상태를 바꿨으면, 같은 숫자 키가 그 선택을 되돌리므로 보내지 않는다", async () => {
    mocks.surface.mockResolvedValue(toppingsScreen(2));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    mocks.surface.mockResolvedValue(toppingsScreen(2, "[ ] Type something", "[✔]"));

    expect(await answer(snapshot, { type: "option", number: 1 })).toBe("PaneChatInputError");

    expect(typedKeys()).toEqual([]);
  });

  it("키를 보낸 뒤 화면이 그 키에 반응할 때까지 기다렸다가 끝난다. 반응이 없어도 한정된 시간 뒤에 끝난다", async () => {
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    let hasReacted = false;
    mocks.write.mockImplementationOnce(async () => { setTimeout(() => { hasReacted = true; mocks.surface.mockResolvedValue(claudeScreen(INPUT_BOX, 1, 2)); }, 120); });
    const answering = answerPaneChatPrompt("task-chat", snapshot.targetToken!, snapshot.prompt!.id, { type: "option", number: 2 }).then(() => hasReacted);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(await answering).toBe(true);

    mocks.surface.mockResolvedValue(claudeScreen(MODEL_SWITCH, 2, 3));
    const unchanged = await getPaneChatSnapshot("task-chat", "%3");
    mocks.write.mockImplementationOnce(async () => {});
    expect(await answer(unchanged, { type: "option", number: 2 })).toBe("sent");
  });

  it("글을 입력받는 중인 화면에서는 숫자가 글자로 들어가므로, 위 화살표로 그 줄을 벗어난 것을 확인한 뒤 숫자 키를 보낸다", async () => {
    mocks.surface.mockResolvedValue(typingScreen("term  ✔"));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    screenAfterEachKey(claudeScreen([...QUESTION_HEAD, "  1. Small", "❯ 2. Large", "  3. term ✔"], 6, 0));

    expect(await answer(snapshot, { type: "option", number: 1 })).toBe("sent");

    expect(typedKeys()).toEqual(["\x1b[A", "1"]);
  });

  it("글을 입력받는 중에 질문을 옮길 때도 위 화살표로 그 줄을 벗어난 뒤 좌우 화살표를 보낸다", async () => {
    mocks.surface.mockResolvedValue(typingScreen("Type something."));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    screenAfterEachKey(claudeScreen([...QUESTION_HEAD, "  1. Small", "❯ 2. Large", "  3. Type something."], 6, 0));

    expect(await answer(snapshot, { type: "step", direction: "next" })).toBe("sent");

    expect(typedKeys()).toEqual(["\x1b[A", "\x1b[C"]);
  });

  it("단일 선택의 직접 입력은 그 줄의 숫자 키로 글을 받게 하고, 붙여넣은 글이 그 줄에 그대로 보일 때 Enter로 답한다", async () => {
    mocks.surface.mockResolvedValue(claudeScreen(QUESTION, 5, 0));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    screenAfterEachKey(typingScreen("Type something."), typingScreen("Extra large"));

    expect(await answer(snapshot, { type: "text", number: 3, text: " Extra large " })).toBe("sent");

    expect(typedKeys()).toEqual(["3", "\x1b[200~Extra large\x1b[201~", "\r"]);
  });

  it("다중 선택의 직접 입력은 숫자 키가 체크만 뒤집으므로 화살표로 그 줄까지 옮기고, 글을 넣은 뒤 Enter 없이 그 줄을 벗어난다", async () => {
    mocks.surface.mockResolvedValue(toppingsScreen(1));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    screenAfterEachKey(toppingsScreen(2), toppingsScreen(3), toppingsScreen(3, "[✔] Garlic"), toppingsScreen(2, "[✔] Garlic"));

    expect(await answer(snapshot, { type: "text", number: 3, text: "Garlic" })).toBe("sent");

    expect(typedKeys()).toEqual(["\x1b[B", "\x1b[B", "\x1b[200~Garlic\x1b[201~", "\x1b[A"]);
  });

  it("직접 입력 줄에 이미 같은 글이 들어 있으면 다시 넣지 않고 Enter로 답한다", async () => {
    mocks.surface.mockResolvedValue(typingScreen("Extra large"));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");

    expect(await answer(snapshot, { type: "text", number: 3, text: "Extra large" })).toBe("sent");

    expect(typedKeys()).toEqual(["\r"]);
  });

  it("직접 입력 줄에 다른 글이 들어 있으면 글이 합쳐지지 않도록 아무 키도 보내지 않는다", async () => {
    mocks.surface.mockResolvedValue(typingScreen("from terminal"));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");

    expect(await answer(snapshot, { type: "text", number: 3, text: "from chat" })).toBe("PaneChatInputError");

    expect(typedKeys()).toEqual([]);
  });

  it("붙여넣은 글이 직접 입력 줄에 그대로 보이지 않으면 Enter를 누르지 않는다", async () => {
    mocks.surface.mockResolvedValue(typingScreen("Type something."));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");
    screenAfterEachKey(typingScreen("Extra largeX"));

    expect(await answer(snapshot, { type: "text", number: 3, text: "Extra large" })).toBe("PaneChatInputError");

    expect(typedKeys()).toEqual(["\x1b[200~Extra large\x1b[201~"]);
  });

  it.each([
    ["다음 질문", { type: "step", direction: "next" }, "\x1b[C"],
    ["이전 질문", { type: "step", direction: "previous" }, "\x1b[D"],
    ["취소", { type: "cancel" }, "\x1b"],
  ])("%s은 그 키 하나를 보낸다", async (_name, requested, key) => {
    mocks.surface.mockResolvedValue(claudeScreen(QUESTION, 5, 0));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");

    expect(await answer(snapshot, requested)).toBe("sent");

    expect(typedKeys()).toEqual([key]);
  });

  it.each([
    ["단계가 없는 화면에서 질문 이동", { type: "step", direction: "next" }],
    ["화면에 없는 번호", { type: "option", number: 3 }],
    ["두 자리 번호", { type: "option", number: 10 }],
    ["직접 입력 줄이 아닌 선택지에 글 넣기", { type: "text", number: 1, text: "note" }],
    ["줄바꿈이 든 글", { type: "text", number: 1, text: "a\nb" }],
    ["알 수 없는 종류", { type: "enter" }],
  ])("거절하고 키를 보내지 않는 답: %s", async (_name, requested) => {
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");

    expect(await answer(snapshot, requested)).toBe("PaneChatInputError");

    expect(typedKeys()).toEqual([]);
  });

  it("비어 있는 직접 입력 줄은 번호 선택으로 고를 수 없다", async () => {
    mocks.surface.mockResolvedValue(claudeScreen(QUESTION, 5, 0));
    const snapshot = await getPaneChatSnapshot("task-chat", "%3");

    expect(await answer(snapshot, { type: "option", number: 3 })).toBe("PaneChatInputError");

    expect(typedKeys()).toEqual([]);
  });
});
