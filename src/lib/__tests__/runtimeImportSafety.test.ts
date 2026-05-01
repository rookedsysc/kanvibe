import { readFile } from "fs/promises";
import path from "path";
import { fileURLToPath } from "url";
import { describe, expect, it } from "vitest";

const currentDir = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(currentDir, "../../..");

describe("runtime import safety", () => {
  it("런타임 라이브러리 코드에 aliased dynamic import를 두지 않는다", async () => {
    const runtimeFiles = [
      path.join(projectRoot, "src/lib/gitOperations.ts"),
    ];

    const offenders: string[] = [];

    for (const filePath of runtimeFiles) {
      const source = await readFile(filePath, "utf-8");
      const matches = source.match(/import\(\s*["']@\/[^"']+["']\s*\)/g) ?? [];

      if (matches.length > 0) {
        offenders.push(`${path.relative(projectRoot, filePath)}: ${matches.join(", ")}`);
      }
    }

    expect(offenders).toEqual([]);
  });
});
