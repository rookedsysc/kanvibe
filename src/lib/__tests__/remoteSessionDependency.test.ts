import { beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

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

  it("로컬 환경에도 세션 의존성 상태를 조회한다", async () => {
    mockExecGit.mockResolvedValue("");
    const { getSessionDependencyStatus } = await import("@/lib/remoteSessionDependency");

    await expect(getSessionDependencyStatus(SessionType.TMUX, null)).resolves.toMatchObject({
      available: true,
      isRemote: false,
      toolName: "tmux",
    });
    expect(mockExecGit).toHaveBeenCalledWith("command -v tmux >/dev/null 2>&1", null);
  });

  it("로컬 환경에도 자동 설치를 시도할 수 있다", async () => {
    mockExecGit
      .mockRejectedValueOnce(new Error("missing"))
      .mockResolvedValueOnce("")
      .mockResolvedValueOnce("");
    const { ensureSessionDependency } = await import("@/lib/remoteSessionDependency");

    await expect(ensureSessionDependency(SessionType.ZELLIJ, null)).resolves.toBeUndefined();
    expect(mockExecGit.mock.calls[1]?.[0]).toContain("cargo install --locked zellij");
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

  it("원격 의존성 확인 중 SSH 연결 실패가 발생하면 설치를 시도하거나 차단하지 않는다", async () => {
    // Given
    mockExecGit.mockRejectedValueOnce(new Error("remote-a 원격 명령 실패: Connection reset by 100.73.171.123 port 22"));
    const { ensureRemoteSessionDependency } = await import("@/lib/remoteSessionDependency");

    // When & Then
    await expect(ensureRemoteSessionDependency(SessionType.TMUX, "remote-a")).rejects.toThrow(/Connection reset/);
    expect(mockExecGit).toHaveBeenCalledTimes(1);
    expect(mockExecGit).toHaveBeenCalledWith("command -v tmux >/dev/null 2>&1", "remote-a");

    mockExecGit.mockClear();
    mockExecGit.mockResolvedValueOnce("");
    await expect(ensureRemoteSessionDependency(SessionType.TMUX, "remote-a")).resolves.toBeUndefined();
    expect(mockExecGit).toHaveBeenCalledTimes(1);
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
    expect(mockExecGit.mock.calls[1]?.[0]).toContain("run_install() {");
    expect(mockExecGit.mock.calls[1]?.[0]).not.toContain("run_install() {;");
    expect(mockExecGit.mock.calls[2]).toEqual(["command -v zellij >/dev/null 2>&1", "remote-b"]);
  });

  it("자동 설치 중 SSH 연결 실패가 발생하면 호스트를 차단하지 않는다", async () => {
    // Given
    mockExecGit
      .mockRejectedValueOnce(new Error("missing"))
      .mockRejectedValueOnce(new Error("remote-b 원격 명령 실패: Connection closed by 100.73.171.123 port 22"));
    const { ensureRemoteSessionDependency } = await import("@/lib/remoteSessionDependency");

    // When & Then
    await expect(ensureRemoteSessionDependency(SessionType.TMUX, "remote-b")).rejects.toThrow(/Connection closed/);

    mockExecGit.mockClear();
    mockExecGit.mockResolvedValueOnce("");
    await expect(ensureRemoteSessionDependency(SessionType.TMUX, "remote-b")).resolves.toBeUndefined();
    expect(mockExecGit).toHaveBeenCalledWith("command -v tmux >/dev/null 2>&1", "remote-b");
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
