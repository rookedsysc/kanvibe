import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

describe("terminal transparency styles", () => {
  it("clears xterm.js's built-in opaque viewport background when transparency is on", () => {
    const source = readFileSync(path.join(process.cwd(), "src", "styles", "globals.css"), "utf8");

    expect(source).toMatch(
      /:root\[data-terminal-transparent="true"\]\s+\.xterm-viewport\s*\{\s*background-color:\s*transparent;\s*\}/,
    );
  });
});
