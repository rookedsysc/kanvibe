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

  it("rebuilds Electron native dependencies without forcing source compilation", () => {
    // Given
    const packageJson = JSON.parse(
      readFileSync(path.join(process.cwd(), "package.json"), "utf8"),
    ) as { scripts: Record<string, string> };

    // When / Then
    expect(packageJson.scripts["rebuild:native:electron"]).toBe(
      "electron-rebuild -f --only better-sqlite3",
    );
    expect(packageJson.scripts["rebuild:native:electron"]).not.toContain("--build-from-source");
    expect(packageJson.scripts.dist).toContain(
      "pnpm db:prepare && pnpm build && pnpm rebuild:native:electron && electron-builder",
    );
    expect(packageJson.scripts["dist:dir"]).toContain(
      "pnpm db:prepare && pnpm build && pnpm rebuild:native:electron && electron-builder",
    );
  });

  it("starts the desktop Vite dev server with polling watchers", () => {
    const source = readFileSync(
      path.join(process.cwd(), "scripts", "run-desktop-dev.cjs"),
      "utf8",
    );

    expect(source).toContain("CHOKIDAR_USEPOLLING");
    expect(source).toContain('process.env.CHOKIDAR_USEPOLLING ?? "true"');
    expect(source).toContain("...VITE_DEV_SERVER_ENV");
  });
});
