import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Ctrl+V, Ctrl+Shift+V, Shift+Insert, 마우스 중간 클릭은 macOS에서 네이티브 paste DOM 이벤트를
 * 만들지 않는다. 그래서 렌더러가 keydown/auxclick 시점에 동기적으로 클립보드 이미지 유무를
 * 물어봐야 하며, 이 sendSync 채널이 끊기면 그 트리거들이 전부 조용히 무력화된다.
 */
describe("clipboard image read bridge", () => {
  const CHANNEL = "kanvibe:clipboard-read-image";

  function readElectronSource(fileName: string): string {
    return readFileSync(path.join(process.cwd(), "electron", fileName), "utf8");
  }

  it("should answer the sync channel from the main process clipboard", () => {
    // Given
    const source = readElectronSource("main.js");

    // When
    const handler = source.match(
      /ipcMain\.on\("kanvibe:clipboard-read-image",[\s\S]*?\n {2}\}\);/,
    )?.[0];

    // Then
    expect(handler).toBeDefined();
    expect(handler).toMatch(/clipboard\.readImage\(\)/);
    expect(handler).toMatch(/event\.returnValue/);
  });

  it("should expose a synchronous readClipboardImage on the desktop bridge", () => {
    // Given
    const source = readElectronSource("preload.js");

    // When
    const bridgeMethod = source.match(/readClipboardImage\(\) \{[\s\S]*?\n {2}\},/)?.[0];

    // Then
    expect(bridgeMethod).toBeDefined();
    expect(bridgeMethod).toContain(CHANNEL);
    expect(bridgeMethod).toMatch(/ipcRenderer\.sendSync/);
  });
});
