import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(resolve(process.cwd(), "src/styles/globals.css"), "utf8");

function readThemeVar(themeSelector: string, variableName: string): string | null {
  const block = css.match(new RegExp(`${themeSelector}\\s*\\{([\\s\\S]*?)\\n\\}`))?.[1];
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

  it("sets task session and remote tag colors by theme", () => {
    const darkThemeSelector = ':root,\\s*:root\\[data-theme="dark"\\]';

    expect(readThemeVar(":root", "--color-tag-pr-text")).toBe("var(--color-brand-primary)");
    expect(readThemeVar(darkThemeSelector, "--color-tag-session-bg")).toBe("#ffffff");
    expect(readThemeVar(darkThemeSelector, "--color-tag-session-text")).toBe("#202124");
    expect(readThemeVar(darkThemeSelector, "--color-tag-ssh-bg")).toBe("rgba(101, 208, 138, 0.16)");
    expect(readThemeVar(darkThemeSelector, "--color-tag-ssh-text")).toBe("#9be6b4");
    expect(readThemeVar(':root\\[data-theme="light"\\]', "--color-tag-session-bg")).toBe("var(--color-button-neutral)");
    expect(readThemeVar(':root\\[data-theme="light"\\]', "--color-tag-session-text")).toBe("#ffffff");
    expect(readThemeVar(':root\\[data-theme="light"\\]', "--color-tag-ssh-bg")).toBe("var(--green-50)");
    expect(readThemeVar(':root\\[data-theme="light"\\]', "--color-tag-ssh-text")).toBe("var(--green-700)");
  });
});
