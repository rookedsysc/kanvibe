import { render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TaskDetailRoute from "@/desktop/renderer/routes/TaskDetailRoute";

const TASK_DETAIL_CACHE_KEY = "kanvibe:route-cache:task-detail:task-1";

const mocks = vi.hoisted(() => ({
  getTaskById: vi.fn(),
  getTaskIdByProjectAndBranch: vi.fn(),
  updateTaskStatus: vi.fn(),
  deleteTask: vi.fn(),
  getGitDiffFiles: vi.fn(),
  getTaskHooksStatus: vi.fn(),
  getTaskGeminiHooksStatus: vi.fn(),
  getTaskCodexHooksStatus: vi.fn(),
  getTaskOpenCodeHooksStatus: vi.fn(),
  getTaskAiSessions: vi.fn(),
  getSidebarDefaultCollapsed: vi.fn(),
  getSidebarHintDismissed: vi.fn(),
  getDoneAlertDismissed: vi.fn(),
  fetchPrUrlWithPrompt: vi.fn(),
  useRefreshSignal: vi.fn(() => 0),
  push: vi.fn(),
  back: vi.fn(),
}));

function createDeferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

vi.mock("react-router-dom", () => ({
  useParams: () => ({ id: "task-1" }),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  useRouter: () => ({ push: mocks.push, back: mocks.back }),
}));

vi.mock("@/desktop/renderer/utils/refresh", () => ({
  useRefreshSignal: () => mocks.useRefreshSignal(),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  getTaskById: (...args: unknown[]) => mocks.getTaskById(...args),
  getTaskIdByProjectAndBranch: (...args: unknown[]) => mocks.getTaskIdByProjectAndBranch(...args),
  updateTaskStatus: (...args: unknown[]) => mocks.updateTaskStatus(...args),
  deleteTask: (...args: unknown[]) => mocks.deleteTask(...args),
}));

vi.mock("@/desktop/renderer/actions/diff", () => ({
  getGitDiffFiles: (...args: unknown[]) => mocks.getGitDiffFiles(...args),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  getTaskHooksStatus: (...args: unknown[]) => mocks.getTaskHooksStatus(...args),
  getTaskGeminiHooksStatus: (...args: unknown[]) => mocks.getTaskGeminiHooksStatus(...args),
  getTaskCodexHooksStatus: (...args: unknown[]) => mocks.getTaskCodexHooksStatus(...args),
  getTaskOpenCodeHooksStatus: (...args: unknown[]) => mocks.getTaskOpenCodeHooksStatus(...args),
  getTaskAiSessions: (...args: unknown[]) => mocks.getTaskAiSessions(...args),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getSidebarDefaultCollapsed: (...args: unknown[]) => mocks.getSidebarDefaultCollapsed(...args),
  getSidebarHintDismissed: (...args: unknown[]) => mocks.getSidebarHintDismissed(...args),
  getDoneAlertDismissed: (...args: unknown[]) => mocks.getDoneAlertDismissed(...args),
}));

vi.mock("@/desktop/renderer/utils/fetchPrUrlWithPrompt", () => ({
  fetchPrUrlWithPrompt: (...args: unknown[]) => mocks.fetchPrUrlWithPrompt(...args),
}));

vi.mock("@/components/AiSessionsCard", () => ({
  default: () => <div data-testid="ai-sessions-card" />,
}));

vi.mock("@/components/CollapsibleSidebar", () => ({
  default: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));

vi.mock("@/components/ConnectTerminalForm", () => ({
  default: () => <div data-testid="connect-terminal-form" />,
}));

vi.mock("@/components/DeleteTaskButton", () => ({
  default: () => <button type="button">delete</button>,
}));

vi.mock("@/components/DoneStatusButton", () => ({
  default: () => <button type="button">done</button>,
}));

vi.mock("@/components/HooksStatusCard", () => ({
  default: () => <div data-testid="hooks-status-card" />,
}));

vi.mock("@/components/NotificationCenterButton", () => ({
  default: () => <div data-testid="notification-center" />,
}));

vi.mock("@/components/TaskDetailInfoCard", () => ({
  default: ({ task }: { task: { branchName: string | null } }) => (
    <div data-testid="task-info">{task.branchName ?? "no-branch"}</div>
  ),
}));

vi.mock("@/components/TaskDetailTitleCard", () => ({
  default: ({ task }: { task: { title: string } }) => (
    <div data-testid="task-title">{task.title}</div>
  ),
}));

vi.mock("@/desktop/renderer/components/TerminalLoader", () => ({
  default: () => <div data-testid="terminal-loader" />,
}));

describe("TaskDetailRoute", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    sessionStorage.clear();
    mocks.getTaskIdByProjectAndBranch.mockResolvedValue(null);
    mocks.getGitDiffFiles.mockResolvedValue([]);
    mocks.getTaskHooksStatus.mockResolvedValue(null);
    mocks.getTaskGeminiHooksStatus.mockResolvedValue(null);
    mocks.getTaskCodexHooksStatus.mockResolvedValue(null);
    mocks.getTaskOpenCodeHooksStatus.mockResolvedValue(null);
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: null,
      repoPath: null,
      sessions: [],
      sources: [],
    });
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(false);
    mocks.getSidebarHintDismissed.mockResolvedValue(false);
    mocks.getDoneAlertDismissed.mockResolvedValue(false);
    mocks.updateTaskStatus.mockResolvedValue(null);
    mocks.deleteTask.mockResolvedValue(true);
    mocks.fetchPrUrlWithPrompt.mockResolvedValue(null);
  });

  it("캐시가 있으면 stale task detail을 즉시 렌더링하고 이후 최신 데이터로 갱신한다", async () => {
    // Given
    sessionStorage.setItem(TASK_DETAIL_CACHE_KEY, JSON.stringify({
      task: {
        id: "task-1",
        title: "cached task title",
        description: null,
        branchName: "feat/cached",
        baseBranch: "main",
        prUrl: "https://example.com/cached",
        sessionType: null,
        sessionName: null,
        sshHost: null,
        projectId: "project-1",
        project: { id: "project-1", name: "kanvibe" },
        status: "todo",
        agentType: null,
        worktreePath: "/repo__worktrees/cached",
      },
      baseBranchTaskId: null,
      diffFiles: [],
      claudeHooksStatus: null,
      geminiHooksStatus: null,
      codexHooksStatus: null,
      openCodeHooksStatus: null,
      aiSessions: {
        isRemote: false,
        targetPath: null,
        repoPath: null,
        sessions: [],
        sources: [],
      },
      sidebarDefaultCollapsed: false,
      sidebarHintDismissed: false,
      doneAlertDismissed: false,
    }));
    const deferredTask = createDeferred<{
      id: string;
      title: string;
      description: null;
      branchName: string;
      baseBranch: string;
      prUrl: string;
      sessionType: null;
      sessionName: null;
      sshHost: null;
      projectId: string;
      project: { id: string; name: string };
      status: string;
      agentType: null;
      worktreePath: string;
    } | null>();
    mocks.getTaskById.mockReturnValue(deferredTask.promise);

    // When
    render(<TaskDetailRoute />);

    // Then
    expect(screen.queryByText("Loading...")).toBeNull();
    expect(screen.getByTestId("task-title").textContent).toBe("cached task title");

    deferredTask.resolve({
      id: "task-1",
      title: "fresh task title",
      description: null,
      branchName: "feat/fresh",
      baseBranch: "main",
      prUrl: "https://example.com/fresh",
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/fresh",
    });

    await waitFor(() => {
      expect(screen.getByTestId("task-title").textContent).toBe("fresh task title");
    });
  });
});
