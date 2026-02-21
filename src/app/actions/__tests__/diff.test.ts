// @vitest-environment node
import { describe, it, expect, vi, beforeEach } from "vitest";

// --- Mocks ---

const mockFindOne = vi.fn();

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn().mockResolvedValue({
    findOne: mockFindOne,
  }),
}));

const mockExecAsync = vi.fn();

vi.mock("child_process", () => ({
  exec: vi.fn(),
}));

vi.mock("util", () => ({
  promisify: () => mockExecAsync,
}));

const mockReadFile = vi.fn();
const mockWriteFile = vi.fn();

vi.mock("fs/promises", () => ({
  readFile: (...args: unknown[]) => mockReadFile(...args),
  writeFile: (...args: unknown[]) => mockWriteFile(...args),
}));

describe("diff actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  describe("getGitDiffFiles", () => {
    it("should return committed diff files with numstat", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "feature",
        baseBranch: "main",
      });

      mockExecAsync
        .mockResolvedValueOnce({ stdout: "M\tsrc/index.ts\nA\tsrc/new.ts\n" })
        .mockResolvedValueOnce({ stdout: "10\t2\tsrc/index.ts\n20\t0\tsrc/new.ts\n" })
        .mockResolvedValueOnce({ stdout: "" });

      // When
      const { getGitDiffFiles } = await import("@/app/actions/diff");
      const files = await getGitDiffFiles("task-1");

      // Then
      expect(files).toHaveLength(2);
      expect(files[0]).toEqual({
        path: "src/index.ts",
        status: "modified",
        additions: 10,
        deletions: 2,
      });
      expect(files[1]).toEqual({
        path: "src/new.ts",
        status: "added",
        additions: 20,
        deletions: 0,
      });
    });

    it("should include untracked files from git status", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "feature",
        baseBranch: "main",
      });

      /** git diff 결과는 비어있고, git status에만 untracked 파일이 있는 경우 */
      mockExecAsync
        .mockResolvedValueOnce({ stdout: "" })
        .mockResolvedValueOnce({ stdout: "" })
        .mockResolvedValueOnce({ stdout: "?? src/untracked.ts\n?? .codex/config.toml\n" });

      // When
      const { getGitDiffFiles } = await import("@/app/actions/diff");
      const files = await getGitDiffFiles("task-1");

      // Then
      expect(files).toHaveLength(2);
      expect(files[0]).toEqual({
        path: "src/untracked.ts",
        status: "added",
        additions: 0,
        deletions: 0,
      });
      expect(files[1]).toEqual({
        path: ".codex/config.toml",
        status: "added",
        additions: 0,
        deletions: 0,
      });
    });

    it("should not duplicate files already in committed diff", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "feature",
        baseBranch: "main",
      });

      /** 같은 파일이 git diff와 git status 양쪽에 있는 경우 명령어 기반으로 mock한다 */
      mockExecAsync.mockImplementation((cmd: string) => {
        if (cmd.includes("--name-status")) return Promise.resolve({ stdout: "M\tsrc/index.ts\n" });
        if (cmd.includes("--numstat")) return Promise.resolve({ stdout: "5\t1\tsrc/index.ts\n" });
        if (cmd.includes("status")) return Promise.resolve({ stdout: " M src/index.ts\n" });
        return Promise.resolve({ stdout: "" });
      });

      // When
      const { getGitDiffFiles } = await import("@/app/actions/diff");
      const files = await getGitDiffFiles("task-1");

      // Then
      expect(files).toHaveLength(1);
      expect(files[0].status).toBe("modified");
      expect(files[0].additions).toBe(5);
    });

    it("should handle git diff failure gracefully and still return git status files", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "nonexistent-branch",
        baseBranch: "main",
      });

      /** git diff가 실패하지만 git status는 성공하는 경우 */
      mockExecAsync
        .mockRejectedValueOnce(new Error("unknown revision"))
        .mockRejectedValueOnce(new Error("unknown revision"))
        .mockResolvedValueOnce({ stdout: "?? newfile.ts\n" });

      // When
      const { getGitDiffFiles } = await import("@/app/actions/diff");
      const files = await getGitDiffFiles("task-1");

      // Then
      expect(files).toHaveLength(1);
      expect(files[0]).toEqual({
        path: "newfile.ts",
        status: "added",
        additions: 0,
        deletions: 0,
      });
    });

    it("should handle renamed files with R prefix", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "feature",
        baseBranch: "main",
      });

      mockExecAsync
        .mockResolvedValueOnce({ stdout: "R100\told.ts\tnew.ts\n" })
        .mockResolvedValueOnce({ stdout: "0\t0\tnew.ts\n" })
        .mockResolvedValueOnce({ stdout: "" });

      // When
      const { getGitDiffFiles } = await import("@/app/actions/diff");
      const files = await getGitDiffFiles("task-1");

      // Then
      expect(files).toHaveLength(1);
      expect(files[0]).toEqual({
        path: "new.ts",
        status: "renamed",
        additions: 0,
        deletions: 0,
      });
    });

    it("should return empty array when task not found", async () => {
      // Given
      mockFindOne.mockResolvedValue(null);

      // When
      const { getGitDiffFiles } = await import("@/app/actions/diff");
      const files = await getGitDiffFiles("nonexistent");

      // Then
      expect(files).toEqual([]);
    });

    it("should use 'main' as default baseBranch when not set", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "feature",
        baseBranch: null,
      });

      mockExecAsync
        .mockResolvedValueOnce({ stdout: "" })
        .mockResolvedValueOnce({ stdout: "" })
        .mockResolvedValueOnce({ stdout: "" });

      // When
      const { getGitDiffFiles } = await import("@/app/actions/diff");
      await getGitDiffFiles("task-1");

      // Then
      expect(mockExecAsync).toHaveBeenCalledWith(
        expect.stringContaining("main...feature"),
        expect.any(Object)
      );
    });
  });

  describe("getFileContent", () => {
    it("should read file content from worktree", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "feature",
      });
      mockReadFile.mockResolvedValue("file content here");

      // When
      const { getFileContent } = await import("@/app/actions/diff");
      const content = await getFileContent("task-1", "src/index.ts");

      // Then
      expect(content).toBe("file content here");
      expect(mockReadFile).toHaveBeenCalledWith(
        "/tmp/worktree/src/index.ts",
        "utf-8"
      );
    });

    it("should reject path traversal attempts", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "feature",
      });

      // When
      const { getFileContent } = await import("@/app/actions/diff");
      const content = await getFileContent("task-1", "../../../etc/passwd");

      // Then
      expect(content).toBe("");
      expect(mockReadFile).not.toHaveBeenCalled();
    });
  });

  describe("saveFileContent", () => {
    it("should write file content to worktree", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "feature",
      });
      mockWriteFile.mockResolvedValue(undefined);

      // When
      const { saveFileContent } = await import("@/app/actions/diff");
      const result = await saveFileContent("task-1", "src/index.ts", "new content");

      // Then
      expect(result).toEqual({ success: true });
      expect(mockWriteFile).toHaveBeenCalledWith(
        "/tmp/worktree/src/index.ts",
        "new content",
        "utf-8"
      );
    });

    it("should return error for path traversal attempts", async () => {
      // Given
      mockFindOne.mockResolvedValue({
        id: "task-1",
        worktreePath: "/tmp/worktree",
        branchName: "feature",
      });

      // When
      const { saveFileContent } = await import("@/app/actions/diff");
      const result = await saveFileContent("task-1", "../../etc/passwd", "malicious");

      // Then
      expect(result.success).toBe(false);
      expect(result.error).toContain("상위 디렉토리");
      expect(mockWriteFile).not.toHaveBeenCalled();
    });
  });
});
