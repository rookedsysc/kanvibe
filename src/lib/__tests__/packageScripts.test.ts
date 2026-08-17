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

  it("모든 Electron QA 진입 스크립트가 스스로 실행 준비를 보장한다", () => {
    // Given: pnpm 스크립트를 거치지 않고 bash로 직접 불러도 준비가 빠지면 안 되는 진입점들
    const entryScripts = [
      "qa-electron.sh",
      "qa-electron-video.sh",
      "qa-electron-flow-video.sh",
      "qa-move-autocomplete-video.sh",
      "qa-task-kind-filter-video.sh",
      "qa-ai-session-history-video.sh",
    ];

    // When / Then
    for (const entryScript of entryScripts) {
      const source = readFileSync(path.join(process.cwd(), "scripts", entryScript), "utf8");

      expect(source, `${entryScript}가 준비 헬퍼를 불러오지 않는다`)
        .toContain('source "$ROOT_DIR/scripts/qa-electron-prepare.sh"');
      expect(source, `${entryScript}가 준비를 실행하지 않는다`)
        .toContain('ensure_qa_electron_prepared "${1:-}"');
    }
  });

  it("준비를 진입 스크립트에 맡겼으므로 qa 스크립트가 준비를 중복 실행하지 않는다", () => {
    // Given
    const packageJson = JSON.parse(
      readFileSync(path.join(process.cwd(), "package.json"), "utf8"),
    ) as { scripts: Record<string, string> };

    // When
    const duplicated = Object.entries(packageJson.scripts)
      .filter(([name, command]) => name !== "qa:electron:prepare" && command.includes("qa:electron:prepare"))
      .map(([name]) => name);

    // Then
    expect(duplicated).toEqual([]);
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
