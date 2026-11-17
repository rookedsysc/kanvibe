import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

/**
 * Electron은 우클릭 메뉴를 기본으로 띄우지 않는다. 채팅 화면의 우클릭은 렌더러가 보낸 요청을
 * 메인 프로세스가 받아 OS 메뉴로 열어야 하며, 이 채널이 끊기면 우클릭이 다시 아무 반응 없이 무시된다.
 */
describe("text context menu bridge", () => {
  const CHANNEL = "kanvibe:text-context-menu";

  function readElectronSource(fileName: string): string {
    return readFileSync(path.join(process.cwd(), "electron", fileName), "utf8");
  }

  it("should open an OS edit menu on the window that sent the request", () => {
    // Given
    const source = readElectronSource("main.js");

    // When
    const handler = source.match(
      /ipcMain\.on\("kanvibe:text-context-menu",[\s\S]*?\n {2}\}\);/,
    )?.[0];

    // Then
    expect(handler).toBeDefined();
    expect(handler).toMatch(/BrowserWindow\.fromWebContents\(event\.sender\)/);
    expect(handler).toMatch(/Menu\.buildFromTemplate/);
    expect(handler).toMatch(/\.popup\(\{ window: senderWindow \}\)/);
  });

  it("should offer copy always and cut and paste only inside an editable field", () => {
    // Given
    const source = readElectronSource("main.js");

    // When
    const handler = source.match(
      /ipcMain\.on\("kanvibe:text-context-menu",[\s\S]*?\n {2}\}\);/,
    )?.[0];

    // Then
    expect(handler).toMatch(/\{ role: "copy", enabled: hasSelection \}/);
    expect(handler).toMatch(/\{ role: "cut", visible: isEditable, enabled: hasSelection \}/);
    expect(handler).toMatch(/\{ role: "paste", visible: isEditable \}/);
    expect(handler).toMatch(/\{ role: "selectAll" \}/);
  });

  it("should expose showTextContextMenu on the desktop bridge", () => {
    // Given
    const source = readElectronSource("preload.js");

    // When
    const bridgeMethod = source.match(/showTextContextMenu\(request\) \{[\s\S]*?\n {2}\},/)?.[0];

    // Then
    expect(bridgeMethod).toBeDefined();
    expect(bridgeMethod).toContain(CHANNEL);
    expect(bridgeMethod).toMatch(/ipcRenderer\.send/);
  });
});
