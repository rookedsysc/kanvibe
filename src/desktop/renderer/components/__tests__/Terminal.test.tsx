import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Terminal from "../Terminal";

function createDeferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const {
  mockOpenTerminal,
  mockFocusTerminal,
  mockOnTerminalData,
  mockOnTerminalClose,
  mockWriteTerminal,
  mockResizeTerminal,
  mockCloseTerminal,
  mockPasteImageToRemoteTerminal,
  mockReadClipboardImage,
  mockTerminalFocus,
  mockDisposeMacShiftSelectionPatch,
  mockFit,
  translations,
} = vi.hoisted(() => ({
  mockOpenTerminal: vi.fn(),
  mockFocusTerminal: vi.fn(),
  mockOnTerminalData: vi.fn(),
  mockOnTerminalClose: vi.fn(),
  mockWriteTerminal: vi.fn(),
  mockResizeTerminal: vi.fn(),
  mockCloseTerminal: vi.fn(),
  mockPasteImageToRemoteTerminal: vi.fn(),
  mockReadClipboardImage: vi.fn(),
  mockTerminalFocus: vi.fn(),
  mockDisposeMacShiftSelectionPatch: vi.fn(),
  mockFit: vi.fn(),
  translations: { current: (key: string) => key },
}));

let latestXtermInstance: MockXTerm | undefined;

function rememberLatestXtermInstance(instance: MockXTerm) {
  latestXtermInstance = instance;
}

class MockXTerm {
  cols = 80;
  rows = 24;
  options = { fontFamily: "" };

  loadAddon = vi.fn();
  open = vi.fn();
  writeln = vi.fn();
  write = vi.fn();
  dispose = vi.fn();
  focus = mockTerminalFocus;
  onData = vi.fn();
  onResize = vi.fn();
  attachCustomKeyEventHandler = vi.fn();
  parser = { registerOscHandler: vi.fn().mockReturnValue({ dispose: vi.fn() }) };

  constructor() {
    rememberLatestXtermInstance(this);
  }
}

class MockFileReader {
  result: string | null = null;
  onload: (() => void) | null = null;

  readAsDataURL(_file: File) {
    this.result = "data:image/png;base64,ZmFrZQ==";
    queueMicrotask(() => this.onload?.());
  }
}

function dispatchPasteEvent(target: HTMLElement, clipboardData: unknown): Event {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: clipboardData });
  target.dispatchEvent(event);
  return event;
}

function createImageClipboardData(file: Partial<File> = {}) {
  return {
    items: [
      {
        kind: "file",
        type: "image/png",
        getAsFile: () => file as File,
      },
    ],
  };
}

function createTextClipboardData() {
  return {
    items: [
      {
        kind: "string",
        type: "text/plain",
        getAsFile: () => null,
      },
    ],
  };
}

vi.mock("next-intl", () => ({
  useTranslations: () => translations.current,
}));

vi.mock("@xterm/xterm", () => ({
  Terminal: MockXTerm,
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = mockFit;
  },
}));

vi.mock("@xterm/addon-web-links", () => ({
  WebLinksAddon: class {},
}));

vi.mock("@/lib/terminalMouseSelection", () => ({
  createTerminalOptions: () => ({}),
  installMacShiftSelectionPatch: () => mockDisposeMacShiftSelectionPatch,
}));

