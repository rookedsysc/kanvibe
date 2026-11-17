import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRef } from "react";
import { TerminalPaneChatLayer } from "../TerminalPaneChatLayer";
import { REQUEST_TOGGLE_PANE_CHAT_EVENT } from "@/desktop/renderer/utils/terminalFocus";
import { getAppSetting } from "@/desktop/renderer/actions/appSettings";

const actions = vi.hoisted(() => ({ getPaneChatLayout: vi.fn(), getPaneChatAgentProvider: vi.fn(), getPaneChatSnapshot: vi.fn(), focusPaneChatTerminal: vi.fn(), getPaneChatModes: vi.fn(), setPaneChatMode: vi.fn() }));
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
/** 실제 조회처럼 부를 때마다 새 객체를 돌려준다. 주기 조회가 이어지는 동안의 변화를 보는 테스트에서 쓴다 */
const readFreshLayout = async () => ({ ...split });

/** 자동으로 채팅을 열지 정할 때 보는 값만 담은 pane 채팅 상태 */
const chatState = (canSend: boolean, reason: string | null = null) => ({ canSend, transcript: { reason } });

/** 데스크톱 앱 설정에 저장된 작업별 pane 채팅 모드를 흉내 낸다 */
let savedModes: Record<string, Record<string, boolean>> = {};

