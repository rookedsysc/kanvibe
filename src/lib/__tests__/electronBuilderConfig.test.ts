import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("electron-builder config", () => {
  it("does not run an unscoped native rebuild during packaging", () => {
    const source = readFileSync(path.join(process.cwd(), "electron-builder.yml"), "utf8");

    expect(source).toContain("npmRebuild: false");
  });

  it("packs app code in asar while unpacking better-sqlite3 native files", () => {
    const source = readFileSync(path.join(process.cwd(), "electron-builder.yml"), "utf8");

    expect(source).toContain("asar: true");
    expect(source).toContain("asarUnpack:");
    expect(source).toContain("node_modules/better-sqlite3/**");
  });
});