describe("Desktop Terminal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    translations.current = (key: string) => key;

    Object.defineProperty(document, "fonts", {
      configurable: true,
      value: {
        add: vi.fn(),
        ready: Promise.resolve(),
      },
    });

    vi.stubGlobal(
      "FontFace",
      class {
        load() {
          return Promise.resolve(this);
        }
      },
    );

    vi.stubGlobal("ResizeObserver", class {
      observe = vi.fn();
      disconnect = vi.fn();
    });

    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });

    mockOpenTerminal.mockResolvedValue({ ok: true });
    mockOnTerminalData.mockReturnValue(vi.fn());
    mockOnTerminalClose.mockReturnValue(vi.fn());
    mockPasteImageToRemoteTerminal.mockResolvedValue({ ok: true, remotePath: "/tmp/kanvibe-paste-fixed-uuid.png" });
    mockReadClipboardImage.mockReturnValue(null);
    vi.stubGlobal("FileReader", MockFileReader);

    window.kanvibeDesktop = {
      isDesktop: true,
      openTerminal: mockOpenTerminal,
      focusTerminal: mockFocusTerminal,
      onTerminalData: mockOnTerminalData,
      onTerminalClose: mockOnTerminalClose,
      writeTerminal: mockWriteTerminal,
      resizeTerminal: mockResizeTerminal,
      closeTerminal: mockCloseTerminal,
      pasteImageToRemoteTerminal: mockPasteImageToRemoteTerminal,
      readClipboardImage: mockReadClipboardImage,
    };
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("연결 응답을 기다리는 동안 접근 가능한 로딩 상태를 표시한다", async () => {
    const pending = createDeferred<{ ok: boolean }>();
    mockOpenTerminal.mockReturnValue(pending.promise);
    const { container } = render(<Terminal taskId="loading" />);
    await waitFor(() => expect(mockOpenTerminal).toHaveBeenCalled());

    expect(screen.getByRole("status").textContent).toBe("terminalConnecting");
    expect(container.firstElementChild?.getAttribute("aria-busy")).toBe("true");
    await act(async () => { pending.resolve({ ok: true }); });
    expect(screen.queryByRole("status")).toBeNull();
    expect(container.firstElementChild?.getAttribute("aria-busy")).toBe("false");
  });

  it("채팅 pane 복귀는 키보드만 차단하고 터미널 프로토콜 응답을 유지한다", async () => {
    const invoke = vi.fn(async (_namespace: string, method: string) => method === "focusPaneChatTerminal" ? { success: true } : method === "getPaneChatLayout"
      ? { activePaneId: "tab-chat", panes: [{ id: "tab-chat", tabId: "tab-chat", bounds: null }] }
      : { taskId: "chat-task", paneId: "tab-chat", provider: null, targetToken: null, canSend: false, inputState: "unknown", screen: "", transcript: { status: "unavailable", messages: [], nextCursor: null }, modelPicker: { supported: false } });
    window.kanvibeDesktop!.invoke = invoke;
    const onHistory = vi.fn();
    const view = render(<Terminal taskId="chat-task" tabId="tab-chat" onHistory={onHistory} />);
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    await act(async () => { window.dispatchEvent(new Event("kanvibe:toggle-pane-chat")); });
    await screen.findByTestId("pane-chat");
    const terminal = latestXtermInstance!;
    view.rerender(<Terminal taskId="chat-task" tabId="tab-chat" onHistory={onHistory} isHidden />);
    view.rerender(<Terminal taskId="chat-task" tabId="tab-chat" onHistory={onHistory} />);
    await waitFor(() => expect(screen.getByTestId("pane-chat").parentElement!.style.display).toBe("flex"));
    mockTerminalFocus.mockClear();
    act(() => { window.dispatchEvent(new Event("kanvibe:request-terminal-focus")); });
    expect(mockTerminalFocus).not.toHaveBeenCalled();
    const onData = terminal.onData.mock.calls[0][0] as (bytes: string) => void;
    onData("\x1b[2;3R");
    expect(mockWriteTerminal).toHaveBeenCalledWith("chat-task", "tab-chat", "\x1b[2;3R");
    const keyHandler = terminal.attachCustomKeyEventHandler.mock.calls[0][0] as (event: KeyboardEvent) => boolean;
    expect(keyHandler(new KeyboardEvent("keydown", { key: "x" }))).toBe(false);
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    act(() => { (terminal.open.mock.calls[0][0] as HTMLElement).dispatchEvent(paste); });
    expect(paste.defaultPrevented).toBe(true);
    await act(async () => { fireEvent.click(screen.getAllByRole("button", { name: "terminal" })[0]); });
    expect(keyHandler(new KeyboardEvent("keydown", { key: "x" }))).toBe(true);
    onData("terminal typing");
    expect(mockWriteTerminal).toHaveBeenCalledWith("chat-task", "tab-chat", "terminal typing");
  });

  it("언어가 바뀌어도 연결과 스크롤백을 유지하며 화면 문구만 바꾼다", async () => {
    const pending = createDeferred<{ ok: boolean }>();
    mockOpenTerminal.mockReturnValue(pending.promise);
    const { rerender } = render(<Terminal taskId="locale" />);
    await waitFor(() => expect(mockOpenTerminal).toHaveBeenCalledTimes(1));
    const terminal = latestXtermInstance!;
    translations.current = (key: string) => `translated:${key}`;
    rerender(<Terminal taskId="locale" />);
    expect(screen.getByRole("status").textContent).toBe("translated:terminalConnecting");
    await act(async () => { pending.resolve({ ok: true }); });
    expect(mockOpenTerminal).toHaveBeenCalledTimes(1);
    expect(terminal.dispose).not.toHaveBeenCalled();
    expect(mockCloseTerminal).not.toHaveBeenCalled();
  });

  it("쿨다운 동안 실패를 표시하지 않고 지정한 시간 뒤 한 번 재연결한다", async () => {
    const pending = createDeferred<{ ok: boolean; error: string; retryAfterMs: number }>();
    mockOpenTerminal.mockReturnValueOnce(pending.promise);
    render(<Terminal taskId="cooldown" />);
    await waitFor(() => expect(mockOpenTerminal).toHaveBeenCalledTimes(1));
    vi.useFakeTimers();
    await act(async () => { pending.resolve({ ok: false, error: "SSH cooldown", retryAfterMs: 1200 }); });
    expect(screen.getByRole("status").textContent).toBe("terminalReconnecting");
    expect(latestXtermInstance?.writeln).not.toHaveBeenCalled();
    act(() => mockOnTerminalClose.mock.calls[0][0]({ taskId: "cooldown", tabId: null, reason: "SSH cooldown" }));
    expect(latestXtermInstance?.writeln).not.toHaveBeenCalled();

    await act(async () => { await vi.advanceTimersByTimeAsync(1199); });
    expect(mockOpenTerminal).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(mockOpenTerminal).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("status")).toBeNull();
    expect(mockTerminalFocus).toHaveBeenCalledTimes(1);
  });

  it("재연결도 실패하면 반복하지 않고 오류와 수동 재시도를 보여 준다", async () => {
    mockOpenTerminal.mockResolvedValue({ ok: false, error: "SSH cooldown", retryAfterMs: 1 });
    render(<Terminal taskId="exhausted" />);
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("SSH cooldown"));
    expect(mockOpenTerminal).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("status")).toBeNull();

    mockOpenTerminal.mockResolvedValue({ ok: true });
    fireEvent.click(screen.getByRole("button", { name: "terminalRetry" }));
    await waitFor(() => expect(mockOpenTerminal).toHaveBeenCalledTimes(3));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(mockCloseTerminal).toHaveBeenCalledTimes(1);
  });

  it("재시도 힌트가 없는 실제 오류는 즉시 표시하고 자동 재시도하지 않는다", async () => {
    mockOpenTerminal.mockResolvedValue({ ok: false, error: "tmux missing" });
    render(<Terminal taskId="permanent" />);
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("tmux missing"));
    expect(mockOpenTerminal).toHaveBeenCalledTimes(1);
    expect(latestXtermInstance?.writeln).toHaveBeenCalledWith("\r\n\x1b[31mtmux missing\x1b[0m");
  });

  it("재연결 대기 중 언마운트하면 예약을 취소하고 자원을 한 번만 정리한다", async () => {
    const pending = createDeferred<{ ok: boolean; error: string; retryAfterMs: number }>();
    mockOpenTerminal.mockReturnValueOnce(pending.promise);
    const { unmount } = render(<Terminal taskId="cancel" />);
    await waitFor(() => expect(mockOpenTerminal).toHaveBeenCalledTimes(1));
    vi.useFakeTimers();
    await act(async () => { pending.resolve({ ok: false, error: "SSH cooldown", retryAfterMs: 1000 }); });
    const terminal = latestXtermInstance!;
    unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(mockOpenTerminal).toHaveBeenCalledTimes(1);
    expect(mockCloseTerminal).toHaveBeenCalledTimes(1);
    expect(terminal.dispose).toHaveBeenCalledTimes(1);
  });

  it("숨긴 탭의 예약 재연결을 취소하고 다시 표시하면 수동 재시도를 제공한다", async () => {
    const pending = createDeferred<{ ok: boolean; error: string; retryAfterMs: number }>();
    mockOpenTerminal.mockReturnValueOnce(pending.promise);
    const { rerender } = render(<Terminal taskId="hidden" tabId="tab-1" />);
    await waitFor(() => expect(mockOpenTerminal).toHaveBeenCalledTimes(1));
    vi.useFakeTimers();
    await act(async () => { pending.resolve({ ok: false, error: "SSH cooldown", retryAfterMs: 1000 }); });
    rerender(<Terminal taskId="hidden" tabId="tab-1" isHidden />);
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(mockOpenTerminal).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("status")).toBeNull();
    expect(latestXtermInstance?.dispose).not.toHaveBeenCalled();
    rerender(<Terminal taskId="hidden" tabId="tab-1" />);
    expect(screen.getByRole("alert").textContent).toBe("terminalReconnectPaused");
    vi.useRealTimers();
    fireEvent.click(screen.getByRole("button", { name: "terminalRetry" }));
    await waitFor(() => expect(mockOpenTerminal).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it.each([
    { taskId: "other-task", tabId: "tab-1" },
    { taskId: "switch", tabId: "tab-2" },
    { taskId: "switch", tabId: "tab-1", sessionKey: "tmux:new-session" },
  ])("작업·탭·세션 변경은 이전 연결의 예약 재시도를 취소한다: %j", async (next) => {
    const pending = createDeferred<{ ok: boolean; error: string; retryAfterMs: number }>();
    mockOpenTerminal.mockReturnValueOnce(pending.promise);
    const { rerender } = render(<Terminal taskId="switch" tabId="tab-1" />);
    await waitFor(() => expect(mockOpenTerminal).toHaveBeenCalledTimes(1));
    vi.useFakeTimers();
    await act(async () => { pending.resolve({ ok: false, error: "SSH cooldown", retryAfterMs: 1000 }); });
    rerender(<Terminal key={next.sessionKey} taskId={next.taskId} tabId={next.tabId} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(mockOpenTerminal.mock.calls.map(([taskId, tabId]) => [taskId, tabId])).toEqual([
      ["switch", "tab-1"], [next.taskId, next.tabId],
    ]);
    expect(mockCloseTerminal).toHaveBeenCalledWith("switch", "tab-1");
  });

  function getRegisteredKeyEventHandler(): (event: Partial<KeyboardEvent> & { type: string }) => boolean {
    const handler = latestXtermInstance?.attachCustomKeyEventHandler.mock.calls[0]?.[0];
    if (!handler) {
      throw new Error("attachCustomKeyEventHandler가 등록되지 않았습니다.");
    }
    return handler;
  }

  it("상세 화면 진입 직후 xterm 입력 포커스만 맞춘다", async () => {
    // Given
    render(<Terminal taskId="task-1" />);

    // When
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });

    // Then
    await waitFor(() => {
      expect(mockTerminalFocus).toHaveBeenCalledTimes(1);
    });
    expect(mockFocusTerminal).not.toHaveBeenCalled();
  });

  it("상세 창으로 다시 포커스되면 terminal fit과 resize를 다시 실행한다", async () => {
    // Given
    render(<Terminal taskId="task-1" />);

    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });

    mockFit.mockClear();
    mockResizeTerminal.mockClear();

    // When
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });

    // Then
    await waitFor(() => {
      expect(mockFit).toHaveBeenCalledTimes(1);
    });
    expect(mockResizeTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
  });

  it("active terminal focus 요청을 받으면 xterm 입력 포커스를 맞춘다", async () => {
    render(<Terminal taskId="task-1" />);

    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    mockTerminalFocus.mockClear();

    await act(async () => {
      window.dispatchEvent(new Event("kanvibe:request-terminal-focus"));
    });

    await waitFor(() => {
      expect(mockTerminalFocus).toHaveBeenCalledTimes(1);
    });
  });

  it("terminal focus blocker가 열려 있으면 active terminal focus 요청을 무시한다", async () => {
    render(<Terminal taskId="task-1" />);

    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    mockTerminalFocus.mockClear();

    const blocker = document.createElement("div");
    blocker.setAttribute("data-terminal-focus-blocker", "true");
    document.body.appendChild(blocker);

    await act(async () => {
      window.dispatchEvent(new Event("kanvibe:request-terminal-focus"));
    });

    expect(mockTerminalFocus).not.toHaveBeenCalled();
    blocker.remove();
  });

  it("Nerd Font 로딩이 느려도 터미널 연결을 먼저 시작한다", async () => {
    // Given
    const fontsReady = createDeferred<void>();
    const fontLoaded = createDeferred<FontFace>();
    Object.defineProperty(document, "fonts", {
      configurable: true,
      value: {
        add: vi.fn(),
        ready: fontsReady.promise,
      },
    });
    vi.stubGlobal(
      "FontFace",
      class {
        load() {
          return fontLoaded.promise;
        }
      },
    );

    // When
    render(<Terminal taskId="task-1" />);

    // Then
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
  });

  it("원격 세션에서 이미지가 담긴 붙여넣기를 가로채 원격 경로로 대체한다", async () => {
    // Given
    render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = latestXtermInstance!.open.mock.calls[0][0] as HTMLElement;

    // When
    const event = dispatchPasteEvent(target, createImageClipboardData());

    // Then
    expect(event.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect(mockPasteImageToRemoteTerminal).toHaveBeenCalledWith("task-1", "data:image/png;base64,ZmFrZQ==");
    });
    await waitFor(() => {
      expect(mockWriteTerminal).toHaveBeenCalledWith("task-1", null, "/tmp/kanvibe-paste-fixed-uuid.png");
    });
  });

  it("원격 세션에서 이미지 전송이 실패하면 터미널에 에러를 표시한다", async () => {
    // Given
    mockPasteImageToRemoteTerminal.mockResolvedValue({ ok: false, error: "원격 세션이 아닙니다." });
    render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = latestXtermInstance!.open.mock.calls[0][0] as HTMLElement;

    // When
    dispatchPasteEvent(target, createImageClipboardData());

    // Then
    await waitFor(() => {
      expect(latestXtermInstance?.writeln).toHaveBeenCalledWith("\r\n\x1b[31m원격 세션이 아닙니다.\x1b[0m");
    });
    expect(mockWriteTerminal).not.toHaveBeenCalled();
  });

  it("클립보드에 이미지가 없는 텍스트 붙여넣기는 원격 세션이어도 그대로 흘려보낸다", async () => {
    // Given
    render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = latestXtermInstance!.open.mock.calls[0][0] as HTMLElement;
    const bubbleSpy = vi.fn();
    document.body.addEventListener("paste", bubbleSpy);

    // When
    const event = dispatchPasteEvent(target, createTextClipboardData());

    // Then
    expect(event.defaultPrevented).toBe(false);
    expect(bubbleSpy).toHaveBeenCalledTimes(1);
    expect(mockPasteImageToRemoteTerminal).not.toHaveBeenCalled();
    document.body.removeEventListener("paste", bubbleSpy);
  });

  it("로컬 세션에서는 이미지 붙여넣기도 가로채지 않는다", async () => {
    // Given
    render(<Terminal taskId="task-1" />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = latestXtermInstance!.open.mock.calls[0][0] as HTMLElement;
    const bubbleSpy = vi.fn();
    document.body.addEventListener("paste", bubbleSpy);

    // When
    const event = dispatchPasteEvent(target, createImageClipboardData());

    // Then
    expect(event.defaultPrevented).toBe(false);
    expect(bubbleSpy).toHaveBeenCalledTimes(1);
    expect(mockPasteImageToRemoteTerminal).not.toHaveBeenCalled();
    document.body.removeEventListener("paste", bubbleSpy);
  });

  it("원격 세션에서 Ctrl+V 시 클립보드에 이미지가 있으면 xterm 기본 처리를 막고 이미지를 전송한다", async () => {
    // Given
    mockReadClipboardImage.mockReturnValue("data:image/png;base64,ZmFrZQ==");
    render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const handler = getRegisteredKeyEventHandler();

    // When
    const handled = handler({ type: "keydown", ctrlKey: true, metaKey: false, shiftKey: false, altKey: false, key: "v" });

    // Then
    expect(handled).toBe(false);
    await waitFor(() => {
      expect(mockPasteImageToRemoteTerminal).toHaveBeenCalledWith("task-1", "data:image/png;base64,ZmFrZQ==");
    });
  });

  it("원격 세션에서 Ctrl+V 시 클립보드에 이미지가 없으면 xterm 기본 처리를 그대로 둔다 (vim 비주얼 블록 등 회귀 방지)", async () => {
    // Given
    mockReadClipboardImage.mockReturnValue(null);
    render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const handler = getRegisteredKeyEventHandler();

    // When
    const handled = handler({ type: "keydown", ctrlKey: true, metaKey: false, shiftKey: false, altKey: false, key: "v" });

    // Then
    expect(handled).toBe(true);
    expect(mockPasteImageToRemoteTerminal).not.toHaveBeenCalled();
  });

  it("로컬 세션에서는 Ctrl+V가 이미지가 있어도 가로채지 않는다", async () => {
    // Given
    mockReadClipboardImage.mockReturnValue("data:image/png;base64,ZmFrZQ==");
    render(<Terminal taskId="task-1" />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const handler = getRegisteredKeyEventHandler();

    // When
    const handled = handler({ type: "keydown", ctrlKey: true, metaKey: false, shiftKey: false, altKey: false, key: "v" });

    // Then
    expect(handled).toBe(true);
    expect(mockReadClipboardImage).not.toHaveBeenCalled();
  });

  it("원격 세션에서 Ctrl+Shift+V와 Shift+Insert도 이미지 붙여넣기를 트리거한다", async () => {
    // Given
    mockReadClipboardImage.mockReturnValue("data:image/png;base64,ZmFrZQ==");
    render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const handler = getRegisteredKeyEventHandler();

    // When
    const ctrlShiftVHandled = handler({ type: "keydown", ctrlKey: true, metaKey: false, shiftKey: true, altKey: false, key: "v" });
    const shiftInsertHandled = handler({ type: "keydown", ctrlKey: false, metaKey: false, shiftKey: true, altKey: false, key: "Insert" });

    // Then
    expect(ctrlShiftVHandled).toBe(false);
    expect(shiftInsertHandled).toBe(false);
    expect(mockPasteImageToRemoteTerminal).toHaveBeenCalledTimes(2);
  });

  it("원격 세션에서 마우스 중간 클릭 시 클립보드 이미지를 전송한다", async () => {
    // Given
    mockReadClipboardImage.mockReturnValue("data:image/png;base64,ZmFrZQ==");
    render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = latestXtermInstance!.open.mock.calls[0][0] as HTMLElement;

    // When
    const event = new MouseEvent("auxclick", { button: 1, bubbles: true, cancelable: true });
    target.dispatchEvent(event);

    // Then
    expect(event.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect(mockPasteImageToRemoteTerminal).toHaveBeenCalledWith("task-1", "data:image/png;base64,ZmFrZQ==");
    });
  });

  it("원격 세션에서 마우스 중간 클릭 시 클립보드에 이미지가 없으면 아무 것도 하지 않는다", async () => {
    // Given
    mockReadClipboardImage.mockReturnValue(null);
    render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = latestXtermInstance!.open.mock.calls[0][0] as HTMLElement;

    // When
    const event = new MouseEvent("auxclick", { button: 1, bubbles: true, cancelable: true });
    target.dispatchEvent(event);

    // Then
    expect(event.defaultPrevented).toBe(false);
    expect(mockPasteImageToRemoteTerminal).not.toHaveBeenCalled();
  });

  it("로컬 세션에서는 마우스 중간 클릭이 이미지가 있어도 가로채지 않는다", async () => {
    // Given
    mockReadClipboardImage.mockReturnValue("data:image/png;base64,ZmFrZQ==");
    render(<Terminal taskId="task-1" />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = latestXtermInstance!.open.mock.calls[0][0] as HTMLElement;

    // When
    const event = new MouseEvent("auxclick", { button: 1, bubbles: true, cancelable: true });
    target.dispatchEvent(event);

    // Then
    expect(event.defaultPrevented).toBe(false);
    expect(mockReadClipboardImage).not.toHaveBeenCalled();
  });
  it("disposes all resources when unmounted while openTerminal is pending", async () => {
    const pending = createDeferred<{ ok: boolean }>();
    mockOpenTerminal.mockReturnValue(pending.promise);
    const unsubscribeData = vi.fn();
    const unsubscribeClose = vi.fn();
    mockOnTerminalData.mockReturnValue(unsubscribeData);
    mockOnTerminalClose.mockReturnValue(unsubscribeClose);
    const { unmount } = render(<Terminal taskId="pending" />);
    await waitFor(() => expect(mockOpenTerminal).toHaveBeenCalled());
    const terminal = latestXtermInstance!;
    unmount();
    expect(terminal.dispose).toHaveBeenCalledTimes(1);
    expect(unsubscribeData).toHaveBeenCalledTimes(1);
    expect(unsubscribeClose).toHaveBeenCalledTimes(1);
    expect(mockCloseTerminal).toHaveBeenCalledWith("pending", null);
    await act(async () => { pending.resolve({ ok: true }); });
    expect(terminal.focus).not.toHaveBeenCalled();
    expect(terminal.dispose).toHaveBeenCalledTimes(1);
    expect(mockCloseTerminal).toHaveBeenCalledTimes(1);
  });

  it("cleans up a successful connection if observer setup throws", async () => {
    const observe = vi.fn(() => { throw new Error("observer setup"); });
    const disconnect = vi.fn();
    vi.stubGlobal("ResizeObserver", class { observe = observe; disconnect = disconnect; });
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    const { unmount } = render(<Terminal taskId="observer-error" />);
    await waitFor(() => expect(observe).toHaveBeenCalled());
    expect(mockCloseTerminal).toHaveBeenCalledWith("observer-error", null);
    expect(latestXtermInstance!.dispose).toHaveBeenCalledTimes(1);
    expect(disconnect).toHaveBeenCalledTimes(1);
    unmount();
    expect(latestXtermInstance!.dispose).toHaveBeenCalledTimes(1);
    log.mockRestore();
  });

});
