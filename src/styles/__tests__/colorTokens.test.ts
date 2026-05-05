import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

function readProjectFile(path: string) {
  return readFileSync(join(process.cwd(), path), "utf-8");
}

describe("UI color tokens", () => {
  it("keeps point and neutral button colors behind semantic CSS tokens", () => {
    const css = readProjectFile("src/styles/globals.css");

    expect(css).toContain("--point-blue: #0064FF;");
    expect(css).toContain("--button-gray: #202632;");
    expect(css).toContain("--color-brand-primary: var(--point-blue);");
    expect(css).toContain("--color-tag-pr-text: var(--color-brand-primary);");
    expect(css).toContain("--color-tag-project-bg: var(--color-button-neutral);");
    expect(css).toContain("--color-tag-base-bg: var(--color-button-neutral);");
  });

  it("documents color token usage in agent conventions", () => {
    const conventions = readProjectFile("CLAUDE.md");

    expect(conventions).toContain("#0064FF");
    expect(conventions).toContain("#202632");
    expect(conventions).toContain("--color-brand-primary");
    expect(conventions).toContain("--color-button-neutral-*");
  });
});
