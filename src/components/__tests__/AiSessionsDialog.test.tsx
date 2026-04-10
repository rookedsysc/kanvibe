import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { IntlProvider } from "next-intl";
import AiSessionsDialog from "@/components/AiSessionsDialog";
import type { AggregatedAiSessionDetail, AggregatedAiSessionsResult } from "@/lib/aiSessions/types";

const { mockGetTaskAiSessionDetail, mockGetTaskAiSessions } = vi.hoisted(() => ({
  mockGetTaskAiSessionDetail: vi.fn(),
  mockGetTaskAiSessions: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  getTaskAiSessionDetail: mockGetTaskAiSessionDetail,
  getTaskAiSessions: mockGetTaskAiSessions,
}));

const messages = {
  taskDetail: {
    hooksStatusDialog: {
      close: "Close",
    },
    aiSessions: {
      title: "AI Sessions",
      description: "Browse local AI conversation history related to this task.",
      includeRepoToggle: "Include main branch sessions",
      includeRepoToggleShort: "+ main",
      includeRepoHint: "Shows only the current worktree sessions by default; include main branch sessions when needed.",
      providerFilterEmpty: "Select AI filters",
      providerSearchPlaceholder: "Search AI",
      noProviderMatch: "No matching AI providers.",
      empty: "No local AI sessions matched this task.",
      remoteUnsupported: "Remote projects do not support local AI session aggregation.",
      untitled: "Untitled",
      noPrompt: "No first user prompt was found.",
      selectSession: "Select a session from the left to load messages.",
      openDetailHint: "Select a session to view details.",
      noPreview: "No preview is available.",
      loadingDetail: "Loading messages...",
      loadingSource: "Searching source",
      loadingWorkspace: "Inspecting workspace path",
      loadingSessions: "Loading session list...",
      loadMore: "Load more",
      loadingMore: "Loading more...",
      detailError: "Failed to load session details.",
      sessionsError: "Failed to load session list.",
      searchPlaceholder: "Search sessions...",
      messageSearchPlaceholder: "Search messages...",
      filterUser: "User",
      filterAssistant: "AI",
      showMore: "Show more",
      messages: "{count} messages",
      providers: {
        all: "All",
        claude: "Claude",
        codex: "Codex",
        opencode: "OpenCode",
        gemini: "Gemini",
      },
      showLess: "Show less",
      roles: {
        user: "User",
        assistant: "Assistant",
        tool: "Tool",
        system: "System",
        unknown: "Other",
      },
    },
  },
};

