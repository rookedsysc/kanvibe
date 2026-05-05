import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(resolve(process.cwd(), "src/styles/globals.css"), "utf8");

function readThemeVar(themeSelector: string, variableName: string): string | null {
  const block = css.match(new RegExp(`${themeSelector} \\{([\\s\\S]*?)\\n\\}`))?.[1];
  return block?.match(new RegExp(`${variableName}:\\s*([^;]+);`))?.[1].trim() ?? null;
}

describe("globals.css theme tokens", () => {
  it("keeps light theme project point tags on neutral button tokens", () => {
    expect(readThemeVar(":root", "--button-gray")).toBe("#202632");
    expect(readThemeVar(':root\\[data-theme="light"\\]', "--color-tag-project-bg")).toBe("var(--color-button-neutral)");
    expect(readThemeVar(':root\\[data-theme="light"\\]', "--color-tag-project-text")).toBe("#ffffff");
    expect(readThemeVar(':root\\[data-theme="light"\\]', "--color-tag-base-bg")).toBe("var(--color-button-neutral)");
    expect(readThemeVar(':root\\[data-theme="light"\\]', "--color-tag-base-text")).toBe("#ffffff");
  });
});
