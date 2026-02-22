// @vitest-environment node
import { describe, it, expect, vi, beforeEach } from "vitest";
import { PaneLayoutType, type PaneCommand } from "@/entities/PaneLayoutConfig";
import { SessionType } from "@/entities/KanbanTask";

// --- Mocks ---

const mockExecGit = vi.fn().mockResolvedValue("");

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

const mockGetEffectivePaneLayout = vi.fn();

vi.mock("@/app/actions/paneLayout", () => ({
  getEffectivePaneLayout: (...args: unknown[]) =>
    mockGetEffectivePaneLayout(...args),
}));

const mockWriteFile = vi.fn().mockResolvedValue(undefined);

vi.mock("fs/promises", () => ({
  writeFile: (...args: unknown[]) => mockWriteFile(...args),
}));

/** execGit 호출 중 특정 패턴을 포함하는 명령어만 필터링한다 */
function filterCalls(pattern: string | RegExp): string[] {
  return mockExecGit.mock.calls
    .map((call) => call[0] as string)
    .filter((cmd) =>
      typeof pattern === "string" ? cmd.includes(pattern) : pattern.test(cmd),
    );
}

describe("formatSessionName", () => {
  it("should format as projectName-branchName with slashes replaced", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("kanvibe", "feat/something");

    // Then
    expect(result).toBe("kanvibe-feat-something");
  });

  it("should handle multiple slashes in branch name", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("kanvibe", "feat/ui/button");

    // Then
    expect(result).toBe("kanvibe-feat-ui-button");
  });

  it("should handle branch name without slashes", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("kanvibe", "main");

    // Then
    expect(result).toBe("kanvibe-main");
  });

  it("should replace slashes in project name as well", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("parent/project", "feat/test");

    // Then
    expect(result).toBe("parent-project-feat-test");
  });
});

describe("sanitizeZellijSessionName", () => {
  it("should return sessionName unchanged when within length limit", async () => {
    // Given
    const { sanitizeZellijSessionName } = await import("@/lib/worktree");
    const shortName = "feat-login";

    // When
    const result = sanitizeZellijSessionName(shortName);

    // Then
    expect(result).toBe(shortName);
  });

  it("should truncate sessionName exceeding 60 characters", async () => {
    // Given
    const { sanitizeZellijSessionName } = await import("@/lib/worktree");
    const longName = "a".repeat(80);

    // When
    const result = sanitizeZellijSessionName(longName);

    // Then
    expect(result).toHaveLength(60);
    expect(result).toBe("a".repeat(60));
  });

  it("should return exact 60-char sessionName unchanged", async () => {
    // Given
    const { sanitizeZellijSessionName } = await import("@/lib/worktree");
    const exactName = "x".repeat(60);

    // When
    const result = sanitizeZellijSessionName(exactName);

    // Then
    expect(result).toBe(exactName);
  });
});

describe("isSessionAlive", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should return true when tmux session exists", async () => {
    // Given
    mockExecGit.mockResolvedValue("");
    const { isSessionAlive } = await import("@/lib/worktree");

    // When
    const result = await isSessionAlive(SessionType.TMUX, "feat-branch");

    // Then
    expect(result).toBe(true);
    expect(mockExecGit).toHaveBeenCalledWith(
      'tmux has-session -t "feat-branch" 2>/dev/null',
      undefined,
    );
  });

  it("should return false when tmux session does not exist", async () => {
    // Given
    mockExecGit.mockRejectedValue(new Error("no session"));
    const { isSessionAlive } = await import("@/lib/worktree");

    // When
    const result = await isSessionAlive(SessionType.TMUX, "nonexistent");

    // Then
    expect(result).toBe(false);
  });

  it("should return true when zellij session exists", async () => {
    // Given
    mockExecGit.mockResolvedValue("feat-branch\nother-session\n");
    const { isSessionAlive } = await import("@/lib/worktree");

    // When
    const result = await isSessionAlive(SessionType.ZELLIJ, "feat-branch");

    // Then
    expect(result).toBe(true);
  });

  it("should return false when zellij session does not exist", async () => {
    // Given
    mockExecGit.mockResolvedValue("other-session\n");
    const { isSessionAlive } = await import("@/lib/worktree");

    // When
    const result = await isSessionAlive(SessionType.ZELLIJ, "nonexistent");

    // Then
    expect(result).toBe(false);
  });

  it("should pass sshHost to execGit for remote session check", async () => {
    // Given
    mockExecGit.mockResolvedValue("");
    const { isSessionAlive } = await import("@/lib/worktree");

    // When
    await isSessionAlive(SessionType.TMUX, "feat-branch", "remote-host");

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      'tmux has-session -t "feat-branch" 2>/dev/null',
      "remote-host",
    );
  });
});

