import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PaneChatSnapshot } from "@/desktop/shared/paneChat";
import { PaneChatView } from "../PaneChatView";
import { getAppSetting, setAppSetting } from "@/desktop/renderer/actions/appSettings";

const actions = vi.hoisted(() => ({
  getPaneChatSnapshot: vi.fn(), sendPaneChatPrompt: vi.fn(), interruptPaneChat: vi.fn(),
  openPaneChatModelPicker: vi.fn(), sendPaneChatKey: vi.fn(),
  getPaneChatTranscriptPage: vi.fn(), getPaneChatModelSettings: vi.fn(), changePaneChatModelSetting: vi.fn(),
  revealPaneChatCodexThread: vi.fn(), answerPaneChatPrompt: vi.fn(),
}));
vi.mock("@/desktop/renderer/actions/paneChat", () => actions);
vi.mock("@/desktop/renderer/actions/appSettings", () => ({ getAppSetting: vi.fn().mockResolvedValue(null), setAppSetting: vi.fn().mockResolvedValue(undefined) }));
vi.mock("next-intl", () => ({ useTranslations: () => (key: string, values?: Record<string, unknown>) => values ? `${key} ${JSON.stringify(values)}` : key }));
vi.mock("@/components/AiProviderIcon", () => ({ AiProviderIcon: () => null }));
vi.mock("../AiChatTranscriptMessage", () => ({
  AiChatTranscriptMessage: ({ message }: { message: { text: string } }) => <article>{message.text}</article>,
}));

