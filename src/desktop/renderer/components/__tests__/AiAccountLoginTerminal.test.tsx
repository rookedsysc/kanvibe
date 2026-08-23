import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AiAccountLoginTerminal from "../AiAccountLoginTerminal";

const { mockRegisterOscHandler, mockDisposeOscHandler, mockFit } = vi.hoisted(() => ({
  mockRegisterOscHandler: vi.fn(),
  mockDisposeOscHandler: vi.fn(),
  mockFit: vi.fn(),
}));

class MockXTerm {
  cols = 80;
  rows = 24;

  loadAddon = vi.fn();
  open = vi.fn();
  write = vi.fn();
  writeln = vi.fn();
  dispose = vi.fn();
  focus = vi.fn();
  onData = vi.fn();
  parser = { registerOscHandler: mockRegisterOscHandler };
}

vi.mock("@xterm/xterm", () => ({
  Terminal: MockXTerm,
}));

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = mockFit;
  },
}));

vi.mock("@/lib/terminalMouseSelection", () => ({
  createTerminalOptions: () => ({}),
}));

describe("AiAccountLoginTerminal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockRegisterOscHandler.mockReturnValue({ dispose: mockDisposeOscHandler });

    vi.stubGlobal("ResizeObserver", class {
      observe = vi.fn();
      disconnect = vi.fn();
    });

    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });

    window.kanvibeDesktop = {
      isDesktop: true,
      openAiAccountLogin: vi.fn().mockResolvedValue({ ok: true }),
      writeAiAccountLogin: vi.fn(),
      resizeAiAccountLogin: vi.fn(),
      closeAiAccountLogin: vi.fn(),
      onAiAccountLoginData: vi.fn().mockReturnValue(vi.fn()),
      onAiAccountLoginExit: vi.fn().mockReturnValue(vi.fn()),
    } as unknown as Window["kanvibeDesktop"];
  });

  it("should handle OSC 52 so a login code copied inside the terminal reaches the system clipboard", async () => {
    // Given
    const OSC_CLIPBOARD_IDENTIFIER = 52;

    // When
    render(
      <AiAccountLoginTerminal provider="claude" accountRoot="/tmp/account" onExit={vi.fn()} />,
    );

    // Then
    await waitFor(() => {
      expect(mockRegisterOscHandler).toHaveBeenCalledWith(
        OSC_CLIPBOARD_IDENTIFIER,
        expect.any(Function),
      );
    });
  });

  it("should unregister the OSC 52 handler when the login terminal closes", async () => {
    // Given
    const { unmount } = render(
      <AiAccountLoginTerminal provider="claude" accountRoot="/tmp/account" onExit={vi.fn()} />,
    );
    await waitFor(() => {
      expect(mockRegisterOscHandler).toHaveBeenCalled();
    });

    // When
    unmount();

    // Then
    await waitFor(() => {
      expect(mockDisposeOscHandler).toHaveBeenCalled();
    });
  });
});
