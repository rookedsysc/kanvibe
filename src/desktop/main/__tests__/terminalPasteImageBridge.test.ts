import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

/**
 * 원격 터미널 이미지 붙여넣기는 렌더러(preload) → IPC 채널 → main의 pasteImageToRemoteTerminal로 이어진다.
 * 세 조각 중 하나만 어긋나도 조용히 실패하므로, 채널 이름과 배선이 맞는지 소스 텍스트로 확인한다.
 */
describe("terminal paste-image bridge", () => {
  const PASTE_IMAGE_CHANNEL = "kanvibe:terminal-paste-image";

  function readElectronSource(fileName: string): string {
    return readFileSync(path.join(process.cwd(), "electron", fileName), "utf8");
  }

  it("should route the paste-image channel to pasteImageToRemoteTerminal in main", () => {
    // Given
    const source = readElectronSource("main.js");

    // When
    const handler = source.match(
      /ipcMain\.handle\("kanvibe:terminal-paste-image",[\s\S]*?\n {2}\}\);/,
    )?.[0];

    // Then
    expect(handler).toBeDefined();
    expect(handler).toMatch(/pasteImageToRemoteTerminal\(/);
    expect(source).toMatch(/pasteImageToRemoteTerminal,?\s*\n[\s\S]*?terminalBridge\.ts/);
  });

  it("should expose pasteImageToRemoteTerminal on the desktop bridge", () => {
    // Given
    const source = readElectronSource("preload.js");

    // When
    const bridgeMethod = source.match(/pasteImageToRemoteTerminal\(taskId, imageDataUrl\) \{[\s\S]*?\n {2}\},/)?.[0];

    // Then
    expect(bridgeMethod).toBeDefined();
    expect(bridgeMethod).toContain(PASTE_IMAGE_CHANNEL);
  });
});
