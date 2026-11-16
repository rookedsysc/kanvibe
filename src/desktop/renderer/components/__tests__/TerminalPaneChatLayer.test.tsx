import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRef } from "react";
import { TerminalPaneChatLayer } from "../TerminalPaneChatLayer";
import { REQUEST_TOGGLE_PANE_CHAT_EVENT } from "@/desktop/renderer/utils/terminalFocus";
import { getAppSetting } from "@/desktop/renderer/actions/appSettings";

const actions = vi.hoisted(() => ({ getPaneChatLayout: vi.fn(), focusPaneChatTerminal: vi.fn() }));
vi.mock("@/desktop/renderer/actions/paneChat", () => actions);
vi.mock("@/desktop/renderer/actions/appSettings", () => ({ getAppSetting: vi.fn().mockResolvedValue(null), setAppSetting: vi.fn().mockResolvedValue(undefined) }));
vi.mock("next-intl", () => ({ useTranslations: () => (key: string) => key }));
vi.mock("@/desktop/renderer/hooks/useIsWindowActive", () => ({ useIsWindowActive: () => true }));
vi.mock("../PaneChatView", () => ({
  PaneChatView: ({ paneId, isVisible, onTerminal }: { paneId: string; isVisible: boolean; onTerminal: () => void }) => (
    <div data-testid={`chat-${paneId}`} data-visible={isVisible}>
      <input aria-label={`draft-${paneId}`} />
      <button onClick={onTerminal}>terminal-{paneId}</button>
    </div>
  ),
}));

const split = {
  activePaneId: "%1",
  panes: [
    { id: "%1", tabId: "@1", bounds: { left: 0, top: 0, width: 39, height: 24 } },
    { id: "%2", tabId: "@1", bounds: { left: 40, top: 0, width: 40, height: 24 } },
  ],
};
const metrics = { cols: 80, rows: 24, left: 0, top: 0, width: 800, height: 480 };

