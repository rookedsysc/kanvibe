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
});
