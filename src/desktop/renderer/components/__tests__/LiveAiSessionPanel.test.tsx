import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { LiveAiSessionPanel } from "@/desktop/renderer/components/LiveAiSessionPanel";
import type { LiveAiSession } from "@/lib/aiSessions/types";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

function createSession(overrides: Partial<LiveAiSession> = {}): LiveAiSession {
  return {
    provider: "claude",
    sessionId: "session-a",
    state: "running",
    lastActiveAt: "2026-08-10T00:00:00.000Z",
    runningSubtasks: [],
    terminalWindow: null,
    ...overrides,
  };
}

describe("LiveAiSessionPanel", () => {
  it("실행중 세션과 유휴 세션을 다른 상태 점으로 구분한다", () => {
    render(<LiveAiSessionPanel sessions={[
      createSession(),
      createSession({ provider: "codex", state: "idle" }),
    ]} />);

    expect(screen.getByTestId("live-ai-session-state-running")).toBeTruthy();
    expect(screen.getByTestId("live-ai-session-state-idle")).toBeTruthy();
  });

  it("실행중 서브태스크의 개수와 이름을 보여준다", () => {
    render(<LiveAiSessionPanel sessions={[createSession({
      runningSubtasks: [
        { id: "agent-1", name: "Explore", lastActiveAt: null },
        { id: "agent-2", name: null, lastActiveAt: null },
      ],
    })]} />);

    expect(screen.getByText('subtaskCount:{"count":2}')).toBeTruthy();
    expect(screen.getAllByTestId("live-ai-subtask").map((node) => node.textContent))
      .toEqual(["Explore", "agent-2"]);
  });

  it("터미널 창을 찾은 세션만 클릭할 수 있다", () => {
    const onSelectSession = vi.fn();
    render(<LiveAiSessionPanel
      sessions={[
        createSession({
          terminalWindow: { sessionName: "kanvibe-task", windowId: "@7", windowName: "claude" },
        }),
        createSession({ provider: "codex" }),
      ]}
      onSelectSession={onSelectSession}
    />);

    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(1);

    fireEvent.click(buttons[0]);
    expect(onSelectSession).toHaveBeenCalledWith(expect.objectContaining({ provider: "claude" }));
  });

  it("실행중인 세션이 없으면 빈 안내를 보여준다", () => {
    render(<LiveAiSessionPanel sessions={[]} />);

    expect(screen.getByText("empty")).toBeTruthy();
  });
});