describe("removeSessionOnly", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should kill tmux session by session name", async () => {
    // Given
    mockExecGit.mockResolvedValue("");
    const { removeSessionOnly } = await import("@/lib/worktree");

    // When
    await removeSessionOnly(SessionType.TMUX, "feat-branch");

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      'tmux kill-session -t "feat-branch"',
      undefined,
    );
  });

  it("should kill zellij session with fallback to delete", async () => {
    // Given
    mockExecGit.mockResolvedValue("");
    const { removeSessionOnly } = await import("@/lib/worktree");

    // When
    await removeSessionOnly(SessionType.ZELLIJ, "feat-branch");

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      'zellij kill-session "feat-branch" 2>/dev/null || zellij delete-session "feat-branch" 2>/dev/null',
      undefined,
    );
  });

  it("should not throw when session is already terminated", async () => {
    // Given
    mockExecGit.mockRejectedValue(new Error("session not found"));
    const { removeSessionOnly } = await import("@/lib/worktree");

    // When & Then
    await expect(removeSessionOnly(SessionType.TMUX, "dead-session")).resolves.not.toThrow();
  });
});

describe("createSessionWithoutWorktree", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should return session name derived from branch name", async () => {
    // Given
    mockExecGit.mockRejectedValueOnce(new Error("no session")).mockResolvedValue("");
    const { createSessionWithoutWorktree } = await import("@/lib/worktree");

    // When
    const result = await createSessionWithoutWorktree(
      "/repo/path",
      "feat/my-feature",
      SessionType.TMUX,
    );

    // Then
    expect(result.sessionName).toBe("path-feat-my-feature");
  });

  it("should create tmux session with working directory when session does not exist", async () => {
    // Given
    mockExecGit.mockRejectedValueOnce(new Error("no session")).mockResolvedValue("");
    const { createSessionWithoutWorktree } = await import("@/lib/worktree");

    // When
    await createSessionWithoutWorktree(
      "/repo/path",
      "feat/test",
      SessionType.TMUX,
      null,
      "/custom/dir",
    );

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      'tmux new-session -d -s "path-feat-test" -c "/custom/dir"',
      null,
    );
  });

  it("should skip session creation when tmux session already exists", async () => {
    // Given
    mockExecGit.mockResolvedValue("");
    const { createSessionWithoutWorktree } = await import("@/lib/worktree");

    // When
    await createSessionWithoutWorktree(
      "/repo/path",
      "main",
      SessionType.TMUX,
    );

    // Then
    /** isSessionAlive → has-session 호출 1회만 발생, new-session은 호출되지 않는다 */
    expect(mockExecGit).toHaveBeenCalledTimes(1);
    expect(mockExecGit).toHaveBeenCalledWith(
      'tmux has-session -t "path-main" 2>/dev/null',
      undefined,
    );
  });
});

