import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

function readMainSource() {
  return readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");
}

describe("desktop shortcut routing", () => {
  it("routes platform-aware shortcuts through the shared shortcut matcher", () => {
    const source = readMainSource();

    expect(source).toContain("matchElectronShortcutInput");
    expect(source).toContain("DESKTOP_SHORTCUTS.createTask");
    expect(source).toContain("DESKTOP_SHORTCUTS.newWindow");
    expect(source).toContain("matchTaskDetailDockShortcutInput");
    expect(source).toContain('browserWindow.webContents.send("kanvibe:create-task-shortcut")');
    expect(source).toContain('browserWindow.webContents.send("kanvibe:task-detail-dock-shortcut"');
    expect(source).toContain("void createAppWindow(currentUrl)");
  });

  it("사용량 단축키를 태스크 상세 판정과 함께 공유 매처에 넘긴다", () => {
    const source = readMainSource();

    expect(source).toContain("resolveTaskDetailUsageShortcutInput");
    expect(source).toContain('browserWindow.webContents.send("kanvibe:task-detail-usage-shortcut")');
    /** 화면 판정 없이 매칭하면 보드에서도 줌 초기화 키를 삼켜 버린다 */
    expect(source).toMatch(
      /resolveTaskDetailUsageShortcutInput\(\s*input,\s*shortcutPlatform,\s*isTaskDetailRouteUrl\(/,
    );
  });

  it("blocks Cmd/Ctrl+R without registering a Kanvibe refresh shortcut", () => {
    const source = readMainSource();

    expect(source).toContain("isBlockedElectronShortcutInput");
    expect(source).not.toContain("kanvibe:refresh-shortcut");
    expect(source).not.toContain("isRefreshShortcut");
  });
});
