import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getSessionDependencyStatus: vi.fn(),
  installSessionDependency: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/sessionDependency", () => ({
  getSessionDependencyStatus: mocks.getSessionDependencyStatus,
  installSessionDependency: mocks.installSessionDependency,
}));

describe("sessionDependencyPrompt", () => {
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

  it("의존성이 이미 있으면 팝업 없이 통과한다", async () => {
    mocks.getSessionDependencyStatus.mockResolvedValue({
      available: true,
      toolName: "tmux",
      isRemote: false,
      sshHost: null,
      blockedReason: null,
    });
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

    const { ensureSessionDependencyWithPrompt } = await import("@/desktop/renderer/utils/sessionDependencyPrompt");

    await expect(ensureSessionDependencyWithPrompt("tmux" as never, null, tCommon)).resolves.toBe(true);
    expect(confirmSpy).not.toHaveBeenCalled();
    expect(mocks.installSessionDependency).not.toHaveBeenCalled();
  });

  it("의존성이 없고 사용자가 취소하면 설치를 진행하지 않는다", async () => {
    mocks.getSessionDependencyStatus.mockResolvedValue({
      available: false,
      toolName: "zellij",
      isRemote: true,
      sshHost: "remote-a",
      blockedReason: null,
    });
    vi.spyOn(window, "confirm").mockReturnValue(false);

    const { ensureSessionDependencyWithPrompt } = await import("@/desktop/renderer/utils/sessionDependencyPrompt");

    await expect(ensureSessionDependencyWithPrompt("zellij" as never, "remote-a", tCommon)).resolves.toBe(false);
    expect(mocks.installSessionDependency).not.toHaveBeenCalled();
  });

  it("의존성이 없고 사용자가 확인하면 설치 후 통과한다", async () => {
    mocks.getSessionDependencyStatus.mockResolvedValue({
      available: false,
      toolName: "tmux",
      isRemote: false,
      sshHost: null,
      blockedReason: null,
    });
    mocks.installSessionDependency.mockResolvedValue({
      available: true,
      toolName: "tmux",
      isRemote: false,
      sshHost: null,
      blockedReason: null,
    });
    vi.spyOn(window, "confirm").mockReturnValue(true);

    const { ensureSessionDependencyWithPrompt } = await import("@/desktop/renderer/utils/sessionDependencyPrompt");

    await expect(ensureSessionDependencyWithPrompt("tmux" as never, null, tCommon)).resolves.toBe(true);
    expect(mocks.installSessionDependency).toHaveBeenCalledWith("tmux", null);
  });
});
