import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Terminal from "../Terminal";

const {
  mockOpenTerminal,
  mockFocusTerminal,
  mockOnTerminalData,
  mockOnTerminalClose,
  mockWriteTerminal,
  mockResizeTerminal,
  mockCloseTerminal,
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
  mockTerminalFocus: vi.fn(),
  mockDisposeMacShiftSelectionPatch: vi.fn(),
  mockFit: vi.fn(),
}));

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

    window.kanvibeDesktop = {
      isDesktop: true,
      openTerminal: mockOpenTerminal,
      focusTerminal: mockFocusTerminal,
      onTerminalData: mockOnTerminalData,
      onTerminalClose: mockOnTerminalClose,
      writeTerminal: mockWriteTerminal,
      resizeTerminal: mockResizeTerminal,
      closeTerminal: mockCloseTerminal,
    };
  });

  it("상세 화면 진입 직후 xterm 입력 포커스만 맞춘다", async () => {
    // Given
    render(<Terminal taskId="task-1" />);

    // When
    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", 80, 24);
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
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", 80, 24);
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
    expect(mockResizeTerminal).toHaveBeenCalledWith("task-1", 80, 24);
  });

  it("active terminal focus 요청을 받으면 xterm 입력 포커스를 맞춘다", async () => {
    render(<Terminal taskId="task-1" />);

    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", 80, 24);
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
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", 80, 24);
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
});
