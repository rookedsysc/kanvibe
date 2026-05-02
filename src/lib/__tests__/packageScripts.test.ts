import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("package scripts", () => {
  it("should configure the packaged app product name as KanVibe", () => {
    // Given
    const electronBuilderConfig = readFileSync(
      path.join(process.cwd(), "electron-builder.yml"),
      "utf8",
    );

    // When / Then
    expect(electronBuilderConfig).toContain("productName: KanVibe");
    expect(electronBuilderConfig).not.toContain("productName: Kanivibe");
  });

  it("rebuilds native dependencies for Electron after preparing the Node seed database", () => {
    // Given
    const packageJson = JSON.parse(
      readFileSync(path.join(process.cwd(), "package.json"), "utf8"),
    ) as { scripts: Record<string, string> };

    // When / Then
    expect(packageJson.scripts["rebuild:native:electron"]).toBe(
      "electron-rebuild -f --build-from-source -w better-sqlite3",
    );
    expect(packageJson.scripts.dist).toContain(
      "pnpm db:prepare && pnpm build && pnpm rebuild:native:electron && electron-builder",
    );
    expect(packageJson.scripts["dist:dir"]).toContain(
      "pnpm db:prepare && pnpm build && pnpm rebuild:native:electron && electron-builder",
    );
  });
});
