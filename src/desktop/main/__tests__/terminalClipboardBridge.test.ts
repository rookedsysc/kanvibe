import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

/**
 * OSC 52 복사는 메인 프로세스 클립보드를 거쳐야 문서 포커스와 무관하게 반영된다.
 * 두 파일 중 한쪽만 남으면 복사가 조용히 사라지므로, 채널 양끝이 이어져 있는지 확인한다.
 */
describe("terminal clipboard bridge", () => {
  const CLIPBOARD_CHANNEL = "kanvibe:clipboard-write";

  function readElectronSource(fileName: string): string {
    return readFileSync(path.join(process.cwd(), "electron", fileName), "utf8");
  }

  it("should write to the main process clipboard when the renderer requests a copy", () => {
    // Given
    const source = readElectronSource("main.js");

    // When
    const handler = source.match(
      /ipcMain\.handle\("kanvibe:clipboard-write",[\s\S]*?\n {2}\}\);/,
    )?.[0];

    // Then
    expect(handler).toBeDefined();
    expect(handler).toMatch(/clipboard\.writeText\(/);
  });

  it("should expose the clipboard write on the desktop bridge", () => {
    // Given
    const source = readElectronSource("preload.js");

    // When
    const bridgeMethod = source.match(/writeSystemClipboard\(text\) \{[\s\S]*?\n {2}\},/)?.[0];

    // Then
    expect(bridgeMethod).toBeDefined();
    expect(bridgeMethod).toContain(CLIPBOARD_CHANNEL);
  });
});
