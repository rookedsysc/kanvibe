import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
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
    const { container } = render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = container.firstChild as HTMLElement;

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
    const { container } = render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = container.firstChild as HTMLElement;

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
    const { container } = render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = container.firstChild as HTMLElement;
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
    const { container } = render(<Terminal taskId="task-1" />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = container.firstChild as HTMLElement;
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
    const { container } = render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = container.firstChild as HTMLElement;

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
    const { container } = render(<Terminal taskId="task-1" isRemote />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = container.firstChild as HTMLElement;

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
    const { container } = render(<Terminal taskId="task-1" />);
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", null, 80, 24);
    });
    const target = container.firstChild as HTMLElement;

    // When
    const event = new MouseEvent("auxclick", { button: 1, bubbles: true, cancelable: true });
    target.dispatchEvent(event);

    // Then
    expect(event.defaultPrevented).toBe(false);
    expect(mockReadClipboardImage).not.toHaveBeenCalled();
  });
});
