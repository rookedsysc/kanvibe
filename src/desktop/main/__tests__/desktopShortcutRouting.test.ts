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

  /**
   * 녹화 화면은 main이 가로챈 조합을 영영 받지 못한다.
   * 그러면 그 조합의 원래 동작(새 창, 창 닫기)만 실행되고 녹화는 실패한다.
   */
  it("녹화 중인 창에서는 명령을 찾기 전에 입력을 렌더러로 흘려보낸다", () => {
    const source = readMainSource();

    expect(source).toContain('ipcMain.on("kanvibe:shortcut-capture-changed"');
    expect(source).toContain("shortcutCapturingWebContentsIds.add(event.sender.id)");
    expect(source).toContain("shortcutCapturingWebContentsIds.delete(event.sender.id)");

    const captureGuardIndex = source.indexOf(
      "if (shortcutCapturingWebContentsIds.has(browserWindow.webContents.id))",
    );
    const commandLookupIndex = source.indexOf("const shortcutCommand = findShortcutCommandForElectronInput(");
    expect(captureGuardIndex).toBeGreaterThan(-1);
    /** 명령을 찾은 뒤에 검사하면 이미 preventDefault가 걸린 뒤라 늦다 */
    expect(captureGuardIndex).toBeLessThan(commandLookupIndex);
  });

  /** 창이 녹화 중인 채로 닫히면 그 id가 남아, 나중에 같은 id를 받은 창이 단축키를 통째로 잃는다 */
  it("창이 닫히면 녹화 중 표시를 지운다", () => {
    const source = readMainSource();

    const closeHandlerIndex = source.indexOf('browserWindow.on("closed"');
    expect(closeHandlerIndex).toBeGreaterThan(-1);

    /** 같은 정리 줄을 쓰는 did-navigate 핸들러가 대신 걸리면 이 정리가 사라져도 통과한다 */
    const closeHandlerBody = source.slice(
      closeHandlerIndex,
      source.indexOf("\n}", closeHandlerIndex),
    );
    expect(closeHandlerBody).toContain(
      "shortcutCapturingWebContentsIds.delete(shortcutCapturingWebContentsId)",
    );
  });

  /**
   * 문서를 다시 로드하면 렌더러 녹화 상태는 사라지는데 effect cleanup은 돌지 않아 해제 알림이 안 나간다.
   * 창은 살아 있으니 closed도 오지 않아, 표시가 남은 그 창은 main 단축키 가로채기를 통째로 잃는다.
   */
  it("문서를 다시 로드해도 녹화 중 표시를 지운다", () => {
    const source = readMainSource();

    const navigationHandlerIndex = source.indexOf('browserWindow.webContents.on("did-navigate"');
    expect(navigationHandlerIndex).toBeGreaterThan(-1);

    const navigationHandlerBody = source.slice(
      navigationHandlerIndex,
      source.indexOf('browserWindow.on("closed"', navigationHandlerIndex),
    );
    expect(navigationHandlerBody).toContain(
      "shortcutCapturingWebContentsIds.delete(shortcutCapturingWebContentsId)",
    );
  });

  /** 저장한 창만 갱신하면 다른 창은 재시작 전까지 옛 조합과 새 조합이 섞인 상태로 남는다 */
  it("단축키 재배정을 다시 읽은 뒤 열려 있는 모든 창에 알린다", () => {
    const source = readMainSource();

    expect(source).toContain("void refreshShortcutBindings().then(broadcastShortcutBindingsChanged);");
    expect(source).toContain("function broadcastShortcutBindingsChanged()");

    const broadcastBody = source.slice(
      source.indexOf("function broadcastShortcutBindingsChanged()"),
    );
    expect(broadcastBody).toContain("BrowserWindow.getAllWindows()");
    expect(broadcastBody).toContain('window.webContents.send("kanvibe:shortcut-bindings-changed")');
  });

  it("blocks Cmd/Ctrl+R without registering a Kanvibe refresh shortcut", () => {
    const source = readMainSource();

    expect(source).toContain("isBlockedElectronShortcutInput");
    expect(source).not.toContain("kanvibe:refresh-shortcut");
    expect(source).not.toContain("isRefreshShortcut");
  });
});
