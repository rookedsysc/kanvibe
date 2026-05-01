import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { IntlProvider } from "next-intl";
import AiSessionsCard from "@/components/AiSessionsCard";
import type { AggregatedAiSessionsResult } from "@/lib/aiSessions/types";

vi.mock("@/components/AiSessionsDialog", () => ({
  default: ({ isOpen, onClose, data }: { isOpen: boolean; onClose: () => void; data: AggregatedAiSessionsResult }) =>
    isOpen ? (
      <div data-testid="ai-sessions-dialog">
        sessions={data.sessions.length}
        <button onClick={onClose}>close</button>
      </div>
    ) : null,
}));

const messages = {
  taskDetail: {
    aiSessions: {
      title: "AI Sessions",
      openDialog: "Open session list",
      summary: "{providers} tools / {sessions} sessions",
      emptyBadge: "No local sessions",
      remoteBadge: "Remote unsupported",
    },
  },
};

describe("AiSessionsCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should render summary for local sessions", () => {
    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo",
      repoPath: "/repo",
      sessions: [
        {
          id: "1",
          provider: "claude",
          startedAt: null,
          updatedAt: null,
          matchedPath: "/repo",
          matchScope: "repo",
          title: "One",
          firstUserPrompt: "Alpha",
          messageCount: 1,
        },
        {
          id: "2",
          provider: "claude",
          startedAt: null,
          updatedAt: null,
          matchedPath: "/repo",
          matchScope: "repo",
          title: "Two",
          firstUserPrompt: "Beta",
          messageCount: 1,
        },
      ],
      sources: [
        { provider: "claude", available: true, sessionCount: 2, reason: null },
        { provider: "codex", available: true, sessionCount: 0, reason: null },
      ],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsCard taskId="task-1" data={data} />
      </IntlProvider>
    );

    expect(screen.getByText("AI Sessions")).toBeTruthy();
    expect(screen.getByText("1 tools / 2 sessions")).toBeTruthy();
  });

  it("should open dialog when card button is clicked", () => {
    const data: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo",
      repoPath: "/repo",
      sessions: [
        {
          id: "1",
          provider: "claude",
          startedAt: null,
          updatedAt: null,
          matchedPath: "/repo",
          matchScope: "repo",
          title: "One",
          firstUserPrompt: "Alpha",
          messageCount: 1,
        },
      ],
      sources: [{ provider: "claude", available: true, sessionCount: 1, reason: null }],
    };

    render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsCard taskId="task-1" data={data} />
      </IntlProvider>
    );
    fireEvent.click(screen.getByText("1 tools / 1 sessions"));

    expect(screen.getByTestId("ai-sessions-dialog")).toBeTruthy();
  });

  it("should close the dialog when the task changes", () => {
    const firstTaskData: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo/task-1",
      repoPath: "/repo",
      sessions: [
        {
          id: "1",
          provider: "claude",
          startedAt: null,
          updatedAt: null,
          matchedPath: "/repo/task-1",
          matchScope: "worktree",
          title: "Task 1 session",
          firstUserPrompt: "Alpha",
          messageCount: 1,
        },
      ],
      sources: [{ provider: "claude", available: true, sessionCount: 1, reason: null }],
    };

    const secondTaskData: AggregatedAiSessionsResult = {
      isRemote: false,
      targetPath: "/repo/task-2",
      repoPath: "/repo",
      sessions: [
        {
          id: "2",
          provider: "codex",
          startedAt: null,
          updatedAt: null,
          matchedPath: "/repo/task-2",
          matchScope: "worktree",
          title: "Task 2 session",
          firstUserPrompt: "Beta",
          messageCount: 1,
        },
      ],
      sources: [{ provider: "codex", available: true, sessionCount: 1, reason: null }],
    };

    const { rerender } = render(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsCard taskId="task-1" data={firstTaskData} />
      </IntlProvider>
    );

    fireEvent.click(screen.getByText("1 tools / 1 sessions"));
    expect(screen.getByTestId("ai-sessions-dialog")).toBeTruthy();

    rerender(
      <IntlProvider locale="en" messages={messages}>
        <AiSessionsCard taskId="task-2" data={secondTaskData} />
      </IntlProvider>
    );

    expect(screen.queryByTestId("ai-sessions-dialog")).toBeNull();
  });
});
