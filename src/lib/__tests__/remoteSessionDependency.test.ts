import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

const mockExecGit = vi.fn();

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

describe("remoteSessionDependency", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("로컬 세션이면 원격 설치 검증을 건너뛴다", async () => {
    // Given
    const { ensureRemoteSessionDependency } = await import("@/lib/remoteSessionDependency");

    // When
    await ensureRemoteSessionDependency(SessionType.TMUX, null);

    // Then
    expect(mockExecGit).not.toHaveBeenCalled();
  });

  it("원격에 도구가 이미 있으면 설치를 시도하지 않는다", async () => {
    // Given
    mockExecGit.mockResolvedValue("");
    const { ensureRemoteSessionDependency } = await import("@/lib/remoteSessionDependency");

    // When
    await ensureRemoteSessionDependency(SessionType.TMUX, "remote-a");

    // Then
    expect(mockExecGit).toHaveBeenCalledTimes(1);
    expect(mockExecGit).toHaveBeenCalledWith("command -v tmux >/dev/null 2>&1", "remote-a");
  });

  it("원격에 도구가 없으면 자동 설치 후 다시 검증한다", async () => {
    // Given
    mockExecGit
      .mockRejectedValueOnce(new Error("missing"))
      .mockResolvedValueOnce("")
      .mockResolvedValueOnce("");
    const { ensureRemoteSessionDependency } = await import("@/lib/remoteSessionDependency");

    // When
    await ensureRemoteSessionDependency(SessionType.ZELLIJ, "remote-b");

    // Then
    expect(mockExecGit).toHaveBeenCalledTimes(3);
    expect(mockExecGit.mock.calls[1]?.[0]).toContain("cargo install --locked zellij");
    expect(mockExecGit.mock.calls[2]).toEqual(["command -v zellij >/dev/null 2>&1", "remote-b"]);
  });

  it("자동 설치에 실패하면 호스트를 차단하고 다음 요청도 즉시 실패시킨다", async () => {
    // Given
    mockExecGit
      .mockRejectedValueOnce(new Error("missing"))
      .mockRejectedValueOnce(new Error("sudo unavailable"));
    const { ensureRemoteSessionDependency } = await import("@/lib/remoteSessionDependency");

    // When
    const firstAttempt = ensureRemoteSessionDependency(SessionType.TMUX, "remote-c");

    // Then
    await expect(firstAttempt).rejects.toThrow(/remote-c 호스트.*tmux 설치/);

    mockExecGit.mockClear();
    await expect(ensureRemoteSessionDependency(SessionType.TMUX, "remote-c")).rejects.toThrow(/원격 접근을 차단/);
    expect(mockExecGit).not.toHaveBeenCalled();
  });
});
