import { describe, it, expect, vi, beforeEach } from "vitest";
import { installOsc52ClipboardHandler, readOsc52ClipboardText } from "@/lib/terminalClipboard";

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

  beforeEach(() => {
    writeText.mockReset();
    writeText.mockResolvedValue(undefined);
    Object.defineProperty(globalThis, "navigator", {
      value: { clipboard: { writeText } },
      configurable: true,
    });
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

  it("should return a disposable that unregisters the handler", () => {
    // Given
    const stub = createTerminalStub();

    // When
    installOsc52ClipboardHandler(stub.terminal as never).dispose();

    // Then
    expect(stub.dispose).toHaveBeenCalled();
  });
});
