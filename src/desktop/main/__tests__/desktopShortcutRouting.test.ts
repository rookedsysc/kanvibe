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

  it("blocks Cmd/Ctrl+R without registering a Kanvibe refresh shortcut", () => {
    const source = readMainSource();

    expect(source).toContain("isBlockedElectronShortcutInput");
    expect(source).not.toContain("kanvibe:refresh-shortcut");
    expect(source).not.toContain("isRefreshShortcut");
  });
});
