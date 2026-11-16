import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PaneChatSnapshot } from "@/desktop/shared/paneChat";
import { PaneChatView } from "../PaneChatView";
import { getAppSetting, setAppSetting } from "@/desktop/renderer/actions/appSettings";

const actions = vi.hoisted(() => ({
  getPaneChatSnapshot: vi.fn(), sendPaneChatPrompt: vi.fn(), interruptPaneChat: vi.fn(),
  openPaneChatModelPicker: vi.fn(), sendPaneChatKey: vi.fn(),
  getPaneChatTranscriptPage: vi.fn(), getPaneChatModelSettings: vi.fn(), changePaneChatModelSetting: vi.fn(),
}));
vi.mock("@/desktop/renderer/actions/paneChat", () => actions);
vi.mock("@/desktop/renderer/actions/appSettings", () => ({ getAppSetting: vi.fn().mockResolvedValue(null), setAppSetting: vi.fn().mockResolvedValue(undefined) }));
vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
vi.mock("@/components/AiProviderIcon", () => ({ AiProviderIcon: () => null }));
vi.mock("../AiChatTranscriptMessage", () => ({
  AiChatTranscriptMessage: ({ message }: { message: { text: string } }) => <article>{message.text}</article>,
}));

function snapshot(token = "process-a"): PaneChatSnapshot {
  return {
    taskId: "task-1", paneId: "%3", sessionType: "tmux" as PaneChatSnapshot["sessionType"],
    provider: "codex", targetToken: token, canSend: true, inputState: "ready",
    screen: "Codex ready", reason: null, modelPicker: { supported: true, mode: "native-cli" },
    transcript: { status: "available", reason: null, sessionId: "session-a", nextCursor: null, messages: [
      { role: "assistant", timestamp: null, text: "Exact pane response", fullText: "Exact pane response", isTruncated: false },
    ] },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  actions.getPaneChatSnapshot.mockResolvedValue(snapshot());
  actions.sendPaneChatPrompt.mockResolvedValue({ success: true });
  actions.interruptPaneChat.mockResolvedValue({ success: true });
  actions.openPaneChatModelPicker.mockResolvedValue({ success: true });
  actions.sendPaneChatKey.mockResolvedValue({ success: true });
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });

function open() {
  const onHistory = vi.fn();
  const onTerminal = vi.fn();
  const view = render(<PaneChatView taskId="task-1" onHistory={onHistory} onTerminal={onTerminal} />);
  return { ...view, onHistory, onTerminal };
}

