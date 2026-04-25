import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  mockExecGit,
  mockReadTextFile,
  mockWriteTextFile,
} = vi.hoisted(() => ({
  mockExecGit: vi.fn(),
  mockReadTextFile: vi.fn(),
  mockWriteTextFile: vi.fn(),
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

vi.mock("@/lib/hostFileAccess", () => ({
  readTextFile: (...args: unknown[]) => mockReadTextFile(...args),
  writeTextFile: (...args: unknown[]) => mockWriteTextFile(...args),
  quoteShellArgument: (value: string) => `'${value.replaceAll("'", `'\\''`)}'`,
}));

describe("gitExclude remote", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockExecGit.mockResolvedValue("/remote/main/.git");
    mockReadTextFile.mockResolvedValue("# existing\n");
    mockWriteTextFile.mockResolvedValue(undefined);
  });

  it("writes AI hook patterns to the remote common git exclude", async () => {
    const { addAiToolPatternsToGitExclude } = await import("@/lib/gitExclude");

    await addAiToolPatternsToGitExclude("/remote/worktree/task-1", "remote-host");

    expect(mockExecGit).toHaveBeenCalledWith(
      "git -C '/remote/worktree/task-1' rev-parse --path-format=absolute --git-common-dir",
      "remote-host",
    );
    expect(mockWriteTextFile).toHaveBeenCalledWith(
      "/remote/main/.git/info/exclude",
      expect.stringContaining(".codex/config.toml"),
      "remote-host",
    );
  });
});
