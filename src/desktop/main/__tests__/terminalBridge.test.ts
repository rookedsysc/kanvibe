/**
 * @vitest-environment node
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const mockEnsureKanvibeHooksCurrent = vi.fn<(...args: unknown[]) => Promise<boolean>>(async () => false);
const mockBroadcastTaskHookInstallFailed = vi.fn();
vi.mock("@/lib/kanvibeHooksInstaller", () => ({
  ensureKanvibeHooksCurrent: (...args: unknown[]) => mockEnsureKanvibeHooksCurrent(...args),
}));
vi.mock("@/lib/boardNotifier", () => ({
  broadcastTaskHookInstallFailed: (...args: unknown[]) => mockBroadcastTaskHookInstallFailed(...args),
}));
const mockFindProjectBy = vi.fn();
const mockFindOneBy = vi.fn();
vi.mock("@/lib/database", () => ({
  getTaskRepository: async () => ({ findOneBy: mockFindOneBy }),
  getProjectRepository: async () => ({ findOneBy: mockFindProjectBy }),
}));

const mockParseSSHConfig = vi.fn();
vi.mock("@/lib/sshConfig", () => ({
  parseSSHConfig: (...args: unknown[]) => mockParseSSHConfig(...args),
}));

const mockDecodeDataUrlToBuffer = vi.fn();
const mockTransferImageToRemoteHost = vi.fn();
vi.mock("@/lib/remoteImagePaste", () => ({
  decodeDataUrlToBuffer: (...args: unknown[]) => mockDecodeDataUrlToBuffer(...args),
  transferImageToRemoteHost: (...args: unknown[]) => mockTransferImageToRemoteHost(...args),
}));

vi.mock("@/lib/terminal", () => ({
  attachLocalSession: vi.fn(),
  attachRemoteSession: vi.fn(),
  focusSession: vi.fn(),
}));

vi.mock("@/lib/remoteSessionDependency", () => ({
  ensureRemoteSessionDependency: vi.fn(),
}));

const mockClearRemoteSSHTransportCooldown = vi.fn();
vi.mock("@/lib/gitOperations", () => ({
  clearRemoteSSHTransportCooldown: (...args: unknown[]) => mockClearRemoteSSHTransportCooldown(...args),
}));

vi.mock("@/desktop/main/services/paneLayoutService", () => ({
  getEffectivePaneLayout: vi.fn(),
}));

const SSH_CONFIG = {
  host: "app-prod",
  hostname: "example.com",
  port: 2202,
  username: "tester",
  privateKeyPath: "/tmp/test-key",
};

async function importBridge() {
  return import("@/desktop/main/terminalBridge");
}

describe("terminalBridge.pasteImageToRemoteTerminal", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("returns an error without attempting a transfer when the task has no sshHost (local session)", async () => {
    // Given
    mockFindOneBy.mockResolvedValue({ id: "task-1", sshHost: null });
    const { pasteImageToRemoteTerminal } = await importBridge();

    // When
    const result = await pasteImageToRemoteTerminal("task-1", "data:image/png;base64,aGVsbG8=");

    // Then
    expect(result).toEqual({ ok: false, error: "원격 세션이 아닙니다." });
    expect(mockTransferImageToRemoteHost).not.toHaveBeenCalled();
  });

  it("returns an error when the task's sshHost has no matching parsed SSH config", async () => {
    // Given
    mockFindOneBy.mockResolvedValue({ id: "task-1", sshHost: "unknown-host" });
    mockParseSSHConfig.mockResolvedValue([]);
    const { pasteImageToRemoteTerminal } = await importBridge();

    // When
    const result = await pasteImageToRemoteTerminal("task-1", "data:image/png;base64,aGVsbG8=");

    // Then
    expect(result).toEqual({ ok: false, error: "SSH 호스트를 찾을 수 없습니다: unknown-host" });
  });

  it("transfers the decoded image and resolves with the remote path for a remote session", async () => {
    // Given
    mockFindOneBy.mockResolvedValue({ id: "task-1", sshHost: "app-prod" });
    mockParseSSHConfig.mockResolvedValue([SSH_CONFIG]);
    const decodedBuffer = Buffer.from("hello");
    mockDecodeDataUrlToBuffer.mockReturnValue(decodedBuffer);
    mockTransferImageToRemoteHost.mockResolvedValue("/tmp/kanvibe-paste-fixed-uuid.png");
    const { pasteImageToRemoteTerminal } = await importBridge();

    // When
    const result = await pasteImageToRemoteTerminal("task-1", "data:image/png;base64,aGVsbG8=");

    // Then
    expect(result).toEqual({ ok: true, remotePath: "/tmp/kanvibe-paste-fixed-uuid.png" });
    expect(mockTransferImageToRemoteHost).toHaveBeenCalledWith(SSH_CONFIG, decodedBuffer);
  });

  it("returns the failure message when the scp transfer rejects", async () => {
    // Given
    mockFindOneBy.mockResolvedValue({ id: "task-1", sshHost: "app-prod" });
    mockParseSSHConfig.mockResolvedValue([SSH_CONFIG]);
    mockDecodeDataUrlToBuffer.mockReturnValue(Buffer.from("hello"));
    mockTransferImageToRemoteHost.mockRejectedValue(new Error("scp 전송 실패 (exit 1): Permission denied"));
    const { pasteImageToRemoteTerminal } = await importBridge();

    // When
    const result = await pasteImageToRemoteTerminal("task-1", "data:image/png;base64,aGVsbG8=");

    // Then
    expect(result).toEqual({ ok: false, error: "scp 전송 실패 (exit 1): Permission denied" });
  });
});

describe("terminalBridge pending open ownership", () => {
  beforeEach(() => { vi.resetModules(); vi.clearAllMocks(); });
  function windowStub() {
    return { id: 100, isDestroyed: vi.fn(() => false), send: vi.fn() } as unknown as import("electron").WebContents;
  }
  it("shares pending open and keeps one balanced client registration", async () => {
    let resolveTask!: (task: unknown) => void;
    mockFindOneBy.mockReturnValue(new Promise((resolve) => { resolveTask = resolve; }));
    const bridge = await importBridge();
    const terminal = await import("@/lib/terminal");
    const window = windowStub();
    const first = bridge.openTerminal(window, "task", null, 100, 30);
    const second = bridge.openTerminal(window, "task", null, 100, 30);
    await vi.waitFor(() => expect(mockFindOneBy).toHaveBeenCalled());
    resolveTask({ sessionType: "terminal", sessionName: "session" });
    await Promise.all([first, second]);
    expect(terminal.attachLocalSession).toHaveBeenCalledTimes(1);
    const client = vi.mocked(terminal.attachLocalSession).mock.calls[0][4] as unknown as { readyState: number };
    bridge.closeTerminal(window.id, "task", null);
    expect(client.readyState).toBe(1);
    bridge.closeWindowTerminals(window.id);
    expect(client.readyState).toBe(3);
  });
  it.each(["close", "destroy"])("cancels before attach on pending %s", async (mode) => {
    let resolveTask!: (task: unknown) => void;
    mockFindOneBy.mockReturnValue(new Promise((resolve) => { resolveTask = resolve; }));
    const bridge = await importBridge();
    const terminal = await import("@/lib/terminal");
    const window = windowStub();
    const pending = bridge.openTerminal(window, "task", null, 100, 30);
    await vi.waitFor(() => expect(mockFindOneBy).toHaveBeenCalled());
    if (mode === "close") bridge.closeTerminal(window.id, "task", null);
    else bridge.closeWindowTerminals(window.id);
    resolveTask({ sessionType: "terminal", sessionName: "session" });
    await pending;
    expect(terminal.attachLocalSession).not.toHaveBeenCalled();
  });
  it("allows retry after failed preparation", async () => {
    mockFindOneBy.mockRejectedValueOnce(new Error("database unavailable"));
    const bridge = await importBridge();
    const window = windowStub();
    await expect(bridge.openTerminal(window, "task", null, 100, 30)).resolves.toMatchObject({ ok: false });
    mockFindOneBy.mockResolvedValue({ sessionType: "terminal", sessionName: "session" });
    await expect(bridge.openTerminal(window, "task", null, 100, 30)).resolves.toEqual({ ok: true });
    bridge.closeWindowTerminals(window.id);
  });

  it("forwards the remaining SSH cooldown as a terminal retry hint", async () => {
    mockFindOneBy.mockRejectedValueOnce(Object.assign(new Error("SSH transport cooldown"), { retryAfterMs: 3200 }));
    const bridge = await importBridge();

    await expect(bridge.openTerminal(windowStub(), "task", null, 100, 30)).resolves.toEqual({
      ok: false, error: "SSH transport cooldown", retryAfterMs: 3200,
    });
  });

  it.each([undefined, 0, -1, Number.NaN, Number.POSITIVE_INFINITY])("does not retry an invalid hint %s", async (retryAfterMs) => {
    mockFindOneBy.mockRejectedValueOnce(Object.assign(new Error("permanent failure"), { retryAfterMs }));
    const bridge = await importBridge();

    await expect(bridge.openTerminal(windowStub(), "task", null, 100, 30)).resolves.toEqual({
      ok: false, error: "permanent failure",
    });
  });
});


describe("terminalBridge existing remote session migration", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mockEnsureKanvibeHooksCurrent.mockResolvedValue(false);
    mockFindOneBy.mockResolvedValue({ id: "task-1", sshHost: "app-prod" });
    mockParseSSHConfig.mockResolvedValue([SSH_CONFIG]);
  });

  function remoteTask() {
    return { id: "task-1", title: "existing task", sshHost: "app-prod", worktreePath: "/remote/repo", sessionType: "tmux", sessionName: "existing" } as import("@/entities/KanbanTask").KanbanTask;
  }

  it("모바일과 데스크톱 공통 attach 이후 백그라운드로 훅을 검사한다", async () => {
    const bridge = await importBridge();
    const terminal = await import("@/lib/terminal");
    const client = new bridge.CallbackTerminalClient(() => {});
    await bridge.attachTaskTerminal(remoteTask(), null, client);
    expect(mockEnsureKanvibeHooksCurrent).toHaveBeenCalledWith("/remote/repo", "task-1", "app-prod");
    expect(vi.mocked(terminal.attachRemoteSession).mock.invocationCallOrder[0]).toBeLessThan(mockEnsureKanvibeHooksCurrent.mock.invocationCallOrder[0]);
  });

  it("원격 터미널을 열 때 백그라운드 실패로 걸린 SSH cooldown을 원격 명령보다 먼저 해제한다", async () => {
    const bridge = await importBridge();
    const remoteSessionDependency = await import("@/lib/remoteSessionDependency");
    await bridge.attachTaskTerminal(remoteTask(), null, new bridge.CallbackTerminalClient(() => {}));
    expect(mockClearRemoteSSHTransportCooldown).toHaveBeenCalledWith("app-prod");
    expect(mockClearRemoteSSHTransportCooldown.mock.invocationCallOrder[0])
      .toBeLessThan(vi.mocked(remoteSessionDependency.ensureRemoteSessionDependency).mock.invocationCallOrder[0]);
  });

  it("worktree 경로가 없는 기본 브랜치 세션도 프로젝트 경로의 훅을 검사한다", async () => {
    mockFindProjectBy.mockResolvedValue({ repoPath: "/remote/root" });
    const bridge = await importBridge();
    const task = { ...remoteTask(), worktreePath: null, projectId: "project-1" };
    await bridge.attachTaskTerminal(task, null, new bridge.CallbackTerminalClient(() => {}));
    expect(mockFindProjectBy).toHaveBeenCalledWith({ id: "project-1" });
    await vi.waitFor(() => expect(mockEnsureKanvibeHooksCurrent).toHaveBeenCalledWith("/remote/root", "task-1", "app-prod"));
  });

  it("백그라운드 hook 실패를 알리되 열린 터미널은 유지하고 다음 요청은 재시도한다", async () => {
    mockEnsureKanvibeHooksCurrent.mockRejectedValueOnce(new Error("hooks write denied"));
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const bridge = await importBridge();
    const terminal = await import("@/lib/terminal");
    const client = new bridge.CallbackTerminalClient(() => {});
    await expect(bridge.attachTaskTerminal(remoteTask(), null, client)).resolves.toBeUndefined();
    await vi.waitFor(() => expect(mockBroadcastTaskHookInstallFailed).toHaveBeenCalled());
    expect(client.readyState).toBe(client.OPEN);
    expect(mockBroadcastTaskHookInstallFailed).toHaveBeenCalledWith({ taskId: "task-1", taskTitle: "existing task", error: "hooks write denied" });
    expect(consoleErrorSpy).toHaveBeenCalledWith("터미널 hooks 확인 실패:", expect.objectContaining({
      taskId: "task-1",
      targetPath: "/remote/repo",
      sshHost: "app-prod",
      error: "hooks write denied",
    }));
    consoleErrorSpy.mockRestore();
    expect(terminal.attachRemoteSession).toHaveBeenCalledTimes(1);
    await bridge.attachTaskTerminal(remoteTask(), null, client);
    expect(terminal.attachRemoteSession).toHaveBeenCalledTimes(2);
  });

  it("일시적인 SSH 재연결 대기를 hook 설치 실패 알림으로 표시하지 않는다", async () => {
    mockFindOneBy.mockResolvedValue(remoteTask());
    mockEnsureKanvibeHooksCurrent.mockRejectedValueOnce(Object.assign(new Error("SSH transport cooldown"), { retryAfterMs: 3200 }));
    const bridge = await importBridge();
    const window = { id: 101, isDestroyed: () => false, send: vi.fn() } as unknown as import("electron").WebContents;

    await expect(bridge.openTerminal(window, "task-1", null, 100, 30)).resolves.toEqual({
      ok: true,
    });
    await vi.waitFor(() => expect(mockEnsureKanvibeHooksCurrent).toHaveBeenCalled());
    expect(mockBroadcastTaskHookInstallFailed).not.toHaveBeenCalled();
  });

  it("hook 지연 중에도 attach와 terminal 입력 경로를 사용할 수 있다", async () => {
    let finish!: (value: boolean) => void;
    mockEnsureKanvibeHooksCurrent.mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
    const bridge = await importBridge();
    const terminal = await import("@/lib/terminal");
    const onInput = vi.fn();
    vi.mocked(terminal.attachRemoteSession).mockImplementationOnce(async (...args) => {
      args[5].on("message", onInput);
    });
    const window = { id: 102, isDestroyed: () => false, send: vi.fn() } as unknown as import("electron").WebContents;
    mockFindOneBy.mockResolvedValue(remoteTask());
    let opened = false;
    const pending = bridge.openTerminal(window, "task-1", null, 100, 30).then((result) => { opened = true; return result; });
    try {
      await vi.waitFor(() => expect(opened).toBe(true), { timeout: 200 });
      expect(await pending).toEqual({ ok: true });
      expect(terminal.attachRemoteSession).toHaveBeenCalledTimes(1);
      bridge.writeTerminal(102, "task-1", null, "pwd\r");
      expect(onInput).toHaveBeenCalledWith(Buffer.from("pwd\r"));
    } finally { finish(false); await pending; }
  });

  it.each(["tmux", "herdr"])("초기 pane 명령은 hook 준비 뒤에만 실행 경로로 진입한다 (%s)", async (sessionType) => {
    let finish!: (value: boolean) => void;
    mockEnsureKanvibeHooksCurrent.mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
    const layout = await import("@/desktop/main/services/paneLayoutService");
    vi.mocked(layout.getEffectivePaneLayout).mockResolvedValueOnce({ panes: [{ position: 0, command: "codex" }] } as never);
    const bridge = await importBridge();
    const terminal = await import("@/lib/terminal");
    const pending = bridge.attachTaskTerminal({ ...remoteTask(), sessionType } as never, null, new bridge.CallbackTerminalClient(() => {}));
    await vi.waitFor(() => expect(mockEnsureKanvibeHooksCurrent).toHaveBeenCalled());
    expect(terminal.attachRemoteSession).not.toHaveBeenCalled();
    finish(false);
    await pending;
    expect(terminal.attachRemoteSession).toHaveBeenCalledTimes(1);
  });

  it("hook 준비 실패 시 초기 pane 명령의 자동 실행은 시작하지 않는다", async () => {
    const layout = await import("@/desktop/main/services/paneLayoutService");
    vi.mocked(layout.getEffectivePaneLayout).mockResolvedValueOnce({ panes: [{ position: 0, command: "codex" }] } as never);
    mockEnsureKanvibeHooksCurrent.mockRejectedValueOnce(new Error("hook unavailable"));
    const bridge = await importBridge();
    const terminal = await import("@/lib/terminal");
    await expect(bridge.attachTaskTerminal(remoteTask(), null, new bridge.CallbackTerminalClient(() => {}))).rejects.toThrow("hook unavailable");
    expect(terminal.attachRemoteSession).not.toHaveBeenCalled();
  });

});
