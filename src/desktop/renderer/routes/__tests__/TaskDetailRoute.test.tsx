import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BoardCommandProvider } from "@/desktop/renderer/components/BoardCommandProvider";
import TaskDetailRoute from "@/desktop/renderer/routes/TaskDetailRoute";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";

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
  getAllProjects: vi.fn(),
  getSidebarDefaultCollapsed: vi.fn(),
  getSidebarHintDismissed: vi.fn(),
  getDoneAlertDismissed: vi.fn(),
  getDefaultSessionType: vi.fn(),
  listNotifications: vi.fn(),
  markNotificationRead: vi.fn(),
  markAllNotificationsRead: vi.fn(),
  activateNotification: vi.fn(),
  fetchPrUrlWithPrompt: vi.fn(),
  useRefreshSignal: vi.fn(() => 0),
  redirect: vi.fn(),
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
  redirect: (...args: unknown[]) => mocks.redirect(...args),
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
  getAllProjects: (...args: unknown[]) => mocks.getAllProjects(...args),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getSidebarDefaultCollapsed: (...args: unknown[]) => mocks.getSidebarDefaultCollapsed(...args),
  getSidebarHintDismissed: (...args: unknown[]) => mocks.getSidebarHintDismissed(...args),
  getDoneAlertDismissed: (...args: unknown[]) => mocks.getDoneAlertDismissed(...args),
  getDefaultSessionType: (...args: unknown[]) => mocks.getDefaultSessionType(...args),
}));

vi.mock("@/desktop/renderer/actions/notifications", () => ({
  listNotifications: (...args: unknown[]) => mocks.listNotifications(...args),
  markNotificationRead: (...args: unknown[]) => mocks.markNotificationRead(...args),
  markAllNotificationsRead: (...args: unknown[]) => mocks.markAllNotificationsRead(...args),
  activateNotification: (...args: unknown[]) => mocks.activateNotification(...args),
}));

vi.mock("@/desktop/renderer/utils/fetchPrUrlWithPrompt", () => ({
  fetchPrUrlWithPrompt: (...args: unknown[]) => mocks.fetchPrUrlWithPrompt(...args),
}));

vi.mock("@/components/AiSessionsCard", () => ({
  default: () => <div data-testid="ai-sessions-card" />,
}));

vi.mock("@/components/CollapsibleSidebar", () => ({
  default: ({
    children,
    showHint,
    onDismissHint,
  }: {
    children: ReactNode;
    showHint: boolean;
    onDismissHint?: () => void;
  }) => (
    <div data-testid="collapsible-sidebar" data-show-hint={String(showHint)}>
      <button type="button" onClick={onDismissHint}>dismiss sidebar hint</button>
      {showHint ? <div>sidebar hint visible</div> : null}
      {children}
    </div>
  ),
}));

vi.mock("@/components/ConnectTerminalForm", () => ({
  default: () => <div data-testid="connect-terminal-form" />,
}));

