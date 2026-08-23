import { describe, it, expect, vi, beforeEach } from "vitest";
import { installOsc52ClipboardHandler, readOsc52ClipboardText } from "@/lib/terminalClipboard";

/** navigator를 클립보드 스텁으로 갈아끼우는 테스트가 있어, 실제 xterm.js를 쓰는 테스트를 위해 원본을 잡아 둔다 */
const jsdomNavigator = globalThis.navigator;

function encodeBase64Utf8(value: string): string {
  const bytes = new TextEncoder().encode(value);
  return btoa(String.fromCharCode(...bytes));
}

describe("readOsc52ClipboardText", () => {
  it("should decode a base64 clipboard payload", () => {
    // Given
    const data = `c;${encodeBase64Utf8("kanvibe")}`;

    // When
    const result = readOsc52ClipboardText(data);

    // Then
    expect(result).toBe("kanvibe");
  });

  it("should decode multibyte text without corruption", () => {
    // Given
    const data = `c;${encodeBase64Utf8("한글 복사 ✅")}`;

    // When
    const result = readOsc52ClipboardText(data);

    // Then
    expect(result).toBe("한글 복사 ✅");
  });

  it("should ignore a clipboard read request so terminal content is not sent back", () => {
    // Given
    const data = "c;?";

    // When
    const result = readOsc52ClipboardText(data);

    // Then
    expect(result).toBeNull();
  });

  it("should ignore a payload without a selection separator", () => {
    // Given
    const data = "c";

    // When
    const result = readOsc52ClipboardText(data);

    // Then
    expect(result).toBeNull();
  });

  it("should ignore an empty payload", () => {
    // Given
    const data = "c;";

    // When
    const result = readOsc52ClipboardText(data);

    // Then
    expect(result).toBeNull();
  });

  it("should ignore an undecodable payload", () => {
    // Given
    const data = "c;!!!not-base64!!!";

    // When
    const result = readOsc52ClipboardText(data);

    // Then
    expect(result).toBeNull();
  });

  it("should ignore a payload above the size limit", () => {
    // Given
    const data = `c;${"A".repeat(1_000_001)}`;

    // When
    const result = readOsc52ClipboardText(data);

    // Then
    expect(result).toBeNull();
  });
});

