/**
 * @vitest-environment node
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

// --- Mocks ---

const mockExecSync = vi.fn();
vi.mock("child_process", () => ({
  execSync: (...args: unknown[]) => mockExecSync(...args),
}));

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

describe("attachLocalSession — tmux 자동 생성 분기", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it("should create session with named window when legacy session does not exist", async () => {
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
      "kanvibe",
      " main",
      createMockWs(),
      "/workspace",
    );

    // Then
    const newSessionCmd = findExecSyncCall("new-session");
    expect(newSessionCmd).toBeDefined();
    expect(newSessionCmd).toContain('-s "kanvibe"');
    expect(newSessionCmd).toContain('-n " main"');
    expect(newSessionCmd).toContain('-c "/workspace"');
  });

  it("should create window only when legacy session exists but window does not", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    mockExecSync.mockImplementation((cmd: string) => {
      if (typeof cmd === "string" && cmd.includes("has-session")) return "";
      if (typeof cmd === "string" && cmd.includes("list-windows")) return "other-window\n";
      return "";
    });

    // When
    await attachLocalSession(
      "task-2",
      SessionType.TMUX,
      "kanvibe",
      " main",
      createMockWs(),
      "/workspace",
    );

    // Then
    const newWindowCmd = findExecSyncCall("new-window");
    expect(newWindowCmd).toBeDefined();
    expect(newWindowCmd).toContain('-t "kanvibe"');
    expect(newWindowCmd).toContain('-n " main"');
    expect(findExecSyncCall("new-session")).toBeUndefined();
  });

  it("should skip creation when legacy session and window both exist", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    mockExecSync.mockImplementation((cmd: string) => {
      if (typeof cmd === "string" && cmd.includes("has-session")) return "";
      if (typeof cmd === "string" && cmd.includes("list-windows")) return " main\n";
      return "";
    });

    // When
    await attachLocalSession(
      "task-3",
      SessionType.TMUX,
      "kanvibe",
      " main",
      createMockWs(),
      "/workspace",
    );

    // Then
    expect(findExecSyncCall("new-session")).toBeUndefined();
    expect(findExecSyncCall("new-window")).toBeUndefined();
  });

  it("should create independent session without window for new format", async () => {
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
      "task-4",
      SessionType.TMUX,
      "kanvibe/feat-login",
      " feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    const newSessionCmd = findExecSyncCall("new-session");
    expect(newSessionCmd).toBeDefined();
    expect(newSessionCmd).toContain('-s "kanvibe/feat-login"');
    expect(newSessionCmd).not.toContain("-n ");
  });

  it("should close ws when legacy session creation fails", async () => {
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
      "task-5",
      SessionType.TMUX,
      "kanvibe",
      " main",
      ws,
      "/workspace",
    );

    // Then
    expect(ws.close).toHaveBeenCalledWith(1008, "tmux 세션 생성에 실패했습니다.");
  });
});
