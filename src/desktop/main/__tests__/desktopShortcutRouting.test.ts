import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

function readMainSource() {
  return readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");
}

describe("desktop shortcut routing", () => {
  it("routes Cmd/Ctrl+N to task creation while preserving Cmd/Ctrl+Shift+N new windows", () => {
    const source = readMainSource();

    expect(source).toMatch(/const isCreateTaskShortcut =[\s\S]*!input\.shift[\s\S]*input\.key\.toLowerCase\(\) === "n";/);
    expect(source).toMatch(/const isNewWindowShortcut =[\s\S]*input\.shift[\s\S]*input\.key\.toLowerCase\(\) === "n";/);
    expect(source).toContain('browserWindow.webContents.send("kanvibe:create-task-shortcut")');
    expect(source).toContain("void createAppWindow(currentUrl)");
  });
});