describe("installOsc52ClipboardHandler", () => {
  const writeText = vi.fn();

  const writeSystemClipboard = vi.fn();

  beforeEach(() => {
    writeText.mockReset();
    writeText.mockResolvedValue(undefined);
    writeSystemClipboard.mockReset();
    writeSystemClipboard.mockResolvedValue(undefined);
    Object.defineProperty(globalThis, "navigator", {
      value: { clipboard: { writeText } },
      configurable: true,
    });
    delete window.kanvibeDesktop;
  });

  function createTerminalStub() {
    let registeredHandler: ((data: string) => boolean) | null = null;
    const dispose = vi.fn();

    return {
      dispose,
      invoke: (data: string) => registeredHandler?.(data) ?? false,
      terminal: {
        parser: {
          registerOscHandler: (identifier: number, handler: (data: string) => boolean) => {
            expect(identifier).toBe(52);
            registeredHandler = handler;
            return { dispose };
          },
        },
      },
    };
  }

  it("should copy the decoded payload to the system clipboard", () => {
    // Given
    const stub = createTerminalStub();
    installOsc52ClipboardHandler(stub.terminal as never);

    // When
    const handled = stub.invoke(`c;${encodeBase64Utf8("복사 대상")}`);

    // Then
    expect(handled).toBe(true);
    expect(writeText).toHaveBeenCalledWith("복사 대상");
  });

  it("should consume a read request without touching the clipboard", () => {
    // Given
    const stub = createTerminalStub();
    installOsc52ClipboardHandler(stub.terminal as never);

    // When
    const handled = stub.invoke("c;?");

    // Then
    expect(handled).toBe(true);
    expect(writeText).not.toHaveBeenCalled();
  });

  it("should keep the terminal usable when the clipboard write is rejected", async () => {
    // Given
    const stub = createTerminalStub();
    writeText.mockRejectedValue(new Error("clipboard blocked"));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    installOsc52ClipboardHandler(stub.terminal as never);

    // When
    const handled = stub.invoke(`c;${encodeBase64Utf8("kanvibe")}`);
    await Promise.resolve();
    await Promise.resolve();

    // Then
    expect(handled).toBe(true);
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it("should copy through the desktop bridge, which is not bound to renderer document focus", async () => {
    // Given
    window.kanvibeDesktop = { writeSystemClipboard } as unknown as Window["kanvibeDesktop"];
    const stub = createTerminalStub();
    installOsc52ClipboardHandler(stub.terminal as never);

    // When
    stub.invoke(`c;${encodeBase64Utf8("데스크톱 복사")}`);
    await Promise.resolve();

    // Then
    expect(writeSystemClipboard).toHaveBeenCalledWith("데스크톱 복사");
    expect(writeText).not.toHaveBeenCalled();
  });

  it("should keep the terminal usable when the desktop bridge write is rejected", async () => {
    // Given
    window.kanvibeDesktop = { writeSystemClipboard } as unknown as Window["kanvibeDesktop"];
    writeSystemClipboard.mockRejectedValue(new Error("bridge unavailable"));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const stub = createTerminalStub();
    installOsc52ClipboardHandler(stub.terminal as never);

    // When
    const handled = stub.invoke(`c;${encodeBase64Utf8("kanvibe")}`);
    await Promise.resolve();
    await Promise.resolve();

    // Then
    expect(handled).toBe(true);
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it("should return a disposable that unregisters the handler", () => {
    // Given
    const stub = createTerminalStub();

    // When
    installOsc52ClipboardHandler(stub.terminal as never).dispose();

    // Then
    expect(stub.dispose).toHaveBeenCalled();
  });
});

/**
 * 위 describe는 parser를 스텁으로 대신해 xterm.js가 실제로 OSC 52를 넘겨주는지는 확인하지 못한다.
 * 실제 터미널이 받는 바이트열을 그대로 흘려 보내, 시퀀스 해석부터 복사까지가 끊기지 않는지 확인한다.
 */
describe("OSC 52 over a real xterm.js terminal", () => {
  beforeEach(() => {
    Object.defineProperty(globalThis, "navigator", {
      value: jsdomNavigator,
      configurable: true,
    });
  });

  const ESCAPE = String.fromCharCode(27);
  const BELL = String.fromCharCode(7);
  const STRING_TERMINATOR = `${ESCAPE}\\`;

  async function writeToRealTerminal(terminalOutput: string): Promise<ReturnType<typeof vi.fn>> {
    const writeSystemClipboard = vi.fn().mockResolvedValue(undefined);
    window.kanvibeDesktop = { writeSystemClipboard } as unknown as Window["kanvibeDesktop"];

    const { Terminal } = await import("@xterm/xterm");
    const terminal = new Terminal();
    installOsc52ClipboardHandler(terminal);
    await new Promise<void>((resolve) => terminal.write(terminalOutput, resolve));

    return writeSystemClipboard;
  }

  it("should copy a bell-terminated sequence, which is what a plain printf sends", async () => {
    // Given
    const terminalOutput = `${ESCAPE}]52;c;${encodeBase64Utf8("OSC52-OK")}${BELL}`;

    // When
    const writeSystemClipboard = await writeToRealTerminal(terminalOutput);

    // Then
    expect(writeSystemClipboard).toHaveBeenCalledWith("OSC52-OK");
  });

  it("should copy a string-terminated sequence, which is what tmux forwards", async () => {
    // Given
    const terminalOutput = `${ESCAPE}]52;c;${encodeBase64Utf8("tmux 복사")}${STRING_TERMINATOR}`;

    // When
    const writeSystemClipboard = await writeToRealTerminal(terminalOutput);

    // Then
    expect(writeSystemClipboard).toHaveBeenCalledWith("tmux 복사");
  });
});