beforeEach(() => {
  savedModes = {};
  vi.clearAllMocks();
  actions.getPaneChatLayout.mockResolvedValue(split);
  // 따로 바꾸지 않는 테스트에서는 모든 pane에서 CLI가 실행 중이고 그 대화를 연결할 수 있다.
  actions.getPaneChatAgentProvider.mockResolvedValue("claude");
  actions.getPaneChatSnapshot.mockResolvedValue(chatState(true));
  actions.focusPaneChatTerminal.mockResolvedValue({ success: true });
  actions.getPaneChatModes.mockImplementation(async (taskId: string) => ({ ...savedModes[taskId] }));
  actions.setPaneChatMode.mockImplementation(async (taskId: string, paneId: string, isEnabled: boolean) => {
    savedModes[taskId] = { ...savedModes[taskId], [paneId]: isEnabled };
  });
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); });

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
  it("기본 채팅 모드가 켜져 있어도 pane 배치와 CLI를 확인하기 전에는 연결 화면 없이 터미널을 보여 주고, 확인되면 채팅을 연다", async () => {
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    let resolveLayout!: (value: typeof split) => void;
    let resolveProvider!: (provider: string | null) => void;
    actions.getPaneChatLayout.mockReturnValue(new Promise((accept) => { resolveLayout = accept; }));
    actions.getPaneChatAgentProvider.mockReturnValue(new Promise((accept) => { resolveProvider = accept; }));
    open();
    await act(async () => {});
    expect(screen.queryByTestId("pane-chat-loading")).toBeNull();
    await act(async () => resolveLayout(split));
    expect(screen.queryByTestId("pane-chat-loading")).toBeNull();
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    expect(actions.getPaneChatAgentProvider).toHaveBeenCalledWith("task-1", "%1");
    await act(async () => resolveProvider("claude"));
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    expect(actions.setPaneChatMode).toHaveBeenCalledWith("task-1", "%1", true);
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });

  it("기본 채팅 모드가 켜져 있어도 CLI가 없는 pane은 채팅으로 열지 않고 모드도 저장하지 않는다", async () => {
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    actions.getPaneChatAgentProvider.mockResolvedValue(null);
    open();
    await act(async () => {});
    expect(actions.getPaneChatAgentProvider).toHaveBeenCalledWith("task-1", "%1");
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    expect(screen.queryByTestId("pane-chat-loading")).toBeNull();
    expect(actions.setPaneChatMode).not.toHaveBeenCalled();
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });

  it("CLI가 없던 pane에서 나중에 CLI가 실행되면 기본 채팅 모드로 채팅을 열고 그 선택을 저장한다", async () => {
    vi.useFakeTimers();
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    actions.getPaneChatLayout.mockImplementation(readFreshLayout);
    actions.getPaneChatAgentProvider.mockResolvedValue(null);
    await act(async () => { open(); });
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    actions.getPaneChatAgentProvider.mockResolvedValue("claude");
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    expect(actions.setPaneChatMode).toHaveBeenCalledWith("task-1", "%1", true);
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });

  it("채팅으로 저장된 pane도 CLI가 없으면 연결 화면 없이 터미널로 두고, CLI가 실행되면 채팅으로 연다", async () => {
    vi.useFakeTimers();
    savedModes["task-1"] = { "%1": true };
    actions.getPaneChatLayout.mockImplementation(readFreshLayout);
    actions.getPaneChatAgentProvider.mockResolvedValue(null);
    await act(async () => { open(); });
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    expect(screen.queryByTestId("pane-chat-loading")).toBeNull();
    actions.getPaneChatAgentProvider.mockResolvedValue("codex");
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    expect(actions.setPaneChatMode).not.toHaveBeenCalled();
  });

  it("CLI가 실행 중이어도 신뢰 확인 같은 첫 화면이라 대화를 연결할 수 없으면 채팅으로 덮지 않고, 연결할 수 있게 되면 연다", async () => {
    vi.useFakeTimers();
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    actions.getPaneChatLayout.mockImplementation(readFreshLayout);
    actions.getPaneChatSnapshot.mockResolvedValue(chatState(false, "exact-session-unavailable"));
    await act(async () => { open(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(actions.getPaneChatSnapshot).toHaveBeenCalledWith("task-1", "%1");
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    expect(screen.queryByTestId("pane-chat-loading")).toBeNull();
    expect(actions.setPaneChatMode).not.toHaveBeenCalled();
    actions.getPaneChatSnapshot.mockResolvedValue(chatState(true));
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    expect(actions.setPaneChatMode).toHaveBeenCalledWith("task-1", "%1", true);
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });

  it("Codex가 대화 ID 표시를 기다리는 상태면 채팅에서 누를 수 있게 연다", async () => {
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    actions.getPaneChatAgentProvider.mockResolvedValue("codex");
    actions.getPaneChatSnapshot.mockResolvedValue(chatState(false, "codex-thread-unconfirmed"));
    open();
    await act(async () => {});
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });

  it("CLI가 없는 pane에서는 대화 상태까지 읽지 않는다", async () => {
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    actions.getPaneChatAgentProvider.mockResolvedValue(null);
    open();
    await act(async () => {});
    expect(actions.getPaneChatAgentProvider).toHaveBeenCalledWith("task-1", "%1");
    expect(actions.getPaneChatSnapshot).not.toHaveBeenCalled();
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });

  it("직접 채팅으로 전환하면 CLI가 없는 pane도 채팅을 연다", async () => {
    actions.getPaneChatAgentProvider.mockResolvedValue(null);
    open();
    await toggle();
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
  });

  it("채팅을 연 탭을 숨겼다 다시 보이면 pane 배치를 다시 읽는 동안에만 연결 화면을 보여 준다", async () => {
    const view = open();
    await toggle();
    const layer = (isVisible: boolean) => <><div ref={view.terminalRef} data-testid="terminal" /><TerminalPaneChatLayer taskId="task-1" terminalTabId="@1" terminalRef={view.terminalRef} metrics={metrics} isVisible={isVisible} onChatFocusChange={view.onChatFocusChange} /></>;
    view.rerender(layer(false));
    let resolveLayout!: (value: typeof split) => void;
    actions.getPaneChatLayout.mockReturnValue(new Promise((accept) => { resolveLayout = accept; }));
    view.rerender(layer(true));
    await act(async () => {});
    expect(screen.getByTestId("pane-chat-loading").getAttribute("aria-busy")).toBe("true");
    await act(async () => resolveLayout(split));
    expect(screen.queryByTestId("pane-chat-loading")).toBeNull();
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
  });

  it("채팅 pane이 열려 있을 때 터미널을 눌러도 연결 화면이 뜨지 않고 채팅 pane이 그대로 보인다", async () => {
    open();
    await toggle();
    // 누른 뒤의 다시 읽기가 끝나지 않은 동안의 화면을 본다.
    actions.getPaneChatLayout.mockReturnValue(new Promise(() => {}));
    await act(async () => { fireEvent.pointerDown(screen.getByTestId("terminal")); });
    expect(screen.queryByTestId("pane-chat-loading")).toBeNull();
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
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

  it("창 저장소가 비어 있어도 앱에 저장된 채팅 모드로 pane을 채팅 UI로 연다", async () => {
    sessionStorage.clear();
    savedModes["task-1"] = { "%1": true };
    open();
    await act(async () => {});
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
  });

  it("채팅으로 바꾸면 true를, 터미널로 되돌리면 false를 pane 단위로 저장한다", async () => {
    open();
    await toggle();
    expect(actions.setPaneChatMode).toHaveBeenCalledWith("task-1", "%1", true);
    await act(async () => { fireEvent.click(screen.getByText("terminal-%1")); });
    expect(actions.setPaneChatMode).toHaveBeenLastCalledWith("task-1", "%1", false);
    expect(actions.setPaneChatMode).toHaveBeenCalledTimes(2);
  });

  it("직접 터미널로 둔 pane은 기본 채팅 모드 설정이 켜져 있어도 터미널로 남는다", async () => {
    vi.mocked(getAppSetting).mockImplementation(async (key) => key === "pane_chat_default_enabled" ? "true" : null);
    let resolveModes!: (modes: Record<string, boolean>) => void;
    actions.getPaneChatModes.mockReturnValue(new Promise((accept) => { resolveModes = accept; }));
    open();
    await act(async () => {});
    // 저장된 기록을 읽기 전에는 기본 채팅 모드를 적용하지 않는다
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    await act(async () => resolveModes({ "%1": false }));
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    expect(actions.setPaneChatMode).not.toHaveBeenCalled();
    vi.mocked(getAppSetting).mockResolvedValue(null);
  });

  it("저장된 모드를 읽지 못해도 오류 없이 터미널로 열리고 전환은 계속 동작한다", async () => {
    actions.getPaneChatModes.mockRejectedValue(new Error("settings unavailable"));
    open();
    await act(async () => {});
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByTestId("chat-%1")).toBeNull();
    await toggle();
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    expect(actions.setPaneChatMode).toHaveBeenCalledWith("task-1", "%1", true);
  });

  it.each([
    ["레이아웃이 먼저", "layout-first"],
    ["모드 조회가 먼저", "modes-first"],
  ] as const)("창을 다시 열어 복원한 활성 pane은 %s 끝나도 채팅 화면으로 포커스를 옮긴다", async (_label, order) => {
    let resolveModes!: (modes: Record<string, boolean>) => void;
    let resolveLayout!: (layout: typeof split) => void;
    actions.getPaneChatModes.mockReturnValue(new Promise((accept) => { resolveModes = accept; }));
    actions.getPaneChatLayout.mockReturnValue(new Promise((accept) => { resolveLayout = accept; }));
    const { onChatFocusChange } = open();
    await act(async () => {});
    const finishModes = () => act(async () => resolveModes({ "%1": true }));
    const finishLayout = () => act(async () => resolveLayout(split));
    if (order === "layout-first") { await finishLayout(); await finishModes(); } else { await finishModes(); await finishLayout(); }
    await act(async () => { await new Promise((done) => requestAnimationFrame(() => done(null))); });
    expect(screen.getByTestId("chat-%1").getAttribute("data-visible")).toBe("true");
    expect(onChatFocusChange).toHaveBeenCalledWith(true);
    expect(screen.getByTestId("chat-%1").parentElement!.contains(document.activeElement)).toBe(true);
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

  it("채팅 화면 안에서 우클릭하면 그 자리에 맞는 OS 글자 메뉴를 요청한다", async () => {
    const showTextContextMenu = vi.fn();
    vi.stubGlobal("kanvibeDesktop", { showTextContextMenu });
    open();
    await toggle();

    fireEvent.contextMenu(screen.getByLabelText("draft-%1"));

    expect(showTextContextMenu).toHaveBeenCalledExactlyOnceWith({ isEditable: true, hasSelection: false });
  });

  it("채팅 화면 밖 터미널 영역의 우클릭에는 글자 메뉴를 요청하지 않는다", async () => {
    const showTextContextMenu = vi.fn();
    vi.stubGlobal("kanvibeDesktop", { showTextContextMenu });
    open();
    await toggle();

    const wasDefaultAllowed = fireEvent.contextMenu(screen.getByTestId("terminal"));

    expect(wasDefaultAllowed).toBe(true);
    expect(showTextContextMenu).not.toHaveBeenCalled();
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
