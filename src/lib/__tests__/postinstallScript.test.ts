import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("postinstall script", () => {
  it("limits Electron native rebuilds to better-sqlite3", () => {
    const source = readFileSync(
      path.join(process.cwd(), "scripts", "postinstall.cjs"),
      "utf8",
    );

    expect(source).toContain('"--only", "better-sqlite3"');
    expect(source).toContain("--only better-sqlite3");
    expect(source).not.toContain("--build-from-source");
    expect(source).not.toContain('"-w", "better-sqlite3"');
    expect(source).not.toContain("-w better-sqlite3");
  });
});
