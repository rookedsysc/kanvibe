import { render, waitFor } from "@testing-library/react";
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

  it("상세 화면 진입 직후 터미널 입력 포커스를 맞춘다", async () => {
    render(<Terminal taskId="task-1" />);

    await waitFor(() => {
      expect(mockOpenTerminal).toHaveBeenCalledWith("task-1", 80, 24);
    });

    await waitFor(() => {
      expect(mockTerminalFocus).toHaveBeenCalledTimes(1);
      expect(mockFocusTerminal).toHaveBeenCalledWith("task-1");
    });
  });
});
