/**
 * @vitest-environment node
 */
import path from "path";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

// --- Mocks ---

const mockExecSync = vi.fn();
vi.mock("child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("child_process")>();
  return {
    ...actual,
    execSync: (...args: unknown[]) => mockExecSync(...args),
  };
});

const mockExistsSync = vi.fn();
vi.mock("fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("fs")>();
  return {
    ...actual,
    existsSync: (...args: unknown[]) => mockExistsSync(...args),
  };
});

const mockPtyOnData = vi.fn();
const mockPtyOnExit = vi.fn();
vi.mock("node-pty", () => ({
  spawn: vi.fn(() => ({
    write: vi.fn(),
    resize: vi.fn(),
    onData: mockPtyOnData,
    onExit: mockPtyOnExit,
    kill: vi.fn(),
    pid: 12345,
  })),
}));

function createMockWs() {
  return {
    readyState: 1,
    OPEN: 1,
    send: vi.fn(),
    close: vi.fn(),
    on: vi.fn(),
  } as unknown as import("ws").WebSocket;
}

/** execSync 호출 중 특정 패턴이 포함된 명령어를 찾아 반환한다 */
function findExecSyncCall(pattern: string): string | undefined {
  return mockExecSync.mock.calls
    .map((call) => call[0] as string)
    .find((cmd) => cmd.includes(pattern));
}

describe("attachLocalSession — tmux 세션 자동 생성", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it("should create session when tmux session does not exist", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    mockExecSync.mockImplementation((cmd: string) => {
      if (typeof cmd === "string" && cmd.includes("has-session")) {
        throw new Error("session not found");
      }
      return "";
    });

    // When
    await attachLocalSession(
      "task-1",
      SessionType.TMUX,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    const newSessionCmd = findExecSyncCall("new-session");
    expect(newSessionCmd).toBeDefined();
    expect(newSessionCmd).toContain('-s "feat-login"');
    expect(newSessionCmd).toContain('-c "/workspace"');
  });

  it("should skip session creation when tmux session already exists", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    mockExecSync.mockReturnValue("");

    // When
    await attachLocalSession(
      "task-2",
      SessionType.TMUX,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    expect(findExecSyncCall("new-session")).toBeUndefined();
  });

  it("should attach directly to session without window targeting", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");
    mockExecSync.mockReturnValue("");

    // When
    await attachLocalSession(
      "task-3",
      SessionType.TMUX,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    expect(nodePty.spawn).toHaveBeenCalledWith(
      "tmux",
      ["attach-session", "-t", "feat-login"],
      expect.any(Object),
    );
  });

  it("should close ws when session creation fails", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    const ws = createMockWs();
    mockExecSync.mockImplementation((cmd: string) => {
      if (typeof cmd === "string" && cmd.includes("has-session")) {
        throw new Error("session not found");
      }
      if (typeof cmd === "string" && cmd.includes("new-session")) {
        throw new Error("tmux creation failed");
      }
      return "";
    });

    // When
    await attachLocalSession(
      "task-4",
      SessionType.TMUX,
      "feat-login",
      ws,
      "/workspace",
    );

    // Then
    expect(ws.close).toHaveBeenCalledWith(1008, "tmux 세션 생성에 실패했습니다.");
  });
});

describe("attachLocalSession — zellij 세션 생성 및 레이아웃 적용", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  /** zellij list-sessions 응답을 설정한다. 다른 execSync 호출은 빈 문자열을 반환한다 */
  function mockZellijSessions(sessionListOutput: string) {
    mockExecSync.mockImplementation((cmd: string) => {
      if (typeof cmd === "string" && cmd.includes("list-sessions")) {
        return sessionListOutput;
      }
      return "";
    });
  }

  it("should spawn new zellij session with --session flag when session does not exist", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");
    mockZellijSessions("other-session [Created 1h ago]\n");
    mockExistsSync.mockReturnValue(false);

    // When
    await attachLocalSession(
      "task-z1",
      SessionType.ZELLIJ,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    expect(nodePty.spawn).toHaveBeenCalledWith(
      "zellij",
      ["--session", "feat-login"],
      expect.objectContaining({ cwd: "/workspace" }),
    );
  });

  it("should include --new-session-with-layout when KDL layout file exists", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");
    mockZellijSessions("");
    mockExistsSync.mockReturnValue(true);

    const expectedLayoutPath = path.join("/workspace", ".zellij-layout.kdl");

    // When
    await attachLocalSession(
      "task-z2",
      SessionType.ZELLIJ,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    expect(nodePty.spawn).toHaveBeenCalledWith(
      "zellij",
      ["--session", "feat-login", "--new-session-with-layout", expectedLayoutPath],
      expect.objectContaining({ cwd: "/workspace" }),
    );
  });

  it("should attach to existing zellij session without creating new one", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");
    mockZellijSessions("feat-login [Created 1h ago]\n");

    // When
    await attachLocalSession(
      "task-z3",
      SessionType.ZELLIJ,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    expect(nodePty.spawn).toHaveBeenCalledWith(
      "zellij",
      ["attach", "feat-login"],
      expect.any(Object),
    );
  });

  it("should not include layout flag when layout file does not exist", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");
    mockZellijSessions("");
    mockExistsSync.mockReturnValue(false);

    // When
    await attachLocalSession(
      "task-z4",
      SessionType.ZELLIJ,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    expect(nodePty.spawn).toHaveBeenCalledWith(
      "zellij",
      ["--session", "feat-login"],
      expect.objectContaining({ cwd: "/workspace" }),
    );
  });

  it("should skip layout file check when cwd is not provided", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");
    mockZellijSessions("");

    // When
    await attachLocalSession(
      "task-z5",
      SessionType.ZELLIJ,
      "feat-login",
      createMockWs(),
    );

    // Then
    /** existsSync가 호출되지 않아야 한다 (cwd가 없으므로 레이아웃 파일 체크 불필요) */
    expect(mockExistsSync).not.toHaveBeenCalled();
    expect(nodePty.spawn).toHaveBeenCalledWith(
      "zellij",
      ["--session", "feat-login"],
      expect.any(Object),
    );
  });
});
