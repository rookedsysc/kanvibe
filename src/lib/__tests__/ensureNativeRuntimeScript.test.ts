import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("ensure-native-runtime script", () => {
  it("verifies Node native rebuilds in a fresh process", () => {
    const source = readFileSync(
      path.join(process.cwd(), "scripts", "ensure-native-runtime.cjs"),
      "utf8",
    );

    expect(source).toContain("function verifyFreshNodeRuntimeAfterRebuild()");
    expect(source).toContain('path.join(__dirname, "verify-node-better-sqlite3.cjs")');
    expect(source).not.toContain('require("better-sqlite3")');
    expect(source).toContain('KANVIBE_NATIVE_REBUILD_ATTEMPTED: "1"');
    expect(source).toContain("process.exit(0);");
  });
});