describe("generateZellijLayoutKdl", () => {
  const worktreePath = "/home/user/kanvibe__worktrees/feat-branch";

  async function importGenerateZellijLayoutKdl() {
    const mod = await import("@/lib/worktree");
    return mod.generateZellijLayoutKdl;
  }

  it("should generate single pane layout with command", async () => {
    // Given
    const panes: PaneCommand[] = [{ position: 0, command: "echo hello" }];
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.SINGLE, panes, worktreePath);

    // Then
    expect(kdl).toContain("layout {");
    expect(kdl).toContain('command="bash"');
    expect(kdl).toContain('"echo hello"');
    /** KDL child-brace 내부에서는 cwd "path" 형태를 사용한다 */
    expect(kdl).toContain(`cwd "${worktreePath}"`);
    expect(kdl).not.toContain("split_direction");
  });

  it("should generate single pane layout without command", async () => {
    // Given
    const panes: PaneCommand[] = [{ position: 0, command: "" }];
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.SINGLE, panes, worktreePath);

    // Then
    expect(kdl).toContain(`pane cwd="${worktreePath}"`);
    expect(kdl).not.toContain('command="bash"');
  });

  it("should generate horizontal 2-pane layout (top/bottom)", async () => {
    // Given
    const panes: PaneCommand[] = [
      { position: 0, command: "pnpm dev" },
      { position: 1, command: "pnpm test" },
    ];
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.HORIZONTAL_2, panes, worktreePath);

    // Then
    expect(kdl).toContain('"pnpm dev"');
    expect(kdl).toContain('"pnpm test"');
    /** 수평 2분할은 split_direction 없이 기본 horizontal 사용 */
    expect(kdl).not.toContain("split_direction");
  });

  it("should generate vertical 2-pane layout (left/right)", async () => {
    // Given
    const panes: PaneCommand[] = [
      { position: 0, command: "pnpm dev" },
      { position: 1, command: "pnpm test" },
    ];
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.VERTICAL_2, panes, worktreePath);

    // Then
    expect(kdl).toContain('split_direction="vertical"');
    expect(kdl).toContain('"pnpm dev"');
    expect(kdl).toContain('"pnpm test"');
  });

  it("should generate LEFT_RIGHT_TB layout (left + right-top/bottom)", async () => {
    // Given
    const panes: PaneCommand[] = [
      { position: 0, command: "claude --dangerously-skip-permissions" },
      { position: 1, command: "pnpm dev" },
      { position: 2, command: "pnpm test:watch" },
    ];
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.LEFT_RIGHT_TB, panes, worktreePath);

    // Then
    expect(kdl).toContain('split_direction="vertical"');
    expect(kdl).toContain('"claude --dangerously-skip-permissions"');
    expect(kdl).toContain('"pnpm dev"');
    expect(kdl).toContain('"pnpm test:watch"');
    /** position 0은 좌측 단독, position 1/2는 우측 상하 분할 */
    const lines = kdl.split("\n");
    const verticalContainerIdx = lines.findIndex((l) => l.includes('split_direction="vertical"'));
    expect(verticalContainerIdx).toBeGreaterThan(-1);
  });

  it("should generate LEFT_TB_RIGHT layout (left-top/bottom + right)", async () => {
    // Given
    const panes: PaneCommand[] = [
      { position: 0, command: "pnpm dev" },
      { position: 1, command: "pnpm test" },
      { position: 2, command: "claude" },
    ];
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.LEFT_TB_RIGHT, panes, worktreePath);

    // Then
    expect(kdl).toContain('split_direction="vertical"');
    expect(kdl).toContain('"pnpm dev"');
    expect(kdl).toContain('"pnpm test"');
    expect(kdl).toContain('"claude"');
  });

  it("should generate QUAD layout (2x2 grid)", async () => {
    // Given
    const panes: PaneCommand[] = [
      { position: 0, command: "cmd0" },
      { position: 1, command: "cmd1" },
      { position: 2, command: "cmd2" },
      { position: 3, command: "cmd3" },
    ];
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.QUAD, panes, worktreePath);

    // Then
    expect(kdl).toContain('split_direction="vertical"');
    expect(kdl).toContain('"cmd0"');
    expect(kdl).toContain('"cmd1"');
    expect(kdl).toContain('"cmd2"');
    expect(kdl).toContain('"cmd3"');
  });

  it("should skip command for panes with empty command string", async () => {
    // Given
    const panes: PaneCommand[] = [
      { position: 0, command: "pnpm dev" },
      { position: 1, command: "" },
    ];
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.VERTICAL_2, panes, worktreePath);

    // Then
    /** position 0은 command가 있고, position 1은 기본 shell pane이다 */
    expect(kdl).toContain('"pnpm dev"');
    const paneLines = kdl.split("\n").filter((l) => l.trim().startsWith("pane"));
    const plainPanes = paneLines.filter(
      (l) => l.includes("cwd=") && !l.includes("command=") && !l.includes("split_direction"),
    );
    expect(plainPanes.length).toBeGreaterThanOrEqual(1);
  });

  it("should escape special characters in paths and commands", async () => {
    // Given
    const panes: PaneCommand[] = [
      { position: 0, command: 'echo "hello world"' },
    ];
    const pathWithBackslash = "/home/user/my\\project";
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.SINGLE, panes, pathWithBackslash);

    // Then
    expect(kdl).toContain("my\\\\project");
    expect(kdl).toContain('\\"hello world\\"');
  });

  it("should set cwd on all panes", async () => {
    // Given
    const panes: PaneCommand[] = [
      { position: 0, command: "cmd0" },
      { position: 1, command: "" },
      { position: 2, command: "cmd2" },
    ];
    const generateZellijLayoutKdl = await importGenerateZellijLayoutKdl();

    // When
    const kdl = generateZellijLayoutKdl(PaneLayoutType.LEFT_RIGHT_TB, panes, worktreePath);

    // Then
    const cwdCount = (kdl.match(new RegExp(worktreePath, "g")) || []).length;
    expect(cwdCount).toBe(3);
  });
});