beforeEach(() => { sessionStorage.clear(); vi.clearAllMocks(); actions.getPaneChatLayout.mockResolvedValue(split); actions.focusPaneChatTerminal.mockResolvedValue({ success: true }); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

async function toggle() {
  await act(async () => { window.dispatchEvent(new Event(REQUEST_TOGGLE_PANE_CHAT_EVENT)); });
}

function open(isVisible = true) {
  const terminalRef = createRef<HTMLDivElement>();
  const onChatFocusChange = vi.fn();
  const view = render(<><div ref={terminalRef} data-testid="terminal" /><TerminalPaneChatLayer taskId="task-1" terminalTabId="@1" terminalRef={terminalRef} metrics={metrics} isVisible={isVisible} onChatFocusChange={onChatFocusChange} /></>);
  return { ...view, terminalRef, onChatFocusChange };
}

describe("pane-local chat layer", () => {
  it("shows the chat canvas before the initial layout request finishes", async () => {
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    let resolve!: (value: typeof split) => void;
    actions.getPaneChatLayout.mockReturnValue(new Promise((accept) => { resolve = accept; }));
    open();
    await act(async () => {});
    expect(screen.getByTestId("pane-chat-loading").getAttribute("aria-busy")).toBe("true");
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    await act(async () => resolve(split));
    expect(screen.queryByTestId("pane-chat-loading")).toBeNull();
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });

  it("같은 터미널에 새로 추가한 pane에도 기본 채팅 모드를 적용한다", async () => {
    vi.useFakeTimers();
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    await act(async () => { open(); });
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    actions.getPaneChatLayout.mockResolvedValue({ ...split, activePaneId: "%2" });
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(screen.getByTestId("chat-%2").getAttribute("data-visible")).toBe("true");
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });
  it("기본 채팅 모드 설정을 새 pane에 적용하고 직접 닫은 선택은 설정보다 우선한다", async () => {
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    const first = open();
    await act(async () => {});
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    await act(async () => { fireEvent.click(screen.getByText("terminal-%1")); });
    first.unmount();
    open();
    await act(async () => {});
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });
  it("화면을 다시 불러와도 열린 채팅 모드를 복원하고 명시적으로 닫은 모드는 복원하지 않는다", async () => {
    const first = open();
    await toggle();
    first.unmount();
    const restored = open();
    await act(async () => {});
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    await act(async () => { fireEvent.click(screen.getByText("terminal-%1")); });
    restored.unmount();
    open();
    await act(async () => {});
    expect(screen.queryByTestId("chat-%1")).toBeNull();
  });

  it("다른 태스크에는 같은 pane ID가 있어도 채팅 모드를 복원하지 않는다", async () => {
    const first = open();
    await toggle();
    first.unmount();
    render(<TerminalPaneChatLayer taskId="task-2" terminalTabId="@1" terminalRef={createRef()} metrics={metrics} isVisible onChatFocusChange={vi.fn()} />);
    await act(async () => {});
    expect(screen.queryByTestId("chat-%1")).toBeNull();
  });

  it("covers only the focused split while preserving the terminal and sibling pane", async () => {
    open();
    await toggle();
    const cover = screen.getByTestId("chat-%1").parentElement!;
    expect(cover.style.left).toBe("0px");
    expect(cover.style.width).toBe("390px");
    expect(screen.queryByTestId("chat-%2")).toBeNull();
    expect(screen.getByTestId("terminal")).toBeTruthy();
    expect(actions.getPaneChatLayout).toHaveBeenCalledWith("task-1", "@1");
  });

  it("toggles each pane independently and keeps its draft across terminal mode", async () => {
    open();
    await toggle();
    fireEvent.change(screen.getByLabelText("draft-%1"), { target: { value: "Draft for Codex" } });
    await act(async () => { fireEvent.click(screen.getByText("terminal-%1")); });
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("false");
    actions.getPaneChatLayout.mockResolvedValue({ ...split, activePaneId: "%2" });
    fireEvent.pointerDown(screen.getByTestId("terminal"));
    await toggle();
    expect(screen.getByTestId("chat-%2").getAttribute("data-visible")).toBe("true");
    expect((screen.getByLabelText("draft-%1") as HTMLInputElement).value).toBe("Draft for Codex");
    actions.getPaneChatLayout.mockResolvedValue(split);
    fireEvent.pointerDown(screen.getByTestId("terminal"));
    await toggle();
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    expect(screen.getByTestId("chat-%2").getAttribute("data-visible")).toBe("true");
  });

  it("does not act on hidden terminal tabs", async () => {
    open(false);
    await toggle();
    expect(actions.getPaneChatLayout).not.toHaveBeenCalled();
    expect(screen.queryByTestId("chat-%1")).toBeNull();
  });

  it("hides an existing cover when its tab is no longer visible", async () => {
    vi.useFakeTimers();
    open();
    await toggle();
    actions.getPaneChatLayout.mockResolvedValue({ activePaneId: "%3", panes: [{ id: "%3", tabId: "@2", bounds: null }] });
    await act(async () => { await vi.advanceTimersByTimeAsync(1600); });
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("false");
  });

  it("ignores a toggle result arriving after its terminal becomes hidden", async () => {
    let resolve!: (value: typeof split) => void;
    actions.getPaneChatLayout.mockReturnValue(new Promise((accept) => { resolve = accept; }));
    const view = open();
    act(() => { window.dispatchEvent(new Event(REQUEST_TOGGLE_PANE_CHAT_EVENT)); });
    view.rerender(<TerminalPaneChatLayer taskId="task-1" terminalTabId="@1" terminalRef={view.terminalRef} metrics={metrics} isVisible={false} onChatFocusChange={view.onChatFocusChange} />);
    await act(async () => resolve(split));
    expect(screen.queryByTestId("chat-%1")).toBeNull();
  });
  it("restores runtime focus before releasing the pane back to the terminal", async () => {
    let resolve!: (value: { success: boolean }) => void;
    actions.focusPaneChatTerminal.mockReturnValue(new Promise((accept) => { resolve = accept; }));
    const view = open();
    await toggle();
    view.onChatFocusChange.mockClear();
    fireEvent.click(screen.getByText("terminal-%1"));
    expect(actions.focusPaneChatTerminal).toHaveBeenCalledWith("task-1", "%1");
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    expect(view.onChatFocusChange).not.toHaveBeenCalledWith(false);
    await act(async () => resolve({ success: true }));
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("false");
    expect(view.onChatFocusChange).toHaveBeenCalledWith(false);
  });

  it("keeps chat visible when restoring the exact runtime pane fails", async () => {
    actions.focusPaneChatTerminal.mockRejectedValue(new Error("pane disappeared"));
    const view = open();
    await toggle();
    view.onChatFocusChange.mockClear();
    await act(async () => { fireEvent.click(screen.getByText("terminal-%1")); });
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    expect(screen.getByRole("alert").textContent).toContain("returnError");
    expect(view.onChatFocusChange).not.toHaveBeenCalledWith(false);
  });

});


it("places the toggle at the owning pane and preserves sibling geometry", async () => {
  open();
  const button = await screen.findByTestId("pane-chat-toggle");
  expect(button.getAttribute("aria-label")).toContain("%1");
  expect(button.parentElement?.style.width).toBe("390px");
  await act(async () => fireEvent.click(button));
  expect(screen.getByTestId("chat-%1").parentElement?.style.width).toBe("390px");
  expect(screen.queryByTestId("chat-%2")).toBeNull();
});


it("pins a clicked toggle to its owner when runtime focus moves before the fresh read", async () => {
  open();
  const button = await screen.findByTestId("pane-chat-toggle");
  actions.getPaneChatLayout.mockResolvedValue({ ...split, activePaneId: "%2" });
  await act(async () => fireEvent.click(button));
  expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
  expect(screen.queryByTestId("chat-%2")).toBeNull();
});