describe("AiSessionsDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetTaskAiSessions.mockResolvedValue(null);
  });

  it("should not render when dialog is closed", () => {
    const data = { isRemote: false, targetPath: null, repoPath: null, sessions: [], sources: [] };

    const { container } = render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsDialog taskId="task-1" isOpen={false} onClose={() => {}} data={data} />
      </IntlProvider>
    );

    expect(container.firstChild).toBeNull();
  });

  it("should not render detected sources section anymore", async () => {
    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo",
      repoPath: "/repo",
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          title: "Claude session",
          firstUserPrompt: "Alpha",
          updatedAt: "2026-03-11T10:00:00.000Z",
          startedAt: "2026-03-11T09:00:00.000Z",
          matchedPath: "/repo",
          matchScope: "worktree",
          messageCount: 1,
        },
      ],
      sources: [{ provider: "claude", available: true, sessionCount: 1, reason: null }],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsDialog taskId="task-1" isOpen onClose={() => {}} data={data} />
      </IntlProvider>
    );

    expect(screen.queryByText("Detected sources")).toBeNull();
    expect(screen.getByText("Select a session from the left to load messages.")).toBeTruthy();
  });

  it("should filter sessions by multi-select provider dropdown", async () => {
    mockGetTaskAiSessionDetail.mockResolvedValue({
      sessionId: "codex-1",
      provider: "codex",
      title: "Codex session",
      matchedPath: "/repo",
      messages: [],
      nextCursor: null,
    } satisfies AggregatedAiSessionDetail);

    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo",
      repoPath: "/repo",
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          title: "Claude session",
          firstUserPrompt: "Alpha",
          updatedAt: "2026-03-11T10:00:00.000Z",
          startedAt: "2026-03-11T09:00:00.000Z",
          matchedPath: "/repo",
          matchScope: "worktree",
          messageCount: 1,
        },
        {
          id: "codex-1",
          provider: "codex",
          title: "Codex session",
          firstUserPrompt: "Beta",
          updatedAt: "2026-03-11T11:00:00.000Z",
          startedAt: "2026-03-11T10:30:00.000Z",
          matchedPath: "/repo",
          matchScope: "worktree",
          messageCount: 1,
        },
      ],
      sources: [
        { provider: "claude", available: true, sessionCount: 1, reason: null },
        { provider: "codex", available: true, sessionCount: 1, reason: null },
      ],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsDialog taskId="task-1" isOpen onClose={() => {}} data={data} />
      </IntlProvider>
    );

    fireEvent.click(screen.getAllByRole("button", { name: /claude/i })[0]!);
    const claudeOption = screen
      .getAllByText("Claude")
      .find((element) => element.closest("li"));
    fireEvent.mouseDown(claudeOption!);

    await waitFor(() => {
      expect(screen.queryByText("Claude session")).toBeNull();
    });
    expect(screen.getAllByText("Codex session").length).toBeGreaterThan(0);
    expect(mockGetTaskAiSessionDetail).not.toHaveBeenCalled();
  });

  it("should fetch repo sessions only when toggle is enabled", async () => {
    mockGetTaskAiSessionDetail.mockResolvedValue({
      sessionId: "claude-1",
      provider: "claude",
      title: "Claude session",
      matchedPath: "/repo/worktree",
      messages: [],
      nextCursor: null,
    } satisfies AggregatedAiSessionDetail);
    mockGetTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo/worktree",
      repoPath: "/repo",
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          title: "Claude session",
          firstUserPrompt: "Alpha",
          updatedAt: "2026-03-11T10:00:00.000Z",
          startedAt: "2026-03-11T09:00:00.000Z",
          matchedPath: "/repo/worktree",
          matchScope: "worktree",
          messageCount: 1,
        },
        {
          id: "claude-2",
          provider: "claude",
          title: "Repo session",
          firstUserPrompt: "Repo",
          updatedAt: "2026-03-11T11:00:00.000Z",
          startedAt: "2026-03-11T10:00:00.000Z",
          matchedPath: "/repo",
          matchScope: "repo",
          messageCount: 1,
        },
      ],
      sources: [{ provider: "claude", available: true, sessionCount: 2, reason: null }],
    } satisfies AggregatedAiSessionsResult);

    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo/worktree",
      repoPath: "/repo",
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          title: "Claude session",
          firstUserPrompt: "Alpha",
          updatedAt: "2026-03-11T10:00:00.000Z",
          startedAt: "2026-03-11T09:00:00.000Z",
          matchedPath: "/repo/worktree",
          matchScope: "worktree",
          messageCount: 1,
        },
      ],
      sources: [{ provider: "claude", available: true, sessionCount: 1, reason: null }],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsDialog taskId="task-1" isOpen onClose={() => {}} data={data} />
      </IntlProvider>
    );

    expect(mockGetTaskAiSessions).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("switch", { name: /\+ main/i }));

    await waitFor(() => {
      expect(mockGetTaskAiSessions).toHaveBeenCalledWith("task-1", true);
    });
    expect(await screen.findByText("Repo session")).toBeTruthy();
  });

  it("should keep long session text constrained inside the card", () => {
    const longTitle = "# AGENTS.md instructions for /home/rookedsysc/Documents/techtaurant/techtaurant-be__worktrees/feat-user-ban";
    const longSubtitle =
      "# AGENTS.md instructions for /home/rookedsysc/Documents/techtaurant/techtaurant-be__worktrees/feat-user-ban <INSTRUCTIONS> ## Default Context @.claude/core/FLAGS.md @.claude/core/CODE_PRINCIPLES.md";

    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo/worktree",
      repoPath: "/repo",
      sessions: [
        {
          id: "codex-1",
          provider: "codex",
          title: longTitle,
          firstUserPrompt: longSubtitle,
          updatedAt: "2026-03-13T10:48:12.000Z",
          startedAt: "2026-03-13T10:40:00.000Z",
          matchedPath: "/repo/worktree",
          matchScope: "worktree",
          messageCount: 1,
        },
      ],
      sources: [{ provider: "codex", available: true, sessionCount: 1, reason: null }],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsDialog taskId="task-1" isOpen onClose={() => {}} data={data} />
      </IntlProvider>
    );

    const card = screen.getByRole("button", { name: new RegExp(longTitle.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")) });
    expect(card.className).toContain("overflow-hidden");

    const title = screen.getByText(longTitle);
    expect(title.className).toContain("line-clamp-2");
    expect(title.className).toContain("break-words");

    const subtitle = screen.getByText(longSubtitle);
    expect(subtitle.className).toContain("line-clamp-3");
    expect(subtitle.className).toContain("break-all");
  });

  it("should expand truncated messages when show more is clicked", async () => {
    mockGetTaskAiSessionDetail.mockResolvedValue({
      sessionId: "claude-1",
      provider: "claude",
      title: "Claude session",
      matchedPath: "/repo",
      messages: [
        {
          role: "assistant",
          timestamp: "2026-03-11T09:00:00.000Z",
          text: "This is a truncated preview...",
          fullText: "This is a truncated preview that should expand to the full message body.",
          isTruncated: true,
        },
      ],
      nextCursor: null,
    } satisfies AggregatedAiSessionDetail);

    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo",
      repoPath: "/repo",
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          title: "Claude session",
          firstUserPrompt: "Alpha",
          updatedAt: "2026-03-11T10:00:00.000Z",
          startedAt: "2026-03-11T09:00:00.000Z",
          matchedPath: "/repo",
          matchScope: "worktree",
          messageCount: 1,
        },
      ],
      sources: [{ provider: "claude", available: true, sessionCount: 1, reason: null }],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsDialog taskId="task-1" isOpen onClose={() => {}} data={data} />
      </IntlProvider>
    );

    fireEvent.click(screen.getByText("Claude session"));
    await screen.findByRole("button", { name: "Show more" });
    fireEvent.click(screen.getByRole("button", { name: "Show more" }));

    expect(screen.getByText("This is a truncated preview that should expand to the full message body.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Show less" })).toBeTruthy();
  });

  it("should show animated loading details while fetching messages", async () => {
    let resolveDetail: ((value: AggregatedAiSessionDetail) => void) | undefined;
    mockGetTaskAiSessionDetail.mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveDetail = resolve;
        })
    );

    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo",
      repoPath: "/repo",
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          title: "Claude session",
          firstUserPrompt: null,
          updatedAt: "2026-03-11T10:00:00.000Z",
          startedAt: "2026-03-11T09:00:00.000Z",
          matchedPath: "/repo/worktree",
          matchScope: "worktree",
          messageCount: 2,
          sourceRef: "/tmp/claude-session.jsonl",
        },
      ],
      sources: [{ provider: "claude", available: true, sessionCount: 1, reason: null }],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsDialog taskId="task-1" isOpen onClose={() => {}} data={data} />
      </IntlProvider>
    );

    fireEvent.click(screen.getByText("Claude session"));

    expect(await screen.findByText("Loading messages...")).toBeTruthy();
    expect(screen.getByText("Searching source")).toBeTruthy();
    expect(screen.getByText("/tmp/claude-session.jsonl")).toBeTruthy();

    if (resolveDetail) {
      resolveDetail({
        sessionId: "claude-1",
        provider: "claude",
        title: "Claude session",
        matchedPath: "/repo/worktree",
        sourceRef: "/tmp/claude-session.jsonl",
        messages: [],
        nextCursor: null,
      });
    }

    await waitFor(() => {
      expect(screen.queryByText("Loading messages...")).toBeNull();
    });
  });

  it("should load more messages when next cursor exists", async () => {
    mockGetTaskAiSessionDetail
      .mockResolvedValueOnce({
        sessionId: "claude-1",
        provider: "claude",
        title: "Claude session",
        matchedPath: "/repo",
        messages: [
          { role: "assistant", timestamp: "2026-03-11T09:01:00.000Z", text: "Newest page", fullText: "Newest page", isTruncated: false },
        ],
        nextCursor: "1",
      } satisfies AggregatedAiSessionDetail)
      .mockResolvedValueOnce({
        sessionId: "claude-1",
        provider: "claude",
        title: "Claude session",
        matchedPath: "/repo",
        messages: [
          { role: "assistant", timestamp: "2026-03-11T09:00:00.000Z", text: "Older page", fullText: "Older page", isTruncated: false },
        ],
        nextCursor: null,
      } satisfies AggregatedAiSessionDetail);

    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo",
      repoPath: "/repo",
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          title: "Claude session",
          firstUserPrompt: "Alpha",
          updatedAt: "2026-03-11T10:00:00.000Z",
          startedAt: "2026-03-11T09:00:00.000Z",
          matchedPath: "/repo",
          matchScope: "worktree",
          messageCount: 2,
        },
      ],
      sources: [{ provider: "claude", available: true, sessionCount: 1, reason: null }],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsDialog taskId="task-1" isOpen onClose={() => {}} data={data} />
      </IntlProvider>
    );

    fireEvent.click(screen.getByText("Claude session"));
    await screen.findByText("Newest page");
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));

    await screen.findByText("Older page");
    const previewMessages = screen.getAllByText(/page$/i);
    expect(previewMessages[0]?.textContent).toBe("Newest page");
    expect(previewMessages[1]?.textContent).toBe("Older page");
    expect(mockGetTaskAiSessionDetail).toHaveBeenNthCalledWith(2, "task-1", "claude", "claude-1", null, "1", 20, false);
  });

  it("should show detail error when session detail resolves null", async () => {
    mockGetTaskAiSessionDetail.mockResolvedValue(null);

    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo",
      repoPath: "/repo",
      sessions: [
        {
          id: "claude-1",
          provider: "claude",
          title: "Claude session",
          firstUserPrompt: "Alpha",
          updatedAt: "2026-03-11T10:00:00.000Z",
          startedAt: "2026-03-11T09:00:00.000Z",
          matchedPath: "/repo",
          matchScope: "worktree",
          messageCount: 1,
        },
      ],
      sources: [{ provider: "claude", available: true, sessionCount: 1, reason: null }],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsDialog taskId="task-1" isOpen onClose={() => {}} data={data} />
      </IntlProvider>
    );

    fireEvent.click(screen.getByText("Claude session"));

    await waitFor(() => {
      expect(screen.getByText("Failed to load session details.")).toBeTruthy();
    });
  });
});
