import { describe, expect, it } from "vitest";
import { summarizeDiffFiles } from "@/desktop/shared/taskDiffStats";
import type { DiffFile } from "@/desktop/main/services/diffService";

function createDiffFile(overrides: Partial<DiffFile> = {}): DiffFile {
  return {
    path: "src/app.ts",
    status: "modified",
    additions: 0,
    deletions: 0,
    ...overrides,
  };
}

describe("summarizeDiffFiles", () => {
  it("파일 수와 추가·삭제 줄 수를 합산한다", () => {
    const files = [
      createDiffFile({ path: "src/a.ts", additions: 12, deletions: 3 }),
      createDiffFile({ path: "src/b.ts", status: "added", additions: 88, deletions: 0 }),
      createDiffFile({ path: "scripts/old.cjs", status: "deleted", additions: 0, deletions: 41 }),
    ];

    expect(summarizeDiffFiles(files)).toEqual({
      fileCount: 3,
      additions: 100,
      deletions: 44,
    });
  });

  it("변경이 없으면 모든 값이 0이다", () => {
    expect(summarizeDiffFiles([])).toEqual({
      fileCount: 0,
      additions: 0,
      deletions: 0,
    });
  });

  it("줄 수를 세지 못한 파일도 파일 수에는 포함한다", () => {
    const files = [createDiffFile({ path: "assets/logo.png", status: "added" })];

    expect(summarizeDiffFiles(files)).toEqual({
      fileCount: 1,
      additions: 0,
      deletions: 0,
    });
  });
});
