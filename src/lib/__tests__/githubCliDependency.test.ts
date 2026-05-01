import { beforeEach, describe, expect, it, vi } from "vitest";

const mockExecGit = vi.fn();

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
  isSSHTransportError: (error: unknown) => {
    const message = error instanceof Error
      ? error.message
      : error && typeof error === "object" && "stderr" in error
        ? String((error as { stderr?: string }).stderr)
        : "";

    return /Connection (?:reset|closed)|kex_exchange_identification/.test(message);
  },
}));

describe("githubCliDependency", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("로컬 환경에서도 gh 상태를 조회한다", async () => {
    mockExecGit.mockResolvedValue("");
    const { getGitHubCliStatus } = await import("@/lib/githubCliDependency");

    await expect(getGitHubCliStatus(null)).resolves.toMatchObject({
      available: true,
      isRemote: false,
      toolName: "gh",
    });
    expect(mockExecGit).toHaveBeenCalledWith("command -v gh >/dev/null 2>&1", null);
  });

  it("원격에 gh가 없으면 설치 후 다시 검증한다", async () => {
    mockExecGit
      .mockResolvedValueOnce("")
      .mockResolvedValueOnce("");
    const { installGitHubCli } = await import("@/lib/githubCliDependency");

    await expect(installGitHubCli("remote-a")).resolves.toBeUndefined();
    expect(mockExecGit).toHaveBeenCalledTimes(2);
    expect(mockExecGit.mock.calls[0]?.[0]).toContain("brew install gh");
    expect(mockExecGit.mock.calls[0]?.[0]).toContain("github-cli.list");
    expect(mockExecGit.mock.calls[0]?.[0]).toContain("pacman -Sy --noconfirm github-cli");
    expect(mockExecGit.mock.calls[1]).toEqual(["command -v gh >/dev/null 2>&1", "remote-a"]);
  });

  it("원격 gh 설치에 실패하면 호스트를 차단하고 다음 요청도 즉시 실패시킨다", async () => {
    mockExecGit.mockRejectedValueOnce(new Error("sudo unavailable"));
    const { installGitHubCli, getGitHubCliStatus } = await import("@/lib/githubCliDependency");

    await expect(installGitHubCli("remote-b")).rejects.toThrow(/remote-b 호스트.*gh 설치/);

    mockExecGit.mockClear();
    await expect(getGitHubCliStatus("remote-b")).resolves.toMatchObject({
      available: false,
      blockedReason: expect.stringMatching(/원격 접근을 차단/),
    });
    expect(mockExecGit).not.toHaveBeenCalled();
  });
});
