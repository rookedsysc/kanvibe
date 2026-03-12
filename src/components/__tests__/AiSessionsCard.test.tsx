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
});
