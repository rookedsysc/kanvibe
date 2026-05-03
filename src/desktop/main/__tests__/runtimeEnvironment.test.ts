import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("desktop runtime environment", () => {
  it("does not chdir into app.asar in packaged builds", () => {
    const source = readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");

    expect(source).toContain("function getRuntimeWorkingDirectory()");
    expect(source).toContain("process.resourcesPath");
    expect(source).toContain("process.chdir(getRuntimeWorkingDirectory())");
  });

  it("adds common macOS local CLI paths before external terminal commands run", () => {
    const source = readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");

    expect(source).toContain("function ensureMacLocalCommandPath()");
    expect(source).toContain("/opt/homebrew/bin");
    expect(source).toContain("/usr/local/bin");
    expect(source).toContain("path.join(homeDirectory, \".cargo\", \"bin\")");
    expect(source).toContain("ensureMacLocalCommandPath()");
    expect(source.indexOf("ensureMacLocalCommandPath()")).toBeLessThan(
      source.indexOf("process.env.KANVIBE_DESKTOP"),
    );
  });
});
