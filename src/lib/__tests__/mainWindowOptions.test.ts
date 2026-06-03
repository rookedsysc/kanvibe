import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("main window options", () => {
  it("does not set app-level minimum window dimensions", () => {
    const source = readFileSync(path.join(process.cwd(), "electron", "main.js"), "utf8");
    const options = source.match(
      /function createBrowserWindowOptions\(\) \{\n\s+return \{([\s\S]*?)\n\s+\};\n\}/,
    )?.[1];

    expect(options).toBeDefined();
    expect(options).not.toMatch(/\bmin(?:Width|Height)\s*:/);
  });
});
