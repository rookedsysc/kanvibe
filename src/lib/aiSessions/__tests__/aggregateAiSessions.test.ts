import { describe, expect, it, vi, beforeEach } from "vitest";

const {
  mockReadClaudeSessions,
  mockReadCodexSessions,
  mockReadOpenCodeSessions,
  mockReadGeminiSessions,
  mockIsSSHTransportError,
} = vi.hoisted(() => ({
  mockReadClaudeSessions: vi.fn(),
  mockReadCodexSessions: vi.fn(),
  mockReadOpenCodeSessions: vi.fn(),
  mockReadGeminiSessions: vi.fn(),
  mockIsSSHTransportError: vi.fn(),
}));

vi.mock("@/lib/aiSessions/readClaudeSessions", () => ({
  readClaudeSessions: mockReadClaudeSessions,
  readClaudeSessionDetail: vi.fn(),
}));

vi.mock("@/lib/aiSessions/readCodexSessions", () => ({
  readCodexSessions: mockReadCodexSessions,
  readCodexSessionDetail: vi.fn(),
}));

vi.mock("@/lib/aiSessions/readOpenCodeSessions", () => ({
  readOpenCodeSessions: mockReadOpenCodeSessions,
  readOpenCodeSessionDetail: vi.fn(),
}));

vi.mock("@/lib/aiSessions/readGeminiSessions", () => ({
  readGeminiSessions: mockReadGeminiSessions,
  readGeminiSessionDetail: vi.fn(),
}));

vi.mock("@/lib/gitOperations", () => ({
  isSSHTransportError: mockIsSSHTransportError,
}));

import { aggregateAiSessions } from "@/lib/aiSessions/aggregateAiSessions";

describe("aggregateAiSessions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockIsSSHTransportError.mockReturnValue(false);
  });

  it("should merge summary sessions from all readers and sort them by updated time", async () => {
    mockReadClaudeSessions.mockResolvedValue({
      provider: "claude",
      available: true,
      sessionCount: 1,
      reason: null,
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          startedAt: "2026-03-10T10:00:00.000Z",
          updatedAt: "2026-03-10T10:10:00.000Z",
          matchedPath: "/repo",
          matchScope: "repo",
          title: "Claude",
          firstUserPrompt: "alpha",
          messageCount: 1,
        },
      ],
    });
    mockReadCodexSessions.mockResolvedValue({
      provider: "codex",
      available: true,
      sessionCount: 1,
      reason: null,
      sessions: [
        {
          id: "codex-1",
          provider: "codex",
          startedAt: "2026-03-11T10:00:00.000Z",
          updatedAt: "2026-03-11T10:10:00.000Z",
          matchedPath: "/repo",
          matchScope: "worktree",
          title: "Codex",
          firstUserPrompt: "beta",
          messageCount: 2,
        },
      ],
    });
    mockReadOpenCodeSessions.mockResolvedValue({
      provider: "opencode",
      available: true,
      sessionCount: 0,
      reason: null,
      sessions: [],
    });
    mockReadGeminiSessions.mockResolvedValue({
      provider: "gemini",
      available: true,
      sessionCount: 0,
      reason: "No data",
      sessions: [],
    });

    const result = await aggregateAiSessions({
      worktreePath: "/repo/worktree",
      repoPath: "/repo",
    });

    expect(result.sessions).toHaveLength(2);
    expect(result.sessions[0]?.id).toBe("codex-1");
    expect(result.sessions[1]?.id).toBe("claude-1");
    expect(result.sources).toEqual([
      { provider: "claude", available: true, sessionCount: 1, reason: null },
      { provider: "codex", available: true, sessionCount: 1, reason: null },
      { provider: "opencode", available: true, sessionCount: 0, reason: null },
      { provider: "gemini", available: true, sessionCount: 0, reason: "No data" },
    ]);
  });

  it("should keep other session sources when a remote provider fails with SSH transport error", async () => {
    mockReadClaudeSessions.mockResolvedValue({
      provider: "claude",
      available: true,
      sessionCount: 1,
      reason: null,
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          startedAt: "2026-03-10T10:00:00.000Z",
          updatedAt: "2026-03-10T10:10:00.000Z",
          matchedPath: "/repo",
          matchScope: "repo",
          title: "Claude",
          firstUserPrompt: "alpha",
          messageCount: 1,
        },
      ],
    });
    mockReadCodexSessions.mockRejectedValue(
      new Error("remote-host 원격 명령 실패: Connection reset by 100.73.171.123 port 22"),
    );
    mockReadOpenCodeSessions.mockResolvedValue({
      provider: "opencode",
      available: false,
      sessionCount: 0,
      reason: "Remote OpenCode session reading is not available yet",
      sessions: [],
    });
    mockReadGeminiSessions.mockResolvedValue({
      provider: "gemini",
      available: false,
      sessionCount: 0,
      reason: "Gemini CLI directory not found",
      sessions: [],
    });
    mockIsSSHTransportError.mockImplementation((error: unknown) =>
      String(error instanceof Error ? error.message : error).includes("Connection reset"),
    );

    const result = await aggregateAiSessions({
      worktreePath: "/repo/worktree",
      repoPath: "/repo",
      sshHost: "remote-host",
    });

    expect(result.sessions).toHaveLength(1);
    expect(result.sessions[0]?.id).toBe("claude-1");
    expect(result.sources).toEqual([
      { provider: "claude", available: true, sessionCount: 1, reason: null },
      {
        provider: "codex",
        available: false,
        sessionCount: 0,
        reason: "SSH connection to remote-host is unavailable",
      },
      {
        provider: "opencode",
        available: false,
        sessionCount: 0,
        reason: "Remote OpenCode session reading is not available yet",
      },
      {
        provider: "gemini",
        available: false,
        sessionCount: 0,
        reason: "Gemini CLI directory not found",
      },
    ]);
  });
});
