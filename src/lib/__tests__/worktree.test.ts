// @vitest-environment node
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

// --- Mocks ---

const mockExecGit = vi.fn();

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

vi.mock("@/entities/PaneLayoutConfig", () => ({
  PaneLayoutType: { SINGLE: "single" },
}));

vi.mock("@/app/actions/paneLayout", () => ({
  getEffectivePaneLayout: vi.fn().mockResolvedValue(null),
}));

describe("formatSessionName", () => {
  it("should replace slashes with hyphens", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("feat/something");

    // Then
    expect(result).toBe("feat-something");
  });

  it("should handle multiple slashes", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("feat/ui/button");

    // Then
    expect(result).toBe("feat-ui-button");
  });

  it("should return the same string when no slashes exist", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("main");

    // Then
    expect(result).toBe("main");
  });

  it("should not add leading space unlike the old formatWindowName", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("feat/test");

    // Then
    expect(result).not.toMatch(/^\s/);
    expect(result).toBe("feat-test");
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
    expect(result.sessionName).toBe("feat-my-feature");
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
      'tmux new-session -d -s "feat-test" -c "/custom/dir"',
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
      'tmux has-session -t "main" 2>/dev/null',
      undefined,
    );
  });
});