function snapshot(token = "process-a"): PaneChatSnapshot {
  return {
    taskId: "task-1", paneId: "%3", sessionType: "tmux" as PaneChatSnapshot["sessionType"],
    provider: "codex", targetToken: token, canSend: true, inputState: "ready", agentActivity: "idle",
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

  describe("CLI가 선택 화면을 띄운 동안", () => {
    const prompt: NonNullable<PaneChatSnapshot["prompt"]> = {
      id: "prompt-a", kind: "confirmation", title: "Switch model?", detail: "", stepsLine: null, highlightedNumber: 1, isMultiSelect: false, isTextEntryActive: false, hasHiddenOptions: false,
      options: [{ number: 1, label: "Yes, switch", description: "", isChecked: false, isTextEntry: false }, { number: 2, label: "No, go back", description: "", isChecked: false, isTextEntry: false }],
    };

    it("입력창 자리에 그 화면의 카드를 보여 주고, 고른 답을 현재 대상과 그 화면의 식별값으로 보낸다", async () => {
      actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), provider: "claude", inputState: "occupied", prompt });
      actions.answerPaneChatPrompt.mockResolvedValue({ success: true });
      open();

      fireEvent.click(await screen.findByRole("button", { name: /No, go back/ }));

      expect(screen.getByRole("textbox", { hidden: true }).closest(".hidden")).not.toBeNull();
      await waitFor(() => expect(actions.answerPaneChatPrompt).toHaveBeenCalledWith("task-1", "process-a", "prompt-a", { type: "option", number: 2 }));
    });

    it("답을 보내지 못하면 서비스가 쓴 안내문만 그 카드에서 알리고, 다른 선택 화면의 카드에는 남기지 않는다", async () => {
      actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), provider: "claude", inputState: "occupied", prompt });
      actions.answerPaneChatPrompt.mockRejectedValue(new Error("Error invoking remote method 'kanvibe:invoke': PaneChatInputError: CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다."));
      open();

      fireEvent.click(await screen.findByRole("button", { name: /Yes, switch/ }));
      expect((await screen.findByRole("alert")).textContent).toBe("CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다.");

      actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), provider: "claude", inputState: "occupied", prompt: { ...prompt, id: "prompt-b", title: "Do you want to proceed?" } });

      await screen.findByText("Do you want to proceed?", {}, { timeout: 4_000 });
      expect(screen.queryByRole("alert")).toBeNull();
    });

    it("같은 선택 화면의 체크 상태만 바뀌어 카드가 갱신돼도, 답을 보내지 못한 이유와 적던 글은 그대로 남는다", async () => {
      const toppings = { ...prompt, kind: "question" as const, title: "Which toppings?", isMultiSelect: true, options: [
        { number: 1, label: "Cheese", description: "", isChecked: false, isTextEntry: false },
        { number: 2, label: "Type something", description: "", isChecked: false, isTextEntry: true },
      ] };
      actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), provider: "claude", inputState: "occupied", prompt: toppings });
      actions.answerPaneChatPrompt.mockRejectedValue(new Error("CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다."));
      open();
      fireEvent.change(await screen.findByRole("textbox", { name: "promptTextPlaceholder" }), { target: { value: "Garlic" } });
      actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), provider: "claude", inputState: "occupied", prompt: { ...toppings, id: "prompt-a-checked", options: [{ ...toppings.options[0], isChecked: true }, toppings.options[1]] } });

      fireEvent.click(screen.getByRole("button", { name: /Cheese/ }));

      await waitFor(() => expect(screen.getByRole("button", { name: /Cheese/, pressed: true })).toBeTruthy());
      expect(screen.getByRole("alert").textContent).toBe("CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다.");
      expect(screen.getByRole("textbox", { name: "promptTextPlaceholder" })).toHaveProperty("value", "Garlic");
    });

    it("답을 보낸 뒤 바뀐 화면을 받아 올 때까지는 카드를 다시 누를 수 없다", async () => {
      actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), provider: "claude", inputState: "occupied", prompt });
      actions.answerPaneChatPrompt.mockResolvedValue({ success: true });
      open();
      const yes = await screen.findByRole("button", { name: /Yes, switch/ });
      let deliver!: (value: PaneChatSnapshot) => void;
      actions.getPaneChatSnapshot.mockReturnValue(new Promise((resolve) => { deliver = resolve; }));

      fireEvent.click(yes);
      await waitFor(() => expect(actions.answerPaneChatPrompt).toHaveBeenCalledTimes(1));
      await act(async () => {});
      expect(yes).toHaveProperty("disabled", true);

      await act(async () => deliver({ ...snapshot(), provider: "claude", inputState: "occupied", prompt }));
      await waitFor(() => expect(screen.getByRole("button", { name: /Yes, switch/ })).toHaveProperty("disabled", false));
    });

    it("선택 화면이 없으면 카드 없이 입력창을 보여 준다", async () => {
      open();
      await screen.findByText("Exact pane response");

      expect(screen.queryByTestId("pane-chat-prompt")).toBeNull();
      expect(screen.getByRole("textbox").closest(".hidden")).toBeNull();
    });
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

  it("전송할 수 있는 빈 대화는 CLI 이름이 든 첫 화면 안내를 입력창 위에 보여 준다", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), transcript: { ...snapshot().transcript, messages: [] } });
    open();
    const welcome = await screen.findByTestId("pane-chat-welcome");
    expect(welcome.textContent).toContain('welcomeTitle {"provider":"Codex"}');
    expect(welcome.compareDocumentPosition(screen.getByTestId("pane-chat-composer")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(false);
  });

  it("대화에 메시지가 있으면 첫 화면 안내를 보여 주지 않는다", async () => {
    open();
    await screen.findByText("Exact pane response");
    expect(screen.queryByTestId("pane-chat-welcome")).toBeNull();
  });

  it("빈 대화에서 첫 메시지를 보내면 첫 화면 안내를 걷고 같은 입력창을 그대로 둔다", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), transcript: { ...snapshot().transcript, messages: [] } });
    actions.sendPaneChatPrompt.mockReturnValue(new Promise(() => {}));
    open();
    await screen.findByTestId("pane-chat-welcome");
    const composerInput = screen.getByRole("textbox");
    fireEvent.change(composerInput, { target: { value: "First request" } });
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    expect(screen.getByText("First request", { selector: "article" })).toBeTruthy();
    expect(screen.queryByTestId("pane-chat-welcome")).toBeNull();
    expect(screen.getByRole("textbox")).toBe(composerInput);
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