vi.mock("@/components/CreateTaskModal", () => ({
  default: ({
    isOpen,
    onClose,
    defaultProjectId,
    defaultBaseBranch,
    defaultSessionType,
  }: {
    isOpen: boolean;
    onClose: () => void;
    defaultProjectId?: string;
    defaultBaseBranch?: string;
    defaultSessionType?: string;
  }) => isOpen ? (
    <div data-testid="create-task-modal" data-terminal-focus-blocker="true">
      <div data-testid="create-task-default-project">{defaultProjectId ?? ""}</div>
      <div data-testid="create-task-default-base-branch">{defaultBaseBranch ?? ""}</div>
      <div data-testid="create-task-default-session">{defaultSessionType ?? ""}</div>
      <button type="button" onClick={onClose}>close create modal</button>
    </div>
  ) : null,
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

vi.mock("@/desktop/renderer/components/TerminalLoader", async () => {
  const React = await vi.importActual<typeof import("react")>("react");

  return {
    default: function MockTerminalLoader() {
      const inputRef = React.useRef<HTMLInputElement>(null);

      React.useEffect(() => {
        function handleRequestTerminalFocus() {
          if (document.querySelector('[data-terminal-focus-blocker="true"]')) {
            return;
          }

          inputRef.current?.focus();
        }

        window.addEventListener("kanvibe:request-terminal-focus", handleRequestTerminalFocus);
        return () => {
          window.removeEventListener("kanvibe:request-terminal-focus", handleRequestTerminalFocus);
        };
      }, []);

      return <input data-testid="terminal-loader" ref={inputRef} aria-label="terminal input" />;
    },
  };
});

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
    mocks.getAllProjects.mockResolvedValue([
      {
        id: "project-1",
        name: "kanvibe",
        repoPath: "/repo/kanvibe",
        defaultBranch: "main",
        sshHost: null,
        isWorktree: false,
        color: null,
        createdAt: new Date(),
      },
    ]);
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(false);
    mocks.getSidebarHintDismissed.mockResolvedValue(false);
    mocks.getDoneAlertDismissed.mockResolvedValue(false);
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
    mocks.listNotifications.mockResolvedValue([]);
    mocks.markNotificationRead.mockResolvedValue(undefined);
    mocks.markAllNotificationsRead.mockResolvedValue(undefined);
    mocks.activateNotification.mockResolvedValue(true);
    mocks.updateTaskStatus.mockResolvedValue(null);
    mocks.deleteTask.mockResolvedValue(true);
    mocks.fetchPrUrlWithPrompt.mockResolvedValue(null);
  });

  afterEach(() => {
    vi.useRealTimers();
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

  it("초기 task 조회가 끝나지 않아도 Loading 화면에 고착되지 않는다", async () => {
    vi.useFakeTimers();
    mocks.getTaskById.mockReturnValue(new Promise(() => {}));

    render(<TaskDetailRoute />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(INITIAL_DESKTOP_LOAD_TIMEOUT_MS);
    });

    expect(screen.queryByText("Loading...")).toBeNull();
    expect(screen.getByText("taskNotFound")).toBeTruthy();
  });

  it("앱 설정을 로드하기 전에는 stale cache의 사이드바 힌트 값을 신뢰하지 않는다", () => {
    sessionStorage.setItem(TASK_DETAIL_CACHE_KEY, JSON.stringify({
      task: {
        id: "task-1",
        title: "cached task title",
        description: null,
        branchName: "feat/cached",
        baseBranch: "main",
        prUrl: null,
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
      projects: [],
      defaultSessionType: "tmux",
      sidebarDefaultCollapsed: false,
      sidebarHintDismissed: false,
      doneAlertDismissed: false,
    }));
    mocks.getTaskById.mockReturnValue(new Promise(() => {}));

    render(<TaskDetailRoute />);

    expect(screen.getByTestId("collapsible-sidebar").dataset.showHint).toBe("false");
    expect(screen.queryByText("sidebar hint visible")).toBeNull();
  });

  it("사이드바 힌트를 닫으면 현재 route state와 cache를 전역 dismissed 상태로 갱신한다", async () => {
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-shortcut",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/detail-shortcut",
    });

    render(<TaskDetailRoute />);

    await screen.findByText("sidebar hint visible");

    fireEvent.click(screen.getByRole("button", { name: "dismiss sidebar hint" }));

    await waitFor(() => {
      expect(screen.queryByText("sidebar hint visible")).toBeNull();
    });

    await waitFor(() => {
      const cached = JSON.parse(sessionStorage.getItem(TASK_DETAIL_CACHE_KEY) ?? "{}") as { sidebarHintDismissed?: boolean };
      expect(cached.sidebarHintDismissed).toBe(true);
    });
  });

  it("알림 단축키로 상세 화면의 알림 센터를 토글한다", async () => {
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-shortcut",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/detail-shortcut",
    });

    render(
      <BoardCommandProvider>
        <TaskDetailRoute />
      </BoardCommandProvider>,
    );

    await screen.findByRole("button", { name: "notifications" });

    fireEvent.keyDown(window, {
      key: "i",
      ctrlKey: true,
      shiftKey: true,
    });

    await waitFor(() => {
      expect(screen.getByText("noNotifications")).toBeTruthy();
    });
  });

  it("새 task 단축키로 현재 상세 task의 branch를 base로 하는 생성 모달을 연다", async () => {
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-shortcut",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/detail-shortcut",
    });

    render(
      <BoardCommandProvider>
        <TaskDetailRoute />
      </BoardCommandProvider>,
    );

    await screen.findByTestId("task-title");

    fireEvent.keyDown(window, {
      key: "n",
      ctrlKey: true,
    });

    await waitFor(() => {
      expect(screen.getByTestId("create-task-default-project").textContent).toBe("project-1");
      expect(screen.getByTestId("create-task-default-base-branch").textContent).toBe("feat/detail-shortcut");
      expect(screen.getByTestId("create-task-default-session").textContent).toBe("tmux");
    });
  });

  it("새 task 모달이 닫히면 terminal 입력 포커스로 복귀한다", async () => {
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-shortcut",
      baseBranch: "main",
      prUrl: null,
      sessionType: "tmux",
      sessionName: "task-session",
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/detail-shortcut",
    });

    render(
      <BoardCommandProvider>
        <TaskDetailRoute />
      </BoardCommandProvider>,
    );

    const terminalInput = await screen.findByLabelText("terminal input");

    fireEvent.keyDown(window, {
      key: "n",
      ctrlKey: true,
    });
    await screen.findByTestId("create-task-modal");

    fireEvent.click(screen.getByRole("button", { name: "close create modal" }));

    await waitFor(() => {
      expect(document.activeElement).toBe(terminalInput);
    });
  });

  it("알림 dropdown이 닫히면 terminal 입력 포커스로 복귀한다", async () => {
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-shortcut",
      baseBranch: "main",
      prUrl: null,
      sessionType: "tmux",
      sessionName: "task-session",
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/detail-shortcut",
    });

    render(
      <BoardCommandProvider>
        <TaskDetailRoute />
      </BoardCommandProvider>,
    );

    const terminalInput = await screen.findByLabelText("terminal input");

    fireEvent.keyDown(window, {
      key: "i",
      ctrlKey: true,
      shiftKey: true,
    });

    const panel = await screen.findByRole("dialog", { name: "notifications" });
    await waitFor(() => {
      expect(document.activeElement).toBe(panel);
    });

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(document.activeElement).toBe(terminalInput);
    });
  });
});
