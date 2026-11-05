/**
 * @vitest-environment node
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const mockFindOneBy = vi.fn();
vi.mock("@/lib/database", () => ({
  getTaskRepository: async () => ({ findOneBy: mockFindOneBy }),
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
});