describe("current pane chat", () => {
  it("전송 응답이 오기 전 말풍선을 표시하고 원본 기록이 도착하면 중복을 제거한다", async () => {
    let accept!: (value: { success: true }) => void;
    actions.sendPaneChatPrompt.mockReturnValue(new Promise((resolve) => { accept = resolve; }));
    open();
    await screen.findByText("Exact pane response");
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Immediate request" } });
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    expect(screen.getByText("Immediate request", { selector: "article" })).toBeTruthy();
    const message = { role: "user" as const, timestamp: "2026-11-15T12:00:00Z", text: "Immediate request", fullText: "Immediate request", isTruncated: false };
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), transcript: { ...snapshot().transcript, messages: [message, ...snapshot().transcript.messages] } });
    await act(async () => accept({ success: true }));
    await waitFor(() => expect(screen.queryByTestId("pane-chat-pending-message")).toBeNull());
    expect(screen.getAllByText("Immediate request")).toHaveLength(1);
  });

  it("현재 채팅에서 이전 페이지를 이어 읽고 최신 폴링 뒤에도 펼친 기록을 유지한다", async () => {
    vi.useFakeTimers();
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), transcript: { ...snapshot().transcript, nextCursor: "100" } });
    actions.getPaneChatTranscriptPage.mockResolvedValue({ ...snapshot().transcript, nextCursor: null, messages: [{ role: "user", timestamp: null, text: "Older request", fullText: "Older request", isTruncated: false }] });
    await act(async () => { open(); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "loadOlder" })); });
    expect(screen.getByText("Older request")).toBeTruthy();
    expect(actions.getPaneChatTranscriptPage).toHaveBeenCalledWith("task-1", "process-a", "100");
    await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
    expect(screen.getByText("Older request")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "loadOlder" })).toBeNull();
  });

  it("History 옆 지침 표시를 켜면 시스템과 자동 AGENTS 문맥을 볼 수 있다", async () => {
    const messages = [
      { role: "developer" as const, timestamp: null, text: "Project instructions", fullText: "Project instructions", isTruncated: false },
      { role: "user" as const, timestamp: null, text: "# AGENTS.md instructions for /repo\n<INSTRUCTIONS>rules</INSTRUCTIONS>\n<environment_context>cwd</environment_context>", fullText: "", isTruncated: false },
      ...snapshot().transcript.messages,
    ];
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), transcript: { ...snapshot().transcript, messages } });
    open();
    await screen.findByText("Exact pane response");
    expect(screen.queryByText("Project instructions")).toBeNull();
    await waitFor(() => expect((screen.getByRole("checkbox", { name: "showInstructions" }) as HTMLInputElement).disabled).toBe(false));
    expect(screen.getByRole("checkbox", { name: "showInstructions" }).compareDocumentPosition(screen.getByRole("button", { name: "history" })) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox", { name: "showInstructions" }));
    await screen.findByText("Project instructions");
    expect(setAppSetting).toHaveBeenCalledWith("pane_chat_show_instructions", "true");
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });

  it("Codex 모델 API 선택기는 채팅 안에 열리고 터미널로 전환하지 않는다", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), modelPicker: { supported: true, mode: "native-cli", supportsApi: true } });
    actions.getPaneChatModelSettings.mockResolvedValue({ model: "a", effort: "high", models: [{ id: "a", label: "A", effortLevels: [{ id: "high", label: "High" }] }, { id: "b", label: "B", effortLevels: [] }] });
    actions.changePaneChatModelSetting.mockResolvedValue({ success: true });
    const view = open();
    await screen.findByText("Exact pane response");
    fireEvent.click(screen.getByRole("button", { name: "model" }));
    const option = await screen.findByRole("menuitemradio", { name: "B" });
    await waitFor(() => expect((option as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(option);
    await waitFor(() => expect(actions.changePaneChatModelSetting).toHaveBeenCalledWith("task-1", "process-a", { kind: "model", model: "b" }));
    expect(view.onTerminal).not.toHaveBeenCalled();
    expect(actions.openPaneChatModelPicker).not.toHaveBeenCalled();
  });
  it("Claude metadata opens the model popover without native API support or terminal switching", async () => {
    const modelSettings = { model: "sonnet", effort: "high", isFast: false, models: [{ id: "sonnet", label: "Sonnet", supportsFast: false, effortLevels: [] }] };
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), provider: "claude", modelSettings, modelPicker: { supported: true, mode: "native-cli", supportsApi: false } });
    actions.getPaneChatModelSettings.mockResolvedValue(modelSettings);
    const view = open();
    await screen.findByText("Exact pane response");
    fireEvent.click(screen.getByRole("button", { name: /Sonnet/ }));
    await screen.findByRole("menuitemradio", { name: "Sonnet" });
    expect(view.onTerminal).not.toHaveBeenCalled();
    expect(actions.openPaneChatModelPicker).not.toHaveBeenCalled();
  });
  it.each(["ready", "occupied"] as const)("hides the CLI capture once the transcript renders while %s", async (inputState) => {
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), inputState });
    open();
    await screen.findByText("Exact pane response");
    expect(screen.queryByText("screen")).toBeNull();
    expect(screen.queryByText("Codex ready")).toBeNull();
  });

  it("queries the explicit pane instead of following another terminal's focus", async () => {
    render(<PaneChatView taskId="task-1" paneId="%3" onHistory={vi.fn()} onTerminal={vi.fn()} />);
    await screen.findByText("Exact pane response");
    expect(actions.getPaneChatSnapshot).toHaveBeenCalledWith("task-1", "%3");
  });

  it("does not send a previous pane snapshot while the next pane is loading", async () => {
    const view = render(<PaneChatView taskId="task-1" paneId="%3" onHistory={vi.fn()} onTerminal={vi.fn()} />);
    await screen.findByText("Exact pane response");
    actions.getPaneChatSnapshot.mockReturnValue(new Promise(() => {}));
    view.rerender(<PaneChatView taskId="task-1" paneId="%4" onHistory={vi.fn()} onTerminal={vi.fn()} />);
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
    expect(actions.sendPaneChatPrompt).not.toHaveBeenCalled();
  });

  it("leads with the task title and only names the exact snapshot pane", async () => {
    const paneId = "67adc931-cb8d-46da-af83-651880ce17e2";
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), paneId });
    const view = render(<PaneChatView taskId="task-1" taskTitle="Fix checkout validation" paneTabs={[{ id: paneId, name: "Review API" }]} onHistory={vi.fn()} onTerminal={vi.fn()} />);
    await screen.findByText("Exact pane response");
    expect(screen.getByRole("heading", { name: "Fix checkout validation" })).toBeTruthy();
    expect(screen.getByText("Review API · Codex · 80ce17e2")).toBeTruthy();
    expect(screen.getByTitle(paneId).getAttribute("aria-label")).toBe(paneId);
    view.rerender(<PaneChatView taskId="task-1" taskTitle="Fix checkout validation" paneTabs={[{ id: "other-pane", name: "Unrelated pane" }]} onHistory={vi.fn()} onTerminal={vi.fn()} />);
    expect(screen.queryByText(/Unrelated pane/)).toBeNull();
    expect(screen.getByText("Codex · 80ce17e2")).toBeTruthy();
  });

  it("waits for a confirmed target before accepting a draft", async () => {
    let resolve!: (value: PaneChatSnapshot) => void;
    actions.getPaneChatSnapshot.mockReturnValue(new Promise((accept) => { resolve = accept; }));
    open();
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
    await act(async () => resolve(snapshot()));
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(false);
  });
  it("sends the multiline draft to the exact process and clears it only after acceptance", async () => {
    let accept!: (value: { success: true }) => void;
    actions.sendPaneChatPrompt.mockReturnValue(new Promise((resolve) => { accept = resolve; }));
    open();
    await screen.findByText("Exact pane response");
    const input = screen.getByRole("textbox") as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "First line\nSecond line" } });
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    expect(actions.sendPaneChatPrompt).toHaveBeenCalledWith("task-1", "process-a", "First line\nSecond line", [], expect.any(String));
    expect(input.value).toBe("First line\nSecond line");
    await act(async () => accept({ success: true }));
    await waitFor(() => expect(input.value).toBe(""));
  });

  it("retains the draft on a rejected write and never automatically resends it", async () => {
    actions.sendPaneChatPrompt.mockRejectedValue(new Error("CLI target changed"));
    open();
    await screen.findByText("Exact pane response");
    const input = screen.getByRole("textbox") as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "Keep this draft" } });
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    await screen.findByRole("alert");
    expect(input.value).toBe("Keep this draft");
    expect(actions.sendPaneChatPrompt).toHaveBeenCalledTimes(1);
  });

  it("opens the same terminal for an unsupported native model dialog", async () => {
    const view = open();
    await screen.findByText("Exact pane response");
    fireEvent.click(screen.getByRole("button", { name: "model" }));
    await waitFor(() => expect(view.onTerminal).toHaveBeenCalledTimes(1));
    expect(actions.openPaneChatModelPicker).toHaveBeenCalledWith("task-1", "process-a");
    expect(actions.sendPaneChatKey).not.toHaveBeenCalled();
  });

  it("isolates drafts when the focused process changes and restores the original draft on return", async () => {
    vi.useFakeTimers();
    await act(async () => { open(); });
    expect(screen.getByText("Exact pane response")).toBeTruthy();
    const input = screen.getByRole("textbox") as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: "For process A only" } });
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot("process-b"), paneId: "%4" });
    await act(async () => { await vi.advanceTimersByTimeAsync(1600); });
    expect(input.value).toBe("");
    actions.getPaneChatSnapshot.mockResolvedValue(snapshot());
    await act(async () => { await vi.advanceTimersByTimeAsync(1600); });
    expect(input.value).toBe("For process A only");
    expect(actions.sendPaneChatPrompt).not.toHaveBeenCalled();
  });

  it("preserves a terminal draft without generic controls while the CLI is occupied", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), inputState: "occupied" });
    open();
    await screen.findByText("inputOccupied");
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Do not append this" } });
    expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "model" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByRole("button", { name: "controls" })).toBeNull();
    expect(actions.sendPaneChatPrompt).not.toHaveBeenCalled();
  });

  it("keeps its draft across mode switches and pauses hidden pane polling", async () => {
    vi.useFakeTimers();
    let view!: ReturnType<typeof open>;
    await act(async () => { view = open(); });
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Return to this draft" } });
    view.rerender(<PaneChatView taskId="task-1" isVisible={false} onHistory={view.onHistory} onTerminal={view.onTerminal} />);
    const reads = actions.getPaneChatSnapshot.mock.calls.length;
    await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
    expect(actions.getPaneChatSnapshot).toHaveBeenCalledTimes(reads);
    await act(async () => {
      view.rerender(<PaneChatView taskId="task-1" onHistory={view.onHistory} onTerminal={view.onTerminal} />);
    });
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Return to this draft");
  });

  it("rechecks the active pane before accepting input after returning from the terminal", async () => {
    const view = open();
    await screen.findByText("Exact pane response");
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Original pane draft" } });
    view.rerender(<PaneChatView taskId="task-1" isVisible={false} onHistory={view.onHistory} onTerminal={view.onTerminal} />);
    let resolve!: (value: PaneChatSnapshot) => void;
    actions.getPaneChatSnapshot.mockReturnValue(new Promise((accept) => { resolve = accept; }));
    view.rerender(<PaneChatView taskId="task-1" onHistory={view.onHistory} onTerminal={view.onTerminal} />);
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
    await act(async () => resolve({ ...snapshot("process-b"), paneId: "%4" }));
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("");
    expect(actions.sendPaneChatPrompt).not.toHaveBeenCalled();
  });

  it("shows the same CLI screen when no exact transcript exists and keeps history separate", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), transcript: { status: "unavailable", reason: "exact-session-unavailable", sessionId: null, messages: [], nextCursor: null } });
    const view = open();
    await screen.findByText("transcriptUnavailable");
    expect(screen.getByText("Codex ready")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "history" }));
    expect(view.onHistory).toHaveBeenCalledTimes(1);
    expect(actions.sendPaneChatPrompt).not.toHaveBeenCalled();
  });
});


