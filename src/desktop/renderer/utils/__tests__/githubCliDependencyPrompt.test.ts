import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getGitHubCliStatus: vi.fn(),
  installGitHubCli: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/githubCliDependency", () => ({
  getGitHubCliStatus: mocks.getGitHubCliStatus,
  installGitHubCli: mocks.installGitHubCli,
}));

describe("githubCliDependencyPrompt", () => {
  const tCommon = (key: string, values?: Record<string, string | number | Date>) => {
    if (key === "sessionDependency.localTarget") return "this machine";
    if (key === "sessionDependency.remoteTarget") return String(values?.host ?? "remote-host");
    if (key === "sessionDependency.installPrompt") return `${values?.tool} -> ${values?.target}`;
    if (key === "sessionDependency.installFailed") return `${values?.tool}: ${values?.error}`;
    return key;
  };

  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("gh가 이미 있으면 팝업 없이 통과한다", async () => {
    mocks.getGitHubCliStatus.mockResolvedValue({
      available: true,
      toolName: "gh",
      isRemote: true,
      sshHost: "remote-a",
      blockedReason: null,
    });
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

    const { ensureGitHubCliWithPrompt } = await import("@/desktop/renderer/utils/githubCliDependencyPrompt");

    await expect(ensureGitHubCliWithPrompt("remote-a", tCommon)).resolves.toBe(true);
    expect(confirmSpy).not.toHaveBeenCalled();
    expect(mocks.installGitHubCli).not.toHaveBeenCalled();
  });

  it("gh가 없고 사용자가 취소하면 설치를 진행하지 않는다", async () => {
    mocks.getGitHubCliStatus.mockResolvedValue({
      available: false,
      toolName: "gh",
      isRemote: true,
      sshHost: "remote-a",
      blockedReason: null,
    });
    vi.spyOn(window, "confirm").mockReturnValue(false);

    const { ensureGitHubCliWithPrompt } = await import("@/desktop/renderer/utils/githubCliDependencyPrompt");

    await expect(ensureGitHubCliWithPrompt("remote-a", tCommon)).resolves.toBe(false);
    expect(mocks.installGitHubCli).not.toHaveBeenCalled();
  });

  it("gh가 없고 사용자가 확인하면 설치 후 통과한다", async () => {
    mocks.getGitHubCliStatus.mockResolvedValue({
      available: false,
      toolName: "gh",
      isRemote: true,
      sshHost: "remote-a",
      blockedReason: null,
    });
    mocks.installGitHubCli.mockResolvedValue({
      available: true,
      toolName: "gh",
      isRemote: true,
      sshHost: "remote-a",
      blockedReason: null,
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);

    const { ensureGitHubCliWithPrompt } = await import("@/desktop/renderer/utils/githubCliDependencyPrompt");

    await expect(ensureGitHubCliWithPrompt("remote-a", tCommon)).resolves.toBe(true);
    expect(mocks.installGitHubCli).toHaveBeenCalledWith("remote-a");
  });
});
