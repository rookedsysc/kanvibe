/**
 * @vitest-environment node
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SessionType } from "@/entities/KanbanTask";

// --- Mocks ---

const mockExecGit = vi.fn();

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

vi.mock("@/app/actions/paneLayout", () => ({
  getEffectivePaneLayout: vi.fn().mockResolvedValue(null),
}));

describe("formatSessionName", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should combine projectName and branchName with slash separator", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("kanvibe", "feat-login");

    // Then
    expect(result).toBe("kanvibe/feat-login");
  });

  it("should replace slashes in branchName with hyphens", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("kanvibe", "feat/user/auth");

    // Then
    expect(result).toBe("kanvibe/feat-user-auth");
  });

  it("should handle branchName without slashes unchanged", async () => {
    // Given
    const { formatSessionName } = await import("@/lib/worktree");

    // When
    const result = formatSessionName("my-project", "main");

    // Then
    expect(result).toBe("my-project/main");
  });
});

describe("sanitizeZellijSessionName", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should return sessionName unchanged when within length limit", async () => {
    // Given
    const { sanitizeZellijSessionName } = await import("@/lib/worktree");
    const shortName = "kanvibe/feat-login";

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

describe("removeSessionOnly", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should kill tmux session for new format sessionName with slash", async () => {
    // Given
    const { removeSessionOnly } = await import("@/lib/worktree");
    mockExecGit.mockResolvedValue("");

    // When
    await removeSessionOnly(SessionType.TMUX, "kanvibe/feat-login", "feat-login");

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      `tmux kill-session -t "kanvibe/feat-login"`,
      undefined,
    );
  });

  it("should kill tmux window for legacy sessionName without slash", async () => {
    // Given
    const { removeSessionOnly } = await import("@/lib/worktree");
    mockExecGit.mockResolvedValue("");

    // When
    await removeSessionOnly(SessionType.TMUX, "kanvibe", "feat-login");

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      `tmux kill-window -t "kanvibe: feat-login"`,
      undefined,
    );
  });

  it("should kill zellij session for new format sessionName with slash", async () => {
    // Given
    const { removeSessionOnly } = await import("@/lib/worktree");
    mockExecGit.mockResolvedValue("");

    // When
    await removeSessionOnly(SessionType.ZELLIJ, "kanvibe/feat-login", "feat-login");

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      `zellij kill-session "kanvibe/feat-login"`,
      undefined,
    );
  });

  it("should close zellij tab for legacy sessionName without slash", async () => {
    // Given
    const { removeSessionOnly } = await import("@/lib/worktree");
    mockExecGit.mockResolvedValue("");

    // When
    await removeSessionOnly(SessionType.ZELLIJ, "kanvibe", "feat/login");

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      `zellij action --session "kanvibe" go-to-tab-name " feat-login" && zellij action --session "kanvibe" close-tab`,
      undefined,
    );
  });

  it("should pass sshHost to execGit for remote session removal", async () => {
    // Given
    const { removeSessionOnly } = await import("@/lib/worktree");
    mockExecGit.mockResolvedValue("");

    // When
    await removeSessionOnly(SessionType.TMUX, "kanvibe/feat-login", "feat-login", "my-server");

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      `tmux kill-session -t "kanvibe/feat-login"`,
      "my-server",
    );
  });

  it("should silently catch errors when session is already terminated", async () => {
    // Given
    const { removeSessionOnly } = await import("@/lib/worktree");
    mockExecGit.mockRejectedValue(new Error("session not found"));

    // When & Then
    await expect(
      removeSessionOnly(SessionType.TMUX, "kanvibe/feat-login", "feat-login"),
    ).resolves.toBeUndefined();
  });
});

describe("isSessionAlive", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should return true when tmux session exists", async () => {
    // Given
    const { isSessionAlive } = await import("@/lib/worktree");
    mockExecGit.mockResolvedValue("");

    // When
    const result = await isSessionAlive(SessionType.TMUX, "kanvibe/feat-login");

    // Then
    expect(result).toBe(true);
    expect(mockExecGit).toHaveBeenCalledWith(
      `tmux has-session -t "kanvibe/feat-login" 2>/dev/null`,
      undefined,
    );
  });

  it("should return false when tmux session does not exist", async () => {
    // Given
    const { isSessionAlive } = await import("@/lib/worktree");
    mockExecGit.mockRejectedValue(new Error("session not found"));

    // When
    const result = await isSessionAlive(SessionType.TMUX, "kanvibe/nonexistent");

    // Then
    expect(result).toBe(false);
  });

  it("should return true when zellij session exists in list", async () => {
    // Given
    const { isSessionAlive } = await import("@/lib/worktree");
    mockExecGit.mockResolvedValue("kanvibe/feat-login\nother-session\n");

    // When
    const result = await isSessionAlive(SessionType.ZELLIJ, "kanvibe/feat-login");

    // Then
    expect(result).toBe(true);
  });

  it("should return false when zellij session is not in list", async () => {
    // Given
    const { isSessionAlive } = await import("@/lib/worktree");
    mockExecGit.mockResolvedValue("other-session\nanother-session\n");

    // When
    const result = await isSessionAlive(SessionType.ZELLIJ, "kanvibe/feat-login");

    // Then
    expect(result).toBe(false);
  });

  it("should return false when zellij list-sessions command fails", async () => {
    // Given
    const { isSessionAlive } = await import("@/lib/worktree");
    mockExecGit.mockRejectedValue(new Error("zellij not installed"));

    // When
    const result = await isSessionAlive(SessionType.ZELLIJ, "kanvibe/feat-login");

    // Then
    expect(result).toBe(false);
  });

  it("should pass sshHost for remote session check", async () => {
    // Given
    const { isSessionAlive } = await import("@/lib/worktree");
    mockExecGit.mockResolvedValue("");

    // When
    await isSessionAlive(SessionType.TMUX, "kanvibe/feat-login", "my-server");

    // Then
    expect(mockExecGit).toHaveBeenCalledWith(
      `tmux has-session -t "kanvibe/feat-login" 2>/dev/null`,
      "my-server",
    );
  });
});
