import { describe, expect, it, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TaskCardLiveSessions } from "@/components/TaskCardLiveSessions";
import type { RunningAgentPane } from "@/lib/aiSessions/types";

const mocks = vi.hoisted(() => ({
  getTaskLiveAiSessions: vi.fn(),
  getRunningAgentPanes: vi.fn(),
  selectTerminalTab: vi.fn(),
  navigateToTaskDetail: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useLocale: () => "ko",
  useTranslations: () => (key: string, values?: Record<string, unknown>) => (
    values ? `${key}:${JSON.stringify(values)}` : key
  ),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  getTaskLiveAiSessions: (...args: unknown[]) => mocks.getTaskLiveAiSessions(...args),
  getRunningAgentPanes: (...args: unknown[]) => mocks.getRunningAgentPanes(...args),
}));

vi.mock("@/desktop/renderer/actions/terminalTabs", () => ({
  selectTerminalTab: (...args: unknown[]) => mocks.selectTerminalTab(...args),
}));

vi.mock("@/desktop/renderer/utils/taskNavigation", () => ({
  navigateToTaskDetail: (...args: unknown[]) => mocks.navigateToTaskDetail(...args),
}));

const runningPane: RunningAgentPane = {
  provider: "claude",
  worktreePath: "/repo/task",
  sessionName: "kanvibe-task",
  windowId: "@7",
  windowName: "claude",
};

describe("TaskCardLiveSessions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getTaskLiveAiSessions.mockResolvedValue({
      taskId: "task-1",
      isRemote: false,
      sessions: [{
        provider: "claude",
        sessionId: "session-a",
        state: "running",
        lastActiveAt: "2026-08-10T00:00:00.000Z",
        runningSubtasks: [],
        terminalWindow: { sessionName: "kanvibe-task", windowId: "@7", windowName: "claude" },
      }],
    });
  });

  it("패널이 닫혀 있으면 세션을 조회하지 않는다", () => {
    render(
      <TaskCardLiveSessions
        taskId="task-1"
        worktreePath="/repo/task"
        runningPanes={[runningPane]}
        isPanelOpen={false}
      />,
    );

    expect(mocks.getTaskLiveAiSessions).not.toHaveBeenCalled();
  });

  it("세션을 클릭하면 그 세션의 tmux window로 옮긴 뒤 태스크 상세로 이동한다", async () => {
    render(
      <TaskCardLiveSessions
        taskId="task-1"
        worktreePath="/repo/task"
        runningPanes={[runningPane]}
        isPanelOpen
      />,
    );

    fireEvent.click(await screen.findByRole("button"));

    await waitFor(() => {
      expect(mocks.selectTerminalTab).toHaveBeenCalledWith("task-1", "@7");
      expect(mocks.navigateToTaskDetail).toHaveBeenCalledWith("task-1", { currentLocale: "ko" });
    });
  });

  it("실행중 에이전트도 열린 패널도 없으면 아무것도 그리지 않는다", () => {
    const { container } = render(
      <TaskCardLiveSessions
        taskId="task-1"
        worktreePath="/repo/other"
        runningPanes={[runningPane]}
        isPanelOpen={false}
      />,
    );

    expect(container.firstChild).toBeNull();
  });
});
