/**
 * @vitest-environment node
 */
import path from "path";
import { spawnSync } from "child_process";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SessionType } from "@/entities/KanbanTask";
import { PaneLayoutType } from "@/entities/PaneLayoutConfig";

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
    existsSync: mockExistsSync,
  };
});

const mockHomedir = vi.fn(() => "/home/local-user");
vi.mock("os", async (importOriginal) => {
  const actual = await importOriginal<typeof import("os")>();
  return {
    ...actual,
    default: {
      ...actual,
      homedir: mockHomedir,
    },
    homedir: mockHomedir,
  };
});

const mockPtyWrite = vi.fn();
const mockPtyOnData = vi.fn();
const mockPtyOnExit = vi.fn();
vi.mock("node-pty", () => ({
  spawn: vi.fn(() => ({
    write: mockPtyWrite,
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

function expectValidPosixShellSyntax(command: string): void {
  const result = spawnSync("sh", ["-n", "-c", command], { encoding: "utf-8" });
  expect(result.status, result.stderr).toBe(0);
}

function extractRemoteShellPayload(remoteShellCommand: string): string {
  const result = spawnSync(
    "sh",
    ["-c", `set -- ${remoteShellCommand}; printf '%s' "$3"`],
    { encoding: "utf-8" },
  );
  expect(result.status, result.stderr).toBe(0);
  expect(result.stdout).not.toBe("");
  return result.stdout;
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
    expect(newSessionCmd).toContain("-s 'feat-login'");
    expect(newSessionCmd).toContain("-c '/workspace'");
    const newSessionCall = mockExecSync.mock.calls.find((call) => String(call[0]).includes("new-session"));
    expect(newSessionCall?.[1]).toEqual(expect.objectContaining({
      env: expect.objectContaining({
        PATH: process.platform === "darwin"
          ? expect.stringContaining("/opt/homebrew/bin")
          : expect.any(String),
      }),
    }));
  });

  it("should apply tmux pane layout commands when creating a local session", async () => {
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
      "task-layout",
      SessionType.TMUX,
      "feat-login",
      createMockWs(),
      "/workspace",
      120,
      30,
      {
        layoutType: PaneLayoutType.VERTICAL_2,
        panes: [
          { position: 0, command: "pnpm dev" },
          { position: 1, command: "pnpm test" },
        ],
      },
    );

    // Then
    const bootstrapCmd = findExecSyncCall("new-session");
    expect(bootstrapCmd).toContain("tmux new-session -d -s 'feat-login' -c '/workspace'");
    expect(bootstrapCmd).toContain("split-window -h -t 'feat-login:0' -c '/workspace'");
    expect(bootstrapCmd).toContain("send-keys -t 'feat-login:0.0' -- 'pnpm dev' Enter");
    expect(bootstrapCmd).toContain("send-keys -t 'feat-login:0.1' -- 'pnpm test' Enter");
    /** 사용자 설정이 세션을 지우기 전에 옵션이 적용되도록 한 번의 tmux 호출로 묶여야 한다 */
    expect(String(bootstrapCmd).split("tmux ").length - 1).toBe(1);
  });

  it("should keep a local session alive when the user tmux config destroys unattached sessions", async () => {
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
      "task-hardening",
      SessionType.TMUX,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    const bootstrapCmd = findExecSyncCall("new-session");
    expect(bootstrapCmd).toContain("set-option -t 'feat-login' destroy-unattached off");
  });

  it("should scope the terminal size option to the KanVibe session instead of the user tmux server", async () => {
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
      "task-window-size",
      SessionType.TMUX,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    const executedCommands = mockExecSync.mock.calls.map((call) => String(call[0]));
    expect(executedCommands.some((command) => command.includes("set-option -g window-size"))).toBe(false);
    expect(findExecSyncCall("new-session")).toContain("set-option -t 'feat-login' window-size latest");
  });

  it("should retry local bootstrap without the user tmux config when the first attempt fails", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    let newSessionAttempts = 0;
    mockExecSync.mockImplementation((cmd: string) => {
      const command = String(cmd);
      if (command.includes("has-session")) {
        throw new Error("session not found");
      }
      if (command.includes("new-session")) {
        newSessionAttempts += 1;
        if (newSessionAttempts === 1) {
          throw new Error("no sessions");
        }
      }
      return "";
    });

    // When
    await attachLocalSession(
      "task-retry",
      SessionType.TMUX,
      "feat-login",
      createMockWs(),
      "/workspace",
    );

    // Then
    const bootstrapCommands = mockExecSync.mock.calls
      .map((call) => String(call[0]))
      .filter((command) => command.includes("new-session"));
    expect(bootstrapCommands).toHaveLength(2);
    expect(bootstrapCommands[0]).not.toContain("-f /dev/null");
    expect(bootstrapCommands[1]).toContain("tmux -f /dev/null new-session");
  });

  it("should build POSIX-valid local tmux bootstrap commands for multiline quoted pane commands", async () => {
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
      "task-layout-quoted",
      SessionType.TMUX,
      "feat-login",
      createMockWs(),
      "/workspace",
      120,
      30,
      {
        layoutType: PaneLayoutType.VERTICAL_2,
        panes: [
          {
            position: 0,
            command: "nvm use 24 && \nnode -e \"console.log('ok')\"",
          },
        ],
      },
    );

    // Then
    const bootstrapCmd = findExecSyncCall("new-session");
    expect(bootstrapCmd).toBeDefined();
    expectValidPosixShellSyntax(bootstrapCmd ?? "");
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
      expect.objectContaining({
        env: expect.objectContaining({
          PATH: process.platform === "darwin"
            ? expect.stringContaining("/opt/homebrew/bin")
            : expect.any(String),
        }),
      }),
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
    await expect(attachLocalSession(
      "task-4",
      SessionType.TMUX,
      "feat-login",
      ws,
      "/workspace",
    )).rejects.toThrow("tmux 세션 생성에 실패했습니다.");

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

  it("should treat an exited local zellij session as missing so it can be recreated", async () => {
    // Given
    const { attachLocalSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");
    mockZellijSessions("EXITED: feat-login\n");
    mockExistsSync.mockReturnValue(false);

    // When
    await attachLocalSession(
      "task-z6",
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
});

describe("attachRemoteSession — zellij 원격 세션", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it("should create the remote zellij session with its layout when attach finds no session", async () => {
    // Given
    const { attachRemoteSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");

    // When
    await attachRemoteSession(
      "task-remote-zellij",
      "remote-host",
      SessionType.ZELLIJ,
      "remote-session",
      createMockWs(),
      {
        host: "remote-host",
        hostname: "example.com",
        port: 2202,
        username: "tester",
        privateKeyPath: "/tmp/test-key",
      },
      120,
      30,
      "/remote/worktree",
    );

    // Then
    const sshArgs = vi.mocked(nodePty.spawn).mock.calls[0][1] as string[];
    const remoteShellCommand = sshArgs.at(-1) ?? "";
    const attachCommand = extractRemoteShellPayload(remoteShellCommand);
    expect(attachCommand).toContain("zellij attach 'remote-session'");
    expect(attachCommand).toContain("exec zellij --session 'remote-session' --new-session-with-layout '/remote/worktree/.zellij-layout.kdl'");
    expect(attachCommand).toContain("exec zellij --session 'remote-session'");
    expectValidPosixShellSyntax(remoteShellCommand);
    expectValidPosixShellSyntax(attachCommand);
  });

  it("should still create the remote zellij session when no worktree path is known", async () => {
    // Given
    const { attachRemoteSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");

    // When
    await attachRemoteSession(
      "task-remote-zellij-no-worktree",
      "remote-host",
      SessionType.ZELLIJ,
      "remote-session",
      createMockWs(),
      {
        host: "remote-host",
        hostname: "example.com",
        port: 2202,
        username: "tester",
        privateKeyPath: "/tmp/test-key",
      },
      120,
      30,
      null,
    );

    // Then
    const sshArgs = vi.mocked(nodePty.spawn).mock.calls[0][1] as string[];
    const attachCommand = extractRemoteShellPayload(sshArgs.at(-1) ?? "");
    expect(attachCommand).toContain("zellij attach 'remote-session' 2>/dev/null || exec zellij --session 'remote-session'");
    expectValidPosixShellSyntax(attachCommand);
  });
});

describe("focusSession — 렌더러 포커스 처리", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it("상세 탭 포커스가 tmux 클라이언트를 전환하지 않는다", async () => {
    // Given
    const { attachLocalSession, focusSession } = await import("@/lib/terminal");
    mockExecSync.mockReturnValue("");

    await attachLocalSession(
      "task-focus",
      SessionType.TMUX,
      "feat-login",
      createMockWs(),
      "/workspace",
    );
    mockExecSync.mockClear();

    // When
    focusSession("task-focus");

    // Then
    expect(findExecSyncCall("switch-client")).toBeUndefined();
  });
});

describe("attachRemoteSession — ssh 바이너리 기반 연결", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
  });

  it("should spawn ssh with tty options for remote tmux attach", async () => {
    // Given
    const originalDisplay = process.env.DISPLAY;
    delete process.env.DISPLAY;
    const { attachRemoteSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");

    try {
      // When
      await attachRemoteSession(
        "task-r1",
        "remote-host",
        SessionType.TMUX,
        "remote-session",
        createMockWs(),
        {
          host: "remote-host",
          hostname: "example.com",
          port: 2202,
          username: "tester",
          privateKeyPath: "/tmp/test-key",
        },
        120,
        30,
        "/remote/worktree",
      );

      // Then
      expect(nodePty.spawn).toHaveBeenCalledWith(
        "ssh",
        expect.any(Array),
        expect.objectContaining({ cwd: expect.any(String) }),
      );
      const sshArgs = vi.mocked(nodePty.spawn).mock.calls[0][1] as string[];
      expect(sshArgs.slice(0, -1)).toEqual([
        "-i",
        "/tmp/test-key",
        "-p",
        "2202",
        "-o",
        "BatchMode=yes",
        "-o",
        "IdentitiesOnly=yes",
        "-tt",
        "-o",
        "ConnectTimeout=8",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=2",
        "remote-host",
      ]);
      expect(sshArgs).not.toContain("-Y");
      expect(sshArgs).not.toContain("ControlMaster=auto");
      const attachCommand = extractRemoteShellPayload(sshArgs.at(-1) ?? "");
      expect(attachCommand).toContain("tmux has-session -t 'remote-session'");
      expect(attachCommand).toContain("tmux new-session -d -s 'remote-session' -c '/remote/worktree'");
      expect(mockPtyWrite).not.toHaveBeenCalled();
    } finally {
      if (originalDisplay === undefined) {
        delete process.env.DISPLAY;
      } else {
        process.env.DISPLAY = originalDisplay;
      }
    }
  });

  it("should build a POSIX-sh-valid remote tmux attach command", async () => {
    // Given
    const originalDisplay = process.env.DISPLAY;
    delete process.env.DISPLAY;
    const { attachRemoteSession } = await import("@/lib/terminal");

    try {
      // When
      await attachRemoteSession(
        "task-r-shell",
        "remote-host",
        SessionType.TMUX,
        "remote-session",
        createMockWs(),
        {
          host: "remote-host",
          hostname: "example.com",
          port: 2202,
          username: "tester",
          privateKeyPath: "/tmp/test-key",
        },
        120,
        30,
        "/remote/worktree",
      );

      // Then
      const sshArgs = vi.mocked((await import("node-pty")).spawn).mock.calls[0][1] as string[];
      const remoteShellCommand = sshArgs.at(-1) ?? "";
      const attachCommand = extractRemoteShellPayload(remoteShellCommand);
      expectValidPosixShellSyntax(remoteShellCommand);
      expectValidPosixShellSyntax(attachCommand);
      expect(mockPtyWrite).not.toHaveBeenCalled();
    } finally {
      if (originalDisplay === undefined) {
        delete process.env.DISPLAY;
      } else {
        process.env.DISPLAY = originalDisplay;
      }
    }
  });

  it("should pass remote tmux bootstrap as an SSH command instead of typing it into the shell", async () => {
    // Given
    const originalDisplay = process.env.DISPLAY;
    delete process.env.DISPLAY;
    const { attachRemoteSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");

    try {
      // When
      await attachRemoteSession(
        "task-r-interactive",
        "remote-host",
        SessionType.TMUX,
        "remote-session",
        createMockWs(),
        {
          host: "remote-host",
          hostname: "example.com",
          port: 2202,
          username: "tester",
          privateKeyPath: "/tmp/test-key",
        },
        120,
        30,
        "/remote/worktree",
      );

      // Then
      const sshArgs = vi.mocked(nodePty.spawn).mock.calls[0][1] as string[];
      expect(sshArgs.at(-2)).toBe("remote-host");
      expect(sshArgs.at(-1)).toEqual(expect.stringMatching(/^sh -lc /));
      expect(extractRemoteShellPayload(sshArgs.at(-1) ?? "")).toContain("tmux has-session -t 'remote-session'");
      expect(mockPtyWrite).not.toHaveBeenCalled();
    } finally {
      if (originalDisplay === undefined) {
        delete process.env.DISPLAY;
      } else {
        process.env.DISPLAY = originalDisplay;
      }
    }
  });

  it("should request trusted X11 forwarding only when a local DISPLAY exists", async () => {
    // Given
    const originalDisplay = process.env.DISPLAY;
    process.env.DISPLAY = ":0";
    const { attachRemoteSession } = await import("@/lib/terminal");
    const nodePty = await import("node-pty");

    try {
      // When
      await attachRemoteSession(
        "task-r-x11",
        "remote-host",
        SessionType.TMUX,
        "remote-session",
        createMockWs(),
        {
          host: "remote-host",
          hostname: "example.com",
          port: 2202,
          username: "tester",
          privateKeyPath: "/tmp/test-key",
        },
      );

      // Then
      const sshArgs = vi.mocked(nodePty.spawn).mock.calls[0][1] as string[];
      expect(sshArgs).toContain("-Y");
      expect(sshArgs.indexOf("-Y")).toBeLessThan(sshArgs.indexOf("remote-host"));
    } finally {
      if (originalDisplay === undefined) {
        delete process.env.DISPLAY;
      } else {
        process.env.DISPLAY = originalDisplay;
      }
    }
  });

  it("should apply tmux pane layout commands when creating a remote session", async () => {
    const { attachRemoteSession } = await import("@/lib/terminal");

    await attachRemoteSession(
      "task-r2",
      "remote-host",
      SessionType.TMUX,
      "remote-session",
      createMockWs(),
      {
        host: "remote-host",
        hostname: "example.com",
        port: 2202,
        username: "tester",
        privateKeyPath: "/tmp/test-key",
      },
      120,
      30,
      "/remote/worktree",
      {
        layoutType: PaneLayoutType.VERTICAL_2,
        panes: [
          { position: 0, command: "pnpm dev" },
          { position: 1, command: "pnpm test" },
        ],
      },
    );

    const nodePty = await import("node-pty");
    const sshArgs = vi.mocked(nodePty.spawn).mock.calls[0][1] as string[];
    const attachCommand = extractRemoteShellPayload(sshArgs.at(-1) ?? "");
    expect(attachCommand).toContain("split-window -h -t 'remote-session:0' -c '/remote/worktree'");
    expect(attachCommand).toContain("send-keys -t 'remote-session:0.0' -- 'pnpm dev' Enter");
    expect(attachCommand).toContain("send-keys -t 'remote-session:0.1' -- 'pnpm test' Enter");
    expect(mockPtyWrite).not.toHaveBeenCalled();
  });

  it("should bootstrap and attach the default tmux socket in one hardened command sequence", async () => {
    const { attachRemoteSession } = await import("@/lib/terminal");

    await attachRemoteSession(
      "task-r-tmux-default-socket",
      "remote-host",
      SessionType.TMUX,
      "remote-session",
      createMockWs(),
      {
        host: "remote-host",
        hostname: "example.com",
        port: 2202,
        username: "tester",
        privateKeyPath: "/tmp/test-key",
      },
      120,
      30,
      "/remote/worktree",
      {
        layoutType: PaneLayoutType.VERTICAL_2,
        panes: [
          { position: 0, command: "pnpm dev" },
        ],
      },
    );

    const nodePty = await import("node-pty");
    const sshArgs = vi.mocked(nodePty.spawn).mock.calls[0][1] as string[];
    const remoteShellCommand = sshArgs.at(-1) ?? "";
    const attachCommand = extractRemoteShellPayload(remoteShellCommand);

    /** 사용자가 원격에서 직접 tmux attach를 쳐도 같은 세션에 붙도록 기본 소켓만 쓴다 */
    expect(attachCommand).not.toContain("-L 'kanvibe'");
    expect(attachCommand).toContain("if tmux has-session -t 'remote-session' 2>/dev/null;");
    expect(attachCommand).toContain("exec tmux attach-session -t 'remote-session';");
    /** 생성부터 attach까지가 하나의 tmux 명령 시퀀스여야 사용자 설정이 세션을 지우기 전에 옵션이 적용된다 */
    expect(attachCommand).toContain(
      "tmux new-session -d -s 'remote-session' -c '/remote/worktree' \\; set-option -t 'remote-session' destroy-unattached off",
    );
    expect(attachCommand).toContain("\\; attach-session -t 'remote-session'");
    /** 사용자 설정 때문에 실패하면 소켓은 그대로 둔 채 설정 없이 한 번 더 시도한다 */
    expect(attachCommand).toContain("tmux -f /dev/null new-session -d -s 'remote-session'");
    expect(attachCommand).not.toContain("tmux default server failed");
    expect(attachCommand).not.toContain("kanvibe-fallback");
    expect(attachCommand.match(/has-session/g)).toHaveLength(1);
    expect(attachCommand).toContain("tmux session setup failed; leaving the SSH shell open");
    expect(attachCommand).toContain('exec "${SHELL:-/bin/sh}" -l');
    expect(Buffer.byteLength(attachCommand)).toBeLessThan(1_500);
    expectValidPosixShellSyntax(remoteShellCommand);
    expectValidPosixShellSyntax(attachCommand);
    expect(mockPtyWrite).not.toHaveBeenCalled();
  });

  it("should build POSIX-valid remote tmux attach commands for multiline quoted pane commands", async () => {
    const { attachRemoteSession } = await import("@/lib/terminal");

    await attachRemoteSession(
      "task-r-quoted",
      "remote-host",
      SessionType.TMUX,
      "remote-session",
      createMockWs(),
      {
        host: "remote-host",
        hostname: "example.com",
        port: 2202,
        username: "tester",
        privateKeyPath: "/tmp/test-key",
      },
      120,
      30,
      "/remote/worktree",
      {
        layoutType: PaneLayoutType.VERTICAL_2,
        panes: [
          {
            position: 0,
            command: "nvm use 24 && \nnode -e \"console.log('ok')\"",
          },
        ],
      },
    );

    const nodePty = await import("node-pty");
    const sshArgs = vi.mocked(nodePty.spawn).mock.calls[0][1] as string[];
    const remoteShellCommand = sshArgs.at(-1) ?? "";
    const attachCommand = extractRemoteShellPayload(remoteShellCommand);
    expectValidPosixShellSyntax(remoteShellCommand);
    expectValidPosixShellSyntax(attachCommand);
    expect(mockPtyWrite).not.toHaveBeenCalled();
  });

  it("should keep long remote tmux bootstrap text out of the login prompt", async () => {
    const { attachRemoteSession } = await import("@/lib/terminal");
    const sessionName = "2026-06-toss-node-developer-agent-2026-06-toss-node-developer-export-hermes-session-history-20260605-205232";

    await attachRemoteSession(
      "task-r-long-bootstrap",
      "remote-host",
      SessionType.TMUX,
      sessionName,
      createMockWs(),
      {
        host: "remote-host",
        hostname: "example.com",
        port: 2202,
        username: "tester",
        privateKeyPath: "/tmp/test-key",
      },
      120,
      30,
      "/home/rookedsysc/Documents/toss/2026-06-toss-node-developer__worktrees/agent-2026-06-toss-node-developer-export-hermes-session-history-20260605-205232",
      {
        layoutType: PaneLayoutType.VERTICAL_2,
        panes: [
          { position: 0, command: "nvm use 24" },
        ],
      },
    );

    const nodePty = await import("node-pty");
    const sshArgs = vi.mocked(nodePty.spawn).mock.calls[0][1] as string[];
    const remoteShellCommand = sshArgs.at(-1) ?? "";
    const attachCommand = extractRemoteShellPayload(remoteShellCommand);
    expect(sshArgs.at(-2)).toBe("remote-host");
    expect(remoteShellCommand).toEqual(expect.stringMatching(/^sh -lc /));
    expect(attachCommand).toContain("nvm use 24");
    expect(attachCommand).not.toContain("-L 'kanvibe'");
    expect(attachCommand).not.toContain("kanvibe-fallback");
    expect(attachCommand).not.toContain("tmux default server failed");
    expect(attachCommand).toContain(sessionName);
    expectValidPosixShellSyntax(remoteShellCommand);
    expectValidPosixShellSyntax(attachCommand);
    expect(mockPtyWrite).not.toHaveBeenCalled();
  });
});
