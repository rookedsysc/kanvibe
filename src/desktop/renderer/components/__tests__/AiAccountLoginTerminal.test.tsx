import { act, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AiAccountLoginTerminal from "../AiAccountLoginTerminal";

const { mockRegisterOscHandler, mockDisposeOscHandler, mockFit } = vi.hoisted(() => ({
  mockRegisterOscHandler: vi.fn(),
  mockDisposeOscHandler: vi.fn(),
  mockFit: vi.fn(),
}));

let latestTerminal: MockXTerm;

class MockXTerm {
  constructor() { latestTerminal = this; }
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
  it("closes a pending login and disposes listeners immediately on unmount", async () => {
    let resolve!: (value: { ok: boolean }) => void;
    const pending = new Promise<{ ok: boolean }>((res) => { resolve = res; });
    const open = vi.mocked(window.kanvibeDesktop!.openAiAccountLogin!);
    open.mockReturnValue(pending);
    const { unmount } = render(<AiAccountLoginTerminal provider="claude" accountRoot="/tmp/pending" onExit={vi.fn()} />);
    await waitFor(() => expect(open).toHaveBeenCalled());
    unmount();
    expect(latestTerminal.dispose).toHaveBeenCalledTimes(1);
    expect(mockDisposeOscHandler).toHaveBeenCalledTimes(1);
    expect(window.kanvibeDesktop!.closeAiAccountLogin).toHaveBeenCalledWith("/tmp/pending");
    await act(async () => { resolve({ ok: true }); });
    expect(latestTerminal.focus).not.toHaveBeenCalled();
    expect(latestTerminal.dispose).toHaveBeenCalledTimes(1);
    expect(window.kanvibeDesktop!.closeAiAccountLogin).toHaveBeenCalledTimes(1);
  });

  it("disposes a rejected login request without leaving subscriptions", async () => {
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.mocked(window.kanvibeDesktop!.openAiAccountLogin!).mockRejectedValue(new Error("IPC failed"));
    const { unmount } = render(<AiAccountLoginTerminal provider="claude" accountRoot="/tmp/rejected" onExit={vi.fn()} />);
    await waitFor(() => expect(latestTerminal.dispose).toHaveBeenCalledTimes(1));
    expect(mockDisposeOscHandler).toHaveBeenCalledTimes(1);
    expect(window.kanvibeDesktop!.closeAiAccountLogin).toHaveBeenCalledWith("/tmp/rejected");
    unmount();
    expect(latestTerminal.dispose).toHaveBeenCalledTimes(1);
    log.mockRestore();
  });

});