it("shows recoverable unavailable without a permanent CLI controller", async () => {
  actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), canSend: false, targetToken: null, reason: "exact-session-unavailable", transcript: { status: "unavailable", reason: "exact-session-unavailable", sessionId: null, messages: [], nextCursor: null } });
  const view = open();
  await screen.findByText("transcriptUnavailable");
  expect(screen.queryByRole("button", { name: "controls" })).toBeNull();
  expect(screen.queryByText("transcriptPending")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "retry" }));
  await waitFor(() => expect(actions.getPaneChatSnapshot).toHaveBeenCalledTimes(2));
  fireEvent.click(screen.getAllByRole("button", { name: "terminal" })[0]);
  expect(view.onTerminal).toHaveBeenCalled();
});


it("backs off unavailable polling and allows an immediate explicit retry", async () => {
  vi.useFakeTimers();
  actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), canSend: false, targetToken: null, transcript: { status: "unavailable", reason: "exact-session-unavailable", sessionId: null, messages: [], nextCursor: null } });
  await act(async () => { open(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(7000); });
  expect(actions.getPaneChatSnapshot).toHaveBeenCalledTimes(3);
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(actions.getPaneChatSnapshot).toHaveBeenCalledTimes(4);
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "retry" })));
  expect(actions.getPaneChatSnapshot).toHaveBeenCalledTimes(5);
});

it("터미널 입력 대기는 정확히 안내하고 세 번 이후에도 확인해 입력이 끝나면 자동으로 연결한다", async () => {
  vi.useFakeTimers();
  actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), canSend: false, targetToken: null, inputState: "occupied", transcript: { status: "unavailable", reason: "codex-thread-input-busy", sessionId: null, messages: [], nextCursor: null } });
  await act(async () => { open(); });
  expect(screen.getByText("transcriptInputBusy")).toBeTruthy();
  expect(screen.queryByText("transcriptIdentityMissing")).toBeNull();
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(actions.getPaneChatSnapshot.mock.calls.length).toBeGreaterThan(3);
  actions.getPaneChatSnapshot.mockResolvedValue(snapshot());
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(screen.getByText("Exact pane response")).toBeTruthy();
  expect(screen.queryByText("transcriptInputBusy")).toBeNull();
  expect(actions.sendPaneChatPrompt).not.toHaveBeenCalled();
});
