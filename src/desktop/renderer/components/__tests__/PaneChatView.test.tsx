import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PaneChatSnapshot } from "@/desktop/shared/paneChat";
import { PaneChatView } from "../PaneChatView";

const actions = vi.hoisted(() => ({
  getPaneChatSnapshot: vi.fn(), sendPaneChatPrompt: vi.fn(), interruptPaneChat: vi.fn(),
  openPaneChatModelPicker: vi.fn(), sendPaneChatKey: vi.fn(),
}));
vi.mock("@/desktop/renderer/actions/paneChat", () => actions);
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

  it("drives the actual model picker and keeps the composer disabled while it is open", async () => {
    open();
    await screen.findByText("Exact pane response");
    fireEvent.click(screen.getByRole("button", { name: "model" }));
    await screen.findByTestId("pane-chat-controls");
    expect(actions.openPaneChatModelPicker).toHaveBeenCalledWith("task-1", "process-a");
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Must not become a model choice" } });
    expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "down" }));
    await waitFor(() => expect(actions.sendPaneChatKey).toHaveBeenCalledWith("task-1", "process-a", "down"));
    await waitFor(() => expect((screen.getByRole("button", { name: "confirm" }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: "confirm" }));
    await waitFor(() => expect(actions.sendPaneChatKey).toHaveBeenCalledWith("task-1", "process-a", "enter"));
    expect(actions.sendPaneChatPrompt).not.toHaveBeenCalled();
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

  it("preserves a terminal draft and offers controls while the CLI is occupied", async () => {
    actions.getPaneChatSnapshot.mockResolvedValue({ ...snapshot(), inputState: "occupied" });
    open();
    await screen.findByText("inputOccupied");
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Do not append this" } });
    expect((screen.getByRole("button", { name: "send" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "model" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "controls" }));
    await screen.findByTestId("pane-chat-controls");
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
    await screen.findByText("transcriptPending");
    expect(screen.getByText("Codex ready")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "history" }));
    expect(view.onHistory).toHaveBeenCalledTimes(1);
    expect(actions.sendPaneChatPrompt).not.toHaveBeenCalled();
  });
});