describe("createWorktreeWithSession — Zellij KDL layout file persistence", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should write layout file to worktree directory without starting zellij", async () => {
    // Given
    mockGetEffectivePaneLayout.mockResolvedValue({
      layoutType: PaneLayoutType.VERTICAL_2,
      panes: [
        { position: 0, command: "pnpm dev" },
        { position: 1, command: "pnpm test" },
      ],
    });

    const { createWorktreeWithSession } = await import("@/lib/worktree");

    // When
    await createWorktreeWithSession(
      "/home/user/kanvibe",
      "feat-new",
      "main",
      SessionType.ZELLIJ,
      null,
      "project-1",
    );

    // Then
    /** writeFile이 worktree 디렉토리에 .zellij-layout.kdl로 호출되어야 한다 */
    expect(mockWriteFile).toHaveBeenCalledTimes(1);
    const [writtenPath, writtenContent] = mockWriteFile.mock.calls[0];
    expect(writtenPath).toContain("kanvibe__worktrees/feat-new");
    expect(writtenPath).toContain(".zellij-layout.kdl");
    expect(writtenContent).toContain('split_direction="vertical"');
    expect(writtenContent).toContain('"pnpm dev"');

    /** zellij를 서버에서 직접 시작하지 않아야 한다 (TTY 없이 실행 불가) */
    const zellijCalls = filterCalls("zellij");
    expect(zellijCalls).toHaveLength(0);
  });

  it("should not write layout file for SINGLE layout type", async () => {
    // Given
    mockGetEffectivePaneLayout.mockResolvedValue({
      layoutType: PaneLayoutType.SINGLE,
      panes: [{ position: 0, command: "echo hello" }],
    });

    const { createWorktreeWithSession } = await import("@/lib/worktree");

    // When
    await createWorktreeWithSession(
      "/home/user/kanvibe",
      "feat-new",
      "main",
      SessionType.ZELLIJ,
      null,
      "project-1",
    );

    // Then
    expect(mockWriteFile).not.toHaveBeenCalled();
  });

  it("should not write layout file for remote Zellij sessions", async () => {
    // Given
    mockGetEffectivePaneLayout.mockResolvedValue({
      layoutType: PaneLayoutType.VERTICAL_2,
      panes: [
        { position: 0, command: "pnpm dev" },
        { position: 1, command: "pnpm test" },
      ],
    });

    const { createWorktreeWithSession } = await import("@/lib/worktree");

    // When
    await createWorktreeWithSession(
      "/home/user/kanvibe",
      "feat-new",
      "main",
      SessionType.ZELLIJ,
      "remote-host",
      "project-1",
    );

    // Then
    expect(mockWriteFile).not.toHaveBeenCalled();
    expect(mockGetEffectivePaneLayout).not.toHaveBeenCalled();
  });

  it("should fallback gracefully when getEffectivePaneLayout fails", async () => {
    // Given
    mockGetEffectivePaneLayout.mockRejectedValue(new Error("DB error"));

    const { createWorktreeWithSession } = await import("@/lib/worktree");

    // When & Then
    await expect(
      createWorktreeWithSession(
        "/home/user/kanvibe",
        "feat-new",
        "main",
        SessionType.ZELLIJ,
        null,
        "project-1",
      ),
    ).resolves.not.toThrow();

    /** 레이아웃 파일 없이 세션 이름만 반환되어야 한다 */
    expect(mockWriteFile).not.toHaveBeenCalled();
  });

  it("should write KDL layout with 3-pane configuration to worktree", async () => {
    // Given
    mockGetEffectivePaneLayout.mockResolvedValue({
      layoutType: PaneLayoutType.LEFT_RIGHT_TB,
      panes: [
        { position: 0, command: "claude --dangerously-skip-permissions" },
        { position: 1, command: "pnpm dev" },
        { position: 2, command: "pnpm test:watch" },
      ],
    });

    const { createWorktreeWithSession } = await import("@/lib/worktree");

    // When
    await createWorktreeWithSession(
      "/home/user/kanvibe",
      "feat-new",
      "main",
      SessionType.ZELLIJ,
      null,
      "project-1",
    );

    // Then
    expect(mockWriteFile).toHaveBeenCalledTimes(1);
    const [, writtenContent] = mockWriteFile.mock.calls[0];
    expect(writtenContent).toContain('"claude --dangerously-skip-permissions"');
    expect(writtenContent).toContain('"pnpm dev"');
    expect(writtenContent).toContain('"pnpm test:watch"');
  });

  it("should write KDL layout with 4-pane QUAD configuration to worktree", async () => {
    // Given
    mockGetEffectivePaneLayout.mockResolvedValue({
      layoutType: PaneLayoutType.QUAD,
      panes: [
        { position: 0, command: "cmd0" },
        { position: 1, command: "cmd1" },
        { position: 2, command: "cmd2" },
        { position: 3, command: "cmd3" },
      ],
    });

    const { createWorktreeWithSession } = await import("@/lib/worktree");

    // When
    await createWorktreeWithSession(
      "/home/user/kanvibe",
      "feat-new",
      "main",
      SessionType.ZELLIJ,
      null,
      "project-1",
    );

    // Then
    expect(mockWriteFile).toHaveBeenCalledTimes(1);
    const [, writtenContent] = mockWriteFile.mock.calls[0];
    expect(writtenContent).toContain('"cmd0"');
    expect(writtenContent).toContain('"cmd1"');
    expect(writtenContent).toContain('"cmd2"');
    expect(writtenContent).toContain('"cmd3"');
    expect(writtenContent).toContain('split_direction="vertical"');
  });

  it("should not start zellij from server for createSessionWithoutWorktree", async () => {
    // Given
    const { createSessionWithoutWorktree } = await import("@/lib/worktree");

    // When
    const result = await createSessionWithoutWorktree(
      "/repo/path",
      "feat/test",
      SessionType.ZELLIJ,
    );

    // Then
    expect(result.sessionName).toBe("path-feat-test");
    /** zellij를 서버에서 직접 시작하지 않아야 한다 */
    const zellijCalls = filterCalls("zellij");
    expect(zellijCalls).toHaveLength(0);
  });
});
