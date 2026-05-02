import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("package scripts", () => {
  it("rebuilds native dependencies for Electron after preparing the Node seed database", () => {
    const packageJson = JSON.parse(
      readFileSync(path.join(process.cwd(), "package.json"), "utf8"),
    ) as { scripts: Record<string, string> };

    expect(packageJson.scripts["rebuild:native:electron"]).toBe(
      "electron-rebuild -f --build-from-source --only better-sqlite3",
    );
    expect(packageJson.scripts.dist).toContain(
      "pnpm db:prepare && pnpm build && pnpm rebuild:native:electron && electron-builder",
    );
    expect(packageJson.scripts["dist:dir"]).toContain(
      "pnpm db:prepare && pnpm build && pnpm rebuild:native:electron && electron-builder",
    );
  });
});