describe("연결하지 못한 대화의 원인별 안내", () => {
  const unavailable = (reason: string, provider: PaneChatSnapshot["provider"] = "codex", inputState: PaneChatSnapshot["inputState"] = "ready"): PaneChatSnapshot => ({ ...snapshot(), provider, canSend: false, targetToken: null, inputState, reason, transcript: { status: "unavailable", reason, sessionId: null, messages: [], nextCursor: null } });

  it("Codex 데몬 제약과 두 해결 방법을 보여 주고, 대화 ID 표시를 누르면 그 pane에 실행해 결과를 알린다", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue(unavailable("codex-thread-unconfirmed"));
    actions.revealPaneChatCodexThread.mockResolvedValue({ outcome: "thread-not-found" });
    open();
    await screen.findByText("codexDaemonLimit");
    expect(screen.getByText("codexRevealHint")).toBeTruthy();
    expect(screen.getByText("codexNoDaemonHint")).toBeTruthy();
    expect(screen.queryByText("transcriptIdentityMissing")).toBeNull();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "revealThread" })));
    expect(actions.revealPaneChatCodexThread).toHaveBeenCalledWith("task-1", undefined);
    expect((await screen.findByRole("alert")).textContent).toBe("revealThreadNotFound");
  });

  it.each([
    ["revealed", "revealRevealed"],
    ["input-not-empty", "revealInputNotEmpty"],
    ["unsupported-version", "revealUnsupportedVersion"],
  ] as const)("대화 ID 표시 결과 %s를 구분해 보여 준다", async (outcome, message) => {
    actions.getPaneChatSnapshot.mockResolvedValue(unavailable("codex-thread-unconfirmed"));
    actions.revealPaneChatCodexThread.mockResolvedValue({ outcome });
    open();
    const button = await screen.findByRole("button", { name: "revealThread" });
    await act(async () => fireEvent.click(button));
    expect(await screen.findByText(message)).toBeTruthy();
  });

  it("Codex가 작업 중이거나 입력란이 차 있으면 대화 ID 표시를 막고 이유를 보여 준다", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue(unavailable("codex-thread-input-busy", "codex", "occupied"));
    open();
    const button = await screen.findByRole("button", { name: "revealThread" });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("codexRevealBusy")).toBeTruthy();
    fireEvent.click(button);
    expect(actions.revealPaneChatCodexThread).not.toHaveBeenCalled();
  });

  it.each([
    ["claude", "exact-session-unavailable", "claudeIdentityMissing"],
    ["codex", "exact-session-unavailable", "codexIdentityMissing"],
    ["codex", "codex-thread-record-missing", "codexRecordMissing"],
    ["gemini", "exact-session-unavailable", "transcriptIdentityMissing"],
    ["claude", "transcript-read-failed", "transcriptReadFailed"],
  ] as const)("%s의 %s에는 %s 안내만 보이고 대화 ID 표시 버튼은 없다", async (provider, reason, message) => {
    actions.getPaneChatSnapshot.mockResolvedValue(unavailable(reason, provider));
    open();
    expect(await screen.findByText(message)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "revealThread" })).toBeNull();
  });
});

describe("agent working feedback", () => {
  it("CLI가 작업 중이면 대화 끝에 작업 중 표시와 경과 초를 보이고, 끝나면 지운다", async () => {
    vi.useFakeTimers();
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), agentActivity: "working" });
    await act(async () => { open(); });
    const indicator = screen.getByTestId("pane-chat-working");
    expect(indicator.getAttribute("role")).toBe("status");
    expect(indicator.textContent).toContain("agentWorking");
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(screen.getByTestId("pane-chat-working").textContent).toContain('agentWorkingElapsed {"seconds":1}');

    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), agentActivity: "idle" });
    await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
    expect(screen.queryByTestId("pane-chat-working")).toBeNull();
  });

  it("작업 중인지 알 수 없는 CLI도 보낸 메시지가 기록되기 전까지는 작업 중으로 보인다", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), agentActivity: "unknown" });
    actions.sendPaneChatPrompt.mockReturnValue(new Promise(() => {}));
    open();
    await screen.findByText("Exact pane response");
    expect(screen.queryByTestId("pane-chat-working")).toBeNull();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Start working" } });
    fireEvent.click(screen.getByRole("button", { name: "send" }));
    expect(screen.getByTestId("pane-chat-working")).toBeTruthy();
  });

  it("작업 중이고 입력이 비어 있으면 전송 버튼이 응답 중지로 바뀌고, 입력하면 다시 전송이 된다", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), agentActivity: "working" });
    open();
    const stop = await screen.findByRole("button", { name: "stopResponse" });
    expect(screen.queryByRole("button", { name: "send" })).toBeNull();
    fireEvent.click(stop);
    await waitFor(() => expect(actions.interruptPaneChat).toHaveBeenCalledWith("task-1", "process-a"));

    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Follow-up" } });
    expect(screen.queryByRole("button", { name: "stopResponse" })).toBeNull();
    expect(screen.getByRole("button", { name: "send" })).toBeTruthy();
  });
});
