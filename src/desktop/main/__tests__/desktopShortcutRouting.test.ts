import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

function readMainSource() {
  return readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");
}

describe("desktop shortcut routing", () => {
  it("routes platform-aware shortcuts through the shared shortcut matcher", () => {
    const source = readMainSource();

    expect(source).toContain("findShortcutCommandForElectronInput");
    expect(source).toContain('shortcutCommand === "createTask"');
    expect(source).toContain('shortcutCommand === "newWindow"');
    expect(source).toContain("getTaskDetailDockIndexForCommand");
    expect(source).toContain('browserWindow.webContents.send("kanvibe:create-task-shortcut")');
    expect(source).toContain('browserWindow.webContents.send("kanvibe:task-detail-dock-shortcut"');
    expect(source).toContain("void createAppWindow(currentUrl)");
  });

  it("사용량 단축키를 태스크 상세 판정과 함께 공유 매처에 넘긴다", () => {
    const source = readMainSource();

    expect(source).toContain('browserWindow.webContents.send("kanvibe:task-detail-usage-shortcut")');
    /** 화면 판정 없이 매칭하면 보드에서도 줌 초기화 키를 삼켜 버린다 */
    expect(source).toContain('shortcutCommand === "taskDetailUsage" && isTaskDetailRoute');
    expect(source).toContain("const isTaskDetailRoute = isTaskDetailRouteUrl(browserWindow.webContents.getURL());");
  });

  /** 저장된 재배정을 읽지 않으면 설정 화면에서 바꾼 단축키가 main 경로에서만 옛 조합으로 남는다 */
  it("저장된 단축키 재배정을 읽어 두고 변경 알림에 다시 읽는다", () => {
    const source = readMainSource();

    expect(source).toContain("await refreshShortcutBindings();");
    expect(source).toContain('ipcMain.on("kanvibe:shortcut-bindings-changed"');
    expect(source).toContain("getCurrentShortcutBindings()");
  });

  it("blocks Cmd/Ctrl+R without registering a Kanvibe refresh shortcut", () => {
    const source = readMainSource();

    expect(source).toContain("isBlockedElectronShortcutInput");
    expect(source).not.toContain("kanvibe:refresh-shortcut");
    expect(source).not.toContain("isRefreshShortcut");
  });
});
