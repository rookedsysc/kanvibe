import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BoardCommandProvider,
  useBoardCommands,
  useHasBoardShortcutBlocker,
} from "@/desktop/renderer/components/BoardCommandProvider";
import TaskDetailRoute from "@/desktop/renderer/routes/TaskDetailRoute";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";

const TASK_DETAIL_CACHE_KEY = "kanvibe:route-cache:task-detail:task-1";
const BOARD_FOCUS_TASK_CACHE_KEY = "kanvibe:route-cache:board-focus-task";
const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;

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
  getTaskAiSessionDetail: vi.fn(),
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
  renderHooksStatusCard: vi.fn(),
  useRefreshSignal: vi.fn(() => 0),
  redirect: vi.fn(),
  push: vi.fn(),
  back: vi.fn(),
  forward: vi.fn(),
  mermaidInitialize: vi.fn(),
  mermaidRender: vi.fn(async (_id: string, definition: string) => ({
    svg: `<svg data-testid="rendered-mermaid-svg"><g onload="alert('svg')"><script>svg-xss</script><text>${definition}</text></g></svg>`,
  })),
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

function TaskDetailShortcutBlocker() {
  const boardCommands = useBoardCommands();
  const hasShortcutBlocker = useHasBoardShortcutBlocker();

  useEffect(() => boardCommands.registerShortcutBlocker(), [boardCommands]);

  return (
    <span data-testid="shortcut-blocker-state">
      {hasShortcutBlocker ? "blocked" : "open"}
    </span>
  );
}

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

vi.mock("@hugeicons/react", () => ({
  HugeiconsIcon: ({
    icon,
    "data-testid": testId = "hugeicons-icon",
    ...props
  }: {
    icon?: { __iconName?: string };
    "data-testid"?: string;
  }) => (
    <svg
      {...props}
      data-testid={testId}
      data-icon-name={icon?.__iconName ?? "unknown"}
    />
  ),
}));

vi.mock("@hugeicons/core-free-icons", () => ({
  Chatting01Icon: { __iconName: "Chatting01Icon" },
  InformationCircleIcon: { __iconName: "InformationCircleIcon" },
}));

vi.mock("mermaid", () => ({
  default: {
    initialize: mocks.mermaidInitialize,
    render: mocks.mermaidRender,
  },
}));

vi.mock("react-router-dom", () => ({
  useParams: () => ({ id: "task-1" }),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  localizeHref: (href: string, currentLocale = "ko") => href.startsWith("/") ? `/${currentLocale}${href}` : href,
  redirect: (...args: unknown[]) => mocks.redirect(...args),
  usePathname: () => "/ko/task/task-1",
  useRouter: () => ({ push: mocks.push, back: mocks.back, forward: mocks.forward }),
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
  getTaskAiSessionDetail: (...args: unknown[]) => mocks.getTaskAiSessionDetail(...args),
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
  default: (props: {
    initialClaudeStatus: { installed: boolean } | null;
    initialGeminiStatus: { installed: boolean } | null;
    initialCodexStatus: { installed: boolean } | null;
    initialOpenCodeStatus: { installed: boolean } | null;
  }) => {
    mocks.renderHooksStatusCard(props);

    return (
      <div
        data-testid="hooks-status-card"
        data-claude-installed={String(props.initialClaudeStatus?.installed ?? false)}
        data-gemini-installed={String(props.initialGeminiStatus?.installed ?? false)}
        data-codex-installed={String(props.initialCodexStatus?.installed ?? false)}
        data-opencode-installed={String(props.initialOpenCodeStatus?.installed ?? false)}
      />
    );
  },
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
    mocks.getTaskAiSessionDetail.mockResolvedValue({
      sessionId: "empty-session",
      provider: "claude",
      title: null,
      matchedPath: null,
      messages: [],
      nextCursor: null,
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
    vi.restoreAllMocks();
    if (typeof originalScrollIntoView === "function") {
      Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
        configurable: true,
        value: originalScrollIntoView,
      });
    } else {
      delete (HTMLElement.prototype as Partial<HTMLElement>).scrollIntoView;
    }
    delete window.kanvibeDesktop;
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
    fireEvent.click(screen.getByRole("button", { name: "info" }));
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

  it("현재 task id를 보드 복귀 focus target으로 기록한다", async () => {
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-focus-return",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/detail-focus-return",
    });

    render(<TaskDetailRoute />);

    await screen.findByTestId("task-title");

    expect(JSON.parse(sessionStorage.getItem(BOARD_FOCUS_TASK_CACHE_KEY) ?? "null")).toBe("task-1");
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

  it("stale cache의 레거시 사이드바 힌트 값은 아이콘 패널 UI에 렌더링하지 않는다", () => {
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

    expect(screen.queryByText("sidebar hint visible")).toBeNull();
  });

  it("stale cache가 펼침 상태여도 현재 설정이 기본 접기면 작업 정보 패널을 자동으로 열지 않는다", async () => {
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
      doneAlertDismissed: false,
    }));
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockReturnValue(new Promise(() => {}));

    render(<TaskDetailRoute />);

    await screen.findByRole("button", { name: "info" });

    expect(screen.queryByTestId("task-title")).toBeNull();
    expect(screen.queryByTestId("task-info")).toBeNull();
  });

  it("캐시가 있어도 현재 설정이 기본 펼침이고 사용자가 닫지 않았으면 작업 정보 패널을 자동으로 연다", async () => {
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
      sidebarDefaultCollapsed: true,
      defaultPanelDismissed: false,
      doneAlertDismissed: false,
    }));
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(false);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "fresh task title",
      description: null,
      branchName: "feat/fresh",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/fresh",
    });

    render(<TaskDetailRoute />);

    await waitFor(() => {
      expect(screen.getByTestId("task-title").textContent).toBe("fresh task title");
    });
  });

  it("작업 정보 패널을 닫아도 상세 데이터를 다시 조회하지 않는다", async () => {
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-dismiss",
      baseBranch: "main",
      prUrl: null,
      sessionType: "tmux",
      sessionName: "task-session",
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/detail-dismiss",
    });

    render(<TaskDetailRoute />);

    await screen.findByTestId("task-title");
    const taskLoadCountBeforeClose = mocks.getTaskById.mock.calls.length;

    fireEvent.click(screen.getByRole("button", { name: "close" }));

    await waitFor(() => {
      expect(screen.queryByTestId("task-title")).toBeNull();
    });
    await act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 0));
    });
    expect(mocks.getTaskById).toHaveBeenCalledTimes(taskLoadCountBeforeClose);
  });

  it("사용자가 작업 정보 패널을 닫은 뒤 상세 데이터가 새로고침되어도 다시 열지 않는다", async () => {
    let refreshSignal = 0;
    mocks.useRefreshSignal.mockImplementation(() => refreshSignal);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-refresh",
      baseBranch: "main",
      prUrl: null,
      sessionType: "tmux",
      sessionName: "task-session",
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/detail-refresh",
    });

    const { rerender } = render(<TaskDetailRoute />);

    await screen.findByTestId("task-title");

    fireEvent.click(screen.getByRole("button", { name: "close" }));

    expect(screen.queryByTestId("task-title")).toBeNull();

    refreshSignal = 1;
    rerender(<TaskDetailRoute />);

    await waitFor(() => {
      expect(mocks.getTaskById.mock.calls.length).toBeGreaterThan(1);
    });
    await waitFor(() => {
      expect(screen.queryByTestId("task-title")).toBeNull();
    });
  });

  it("아이콘 버튼으로 상세 overview 패널을 열고 닫을 수 있다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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

    const overviewButton = await screen.findByRole("button", { name: "info" });
    expect(screen.queryByTestId("task-title")).toBeNull();

    fireEvent.click(overviewButton);

    await waitFor(() => {
      expect(screen.getByTestId("task-title").textContent).toBe("task title");
    });

    fireEvent.click(screen.getByRole("button", { name: "close" }));

    expect(screen.queryByTestId("task-title")).toBeNull();
  });

  it("PR URL이 있는 task는 dock에 PR 링크를 바로 표시한다", async () => {
    const prUrl = "https://github.com/kanvibe/kanvibe/pull/236";
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-shortcut",
      baseBranch: "main",
      prUrl,
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

    const prLink = await screen.findByRole("link", { name: "PR" });
    expect(prLink.getAttribute("href")).toBe(prUrl);
    expect(prLink.getAttribute("target")).toBe("_blank");
    expect(screen.getByTestId("task-detail-pr-icon")).toBeTruthy();
  });

  it("상세 dock shortcut은 terminal 입력보다 먼저 capture 단계에서 처리한다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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
    const terminalKeyDown = vi.fn();
    const windowBubbleKeyDown = vi.fn();
    terminalInput.addEventListener("keydown", terminalKeyDown);
    window.addEventListener("keydown", windowBubbleKeyDown);

    const wasNotPrevented = fireEvent.keyDown(terminalInput, {
      key: "1",
      altKey: true,
    });
    terminalInput.removeEventListener("keydown", terminalKeyDown);
    window.removeEventListener("keydown", windowBubbleKeyDown);

    expect(wasNotPrevented).toBe(false);
    expect(terminalKeyDown).not.toHaveBeenCalled();
    expect(windowBubbleKeyDown).not.toHaveBeenCalled();
    expect(await screen.findByTestId("task-title")).toBeTruthy();
  });

  it("PR이 있는 task는 dock 4번 shortcut으로 PR을 새 브라우저 창에서 연다", async () => {
    const prUrl = "https://github.com/kanvibe/kanvibe/pull/236";
    const openSpy = vi.spyOn(window, "open").mockImplementation(() => null);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/detail-shortcut",
      baseBranch: "main",
      prUrl,
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

    const prLink = await screen.findByRole("link", { name: "PR" });
    expect(prLink.getAttribute("title")).toContain("Alt+4");

    const wasNotPrevented = fireEvent.keyDown(window, {
      key: "4",
      altKey: true,
    });

    expect(wasNotPrevented).toBe(false);
    expect(openSpy).toHaveBeenCalledWith(prUrl, "_blank", "noopener,noreferrer");
  });

  it("Electron main에서 전달된 dock shortcut도 같은 dock action을 실행한다", async () => {
    let dockShortcutListener: ((index: number) => void) | null = null;
    window.kanvibeDesktop = {
      isDesktop: true,
      onTaskDetailDockShortcut(listener: (index: number) => void) {
        dockShortcutListener = listener;
        return () => {
          dockShortcutListener = null;
        };
      },
    };
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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

    await screen.findByLabelText("terminal input");

    act(() => {
      dockShortcutListener?.(2);
    });

    expect(await screen.findByTestId("hooks-status-card")).toBeTruthy();
  });

  it("최초 등록된 Electron dock shortcut 구독은 task 로드 이후에도 최신 dock 항목을 실행한다", async () => {
    const dockShortcutListeners: ((index: number) => void)[] = [];
    window.kanvibeDesktop = {
      isDesktop: true,
      onTaskDetailDockShortcut(listener: (index: number) => void) {
        dockShortcutListeners.push(listener);
        return () => {};
      },
    };
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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

    await screen.findByLabelText("terminal input");

    const [initialDockShortcutListener] = dockShortcutListeners;
    act(() => {
      initialDockShortcutListener?.(2);
    });

    expect(await screen.findByTestId("hooks-status-card")).toBeTruthy();
  });

  it("Electron dock shortcut 3은 수동 히스토리 버튼 없이 AI 채팅 첫 페이지를 로드한다", async () => {
    let dockShortcutListener: ((index: number) => void) | null = null;
    window.kanvibeDesktop = {
      isDesktop: true,
      onTaskDetailDockShortcut(listener: (index: number) => void) {
        dockShortcutListener = listener;
        return () => {
          dockShortcutListener = null;
        };
      },
    };
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/shortcut-ai-chat",
      baseBranch: "main",
      prUrl: null,
      sessionType: "tmux",
      sessionName: "task-session",
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/shortcut-ai-chat",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/shortcut-ai-chat",
      repoPath: "/repo",
      sessions: [],
      sources: [],
      nextCursor: null,
    });

    render(
      <BoardCommandProvider>
        <TaskDetailRoute />
      </BoardCommandProvider>,
    );

    await screen.findByLabelText("terminal input");
    expect(mocks.getTaskAiSessions).not.toHaveBeenCalled();

    act(() => {
      dockShortcutListener?.(3);
    });

    expect(await screen.findByTestId("inline-ai-chat")).toBeTruthy();
    await waitFor(() => {
      expect(mocks.getTaskAiSessions).toHaveBeenCalledWith("task-1", undefined, null, 20);
    });
    expect(screen.queryByRole("button", { name: "aiSessions.loadHistory" })).toBeNull();
  });

  it("shortcut blocker가 등록되어 있으면 상세 dock shortcut을 실행하지 않는다", async () => {
    let dockShortcutListener: ((index: number) => void) | null = null;
    window.kanvibeDesktop = {
      isDesktop: true,
      onTaskDetailDockShortcut(listener: (index: number) => void) {
        dockShortcutListener = listener;
        return () => {
          dockShortcutListener = null;
        };
      },
    };
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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
        <TaskDetailShortcutBlocker />
        <TaskDetailRoute />
      </BoardCommandProvider>,
    );

    await screen.findByLabelText("terminal input");
    await waitFor(() => {
      expect(screen.getByTestId("shortcut-blocker-state").textContent).toBe("blocked");
    });

    const wasNotPrevented = fireEvent.keyDown(window, {
      key: "2",
      altKey: true,
    });
    act(() => {
      dockShortcutListener?.(2);
    });

    expect(wasNotPrevented).toBe(false);
    expect(screen.queryByTestId("hooks-status-card")).toBeNull();
  });

  it("채팅 아이콘을 클릭하면 drawer 대신 메인 영역을 AI 채팅 내역으로 전환한다", async () => {
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
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/detail-shortcut",
      repoPath: "/repo",
      sessions: [
        {
          id: "claude-session",
          provider: "claude",
          startedAt: null,
          updatedAt: null,
          matchedPath: "/repo__worktrees/detail-shortcut",
          matchScope: "worktree",
          title: "Claude chat",
          firstUserPrompt: "Please fix the UI",
          messageCount: 2,
        },
      ],
      sources: [
        {
          provider: "claude",
          available: true,
          sessionCount: 1,
          reason: null,
        },
      ],
    });
    mocks.getTaskAiSessionDetail.mockResolvedValue({
      sessionId: "claude-session",
      provider: "claude",
      title: "Claude chat",
      matchedPath: "/repo__worktrees/detail-shortcut",
      messages: [
        {
          role: "user",
          timestamp: null,
          text: "Please fix the UI",
          fullText: "Please fix the UI",
          isTruncated: false,
        },
        {
          role: "assistant",
          timestamp: null,
          text: "Updated the terminal chat view.",
          fullText: "Updated the terminal chat view.",
          isTruncated: false,
        },
      ],
      nextCursor: null,
    });

    render(<TaskDetailRoute />);

    await screen.findByLabelText("terminal input");

    expect(mocks.getTaskAiSessions).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "aiSessions.inlineChat" }));

    expect(await screen.findByTestId("inline-ai-chat")).toBeTruthy();
    await waitFor(() => {
      expect(mocks.getTaskAiSessions).toHaveBeenCalledWith("task-1", undefined, null, 20);
    });

    fireEvent.click(await screen.findByRole("button", { name: /Claude chat/ }));

    await waitFor(() => {
      expect(mocks.getTaskAiSessionDetail).toHaveBeenCalledWith(
        "task-1",
        "claude",
        "claude-session",
        null,
        null,
        40,
        undefined,
        undefined,
      );
    });

    await waitFor(() => {
      expect(screen.getAllByText("Please fix the UI").length).toBeGreaterThanOrEqual(1);
    });
    expect(screen.getByText("Updated the terminal chat view.")).toBeTruthy();
    expect(screen.queryByLabelText("terminal input")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "aiSessions.inlineChat" }));

    await waitFor(() => {
      expect(screen.getByLabelText("terminal input")).toBeTruthy();
    });
    expect(screen.queryByTestId("inline-ai-chat")).toBeNull();
  });

  it("AI sessions dialog 진입점은 상세 화면 sidebar에 표시하지 않는다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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

    await screen.findByRole("button", { name: "info" });

    expect(screen.queryByRole("button", { name: "aiSessions.title" })).toBeNull();
  });

  it("상태 변경과 hooks 상태는 하나의 status 패널에서 함께 보여준다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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

    const statusButton = await screen.findByRole("button", { name: "actions · hooksStatus" });
    expect(screen.queryByTestId("hooks-status-card")).toBeNull();
    expect(screen.getByTestId("task-status-panel-icon").getAttribute("data-icon-name")).toBe("AntennaSignalIcon");

    fireEvent.click(statusButton);

    expect(await screen.findByTestId("hooks-status-card")).toBeTruthy();
    expect(screen.getByText("done")).toBeTruthy();
  });

  it("AI 세션 로드가 느려도 hooks 상태는 먼저 갱신한다", async () => {
    const codexStatus = { installed: true };
    const unresolvedAiSessions = createDeferred<Awaited<ReturnType<typeof mocks.getTaskAiSessions>>>();
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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
    mocks.getTaskCodexHooksStatus.mockResolvedValue(codexStatus);
    mocks.getTaskAiSessions.mockReturnValue(unresolvedAiSessions.promise);

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "actions · hooksStatus" }));

    await waitFor(() => {
      expect(screen.getByTestId("hooks-status-card").getAttribute("data-codex-installed")).toBe("true");
    });
    expect(mocks.renderHooksStatusCard).toHaveBeenLastCalledWith(expect.objectContaining({
      initialCodexStatus: codexStatus,
    }));
  });

  it("상세 화면 좌측 하단 rail에는 알림 버튼을 렌더링하지 않는다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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

    await screen.findByRole("button", { name: "info" });

    expect(screen.queryByRole("button", { name: "notifications" })).toBeNull();
  });

  it("서랍이 열린 상태에서 터미널 히스토리창을 누르면 서랍을 닫는다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
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

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "info" }));
    await screen.findByTestId("task-title");

    fireEvent.click(screen.getByTestId("terminal-loader"));

    await waitFor(() => {
      expect(screen.queryByTestId("task-title")).toBeNull();
    });
  });

  it("상세 화면 페이지 이동 단축키는 터미널 입력보다 먼저 capture 단계에서 처리한다", async () => {
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
    const terminalKeyDown = vi.fn();
    const windowBubbleKeyDown = vi.fn();
    terminalInput.addEventListener("keydown", terminalKeyDown);
    window.addEventListener("keydown", windowBubbleKeyDown);

    const wasNotPrevented = fireEvent.keyDown(terminalInput, {
      key: "[",
      ctrlKey: true,
    });
    terminalInput.removeEventListener("keydown", terminalKeyDown);
    window.removeEventListener("keydown", windowBubbleKeyDown);

    expect(wasNotPrevented).toBe(false);
    expect(mocks.back).toHaveBeenCalledTimes(1);
    expect(terminalKeyDown).not.toHaveBeenCalled();
    expect(windowBubbleKeyDown).not.toHaveBeenCalled();

    fireEvent.keyDown(terminalInput, {
      key: "]",
      ctrlKey: true,
    });

    expect(mocks.forward).toHaveBeenCalledTimes(1);
    expect(mocks.back).toHaveBeenCalledTimes(1);
  });

  it("shortcut blocker가 등록되어 있으면 상세 화면 페이지 이동 단축키를 실행하지 않는다", async () => {
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
        <TaskDetailShortcutBlocker />
        <TaskDetailRoute />
      </BoardCommandProvider>,
    );

    const terminalInput = await screen.findByLabelText("terminal input");
    await waitFor(() => {
      expect(screen.getByTestId("shortcut-blocker-state").textContent).toBe("blocked");
    });

    const wasBackNotPrevented = fireEvent.keyDown(terminalInput, {
      key: "[",
      ctrlKey: true,
    });
    const wasForwardNotPrevented = fireEvent.keyDown(terminalInput, {
      key: "]",
      ctrlKey: true,
    });

    expect(wasBackNotPrevented).toBe(false);
    expect(wasForwardNotPrevented).toBe(false);
    expect(mocks.back).not.toHaveBeenCalled();
    expect(mocks.forward).not.toHaveBeenCalled();
  });

  it("알림 단축키로 상세 화면의 알림 센터를 토글한다", async () => {
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

  it("상세 화면 로드만으로 AI 세션 히스토리를 읽지 않고 채팅 버튼 클릭 때 최신 첫 페이지를 로드한다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/manual-ai-history",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/manual-ai-history",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/manual-ai-history",
      repoPath: "/repo",
      sessions: [],
      sources: [],
    });

    render(<TaskDetailRoute />);

    await screen.findByRole("button", { name: "aiSessions.inlineChat" });
    expect(mocks.getTaskAiSessions).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "aiSessions.inlineChat" }));

    await waitFor(() => {
      expect(mocks.getTaskAiSessions).toHaveBeenCalledTimes(1);
      expect(mocks.getTaskAiSessions).toHaveBeenCalledWith("task-1", undefined, null, 20);
    });
    expect(screen.queryByRole("button", { name: "aiSessions.loadHistory" })).toBeNull();
    expect(screen.queryByText("aiSessions.loadHistoryHint")).toBeNull();
  });

  it("채팅 화면 세션 목록은 최신순 첫 페이지를 받고 다음 페이지를 이어 붙인다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/paged-ai-history",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/paged-ai-history",
    });
    mocks.getTaskAiSessions
      .mockResolvedValueOnce({
        isRemote: false,
        targetPath: "/repo__worktrees/paged-ai-history",
        repoPath: "/repo",
        sources: [],
        nextCursor: "20",
        sessions: [
          { id: "newest-session", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Newest chat", firstUserPrompt: "Newest prompt", messageCount: 2, sourceRef: "newest.jsonl" },
        ],
      })
      .mockResolvedValueOnce({
        isRemote: false,
        targetPath: "/repo__worktrees/paged-ai-history",
        repoPath: "/repo",
        sources: [],
        nextCursor: null,
        sessions: [
          { id: "older-session", provider: "codex", startedAt: null, updatedAt: "2026-01-01T00:02:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Older chat", firstUserPrompt: "Older prompt", messageCount: 3, sourceRef: "older.jsonl" },
        ],
      });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));

    expect(await screen.findByRole("button", { name: /Newest chat/ })).toBeTruthy();
    expect(mocks.getTaskAiSessions).toHaveBeenCalledWith("task-1", undefined, null, 20);

    fireEvent.click(screen.getByRole("button", { name: "aiSessions.loadMoreSessions" }));

    await waitFor(() => {
      expect(mocks.getTaskAiSessions).toHaveBeenLastCalledWith("task-1", undefined, "20", 20);
    });
    expect(await screen.findByRole("button", { name: /Older chat/ })).toBeTruthy();
  });

  it("provider 필터가 현재 페이지를 모두 숨겨도 다음 세션 페이지를 불러올 수 있다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/paged-provider-filter",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/paged-provider-filter",
    });
    mocks.getTaskAiSessions
      .mockResolvedValueOnce({
        isRemote: false,
        targetPath: "/repo__worktrees/paged-provider-filter",
        repoPath: "/repo",
        sources: [],
        nextCursor: "20",
        sessions: [
          { id: "claude-newest", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Claude newest", firstUserPrompt: "Claude prompt", messageCount: 2, sourceRef: "claude.jsonl" },
        ],
      })
      .mockResolvedValueOnce({
        isRemote: false,
        targetPath: "/repo__worktrees/paged-provider-filter",
        repoPath: "/repo",
        sources: [],
        nextCursor: null,
        sessions: [
          { id: "codex-older", provider: "codex", startedAt: null, updatedAt: "2026-01-01T00:02:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Codex older", firstUserPrompt: "Codex prompt", messageCount: 3, sourceRef: "codex.jsonl" },
        ],
      });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));
    expect(await screen.findByRole("button", { name: /Claude newest/ })).toBeTruthy();

    fireEvent.click(screen.getByTestId("ai-session-filter-codex"));

    expect(screen.queryByRole("button", { name: /Claude newest/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "aiSessions.loadMoreSessions" }));

    await waitFor(() => {
      expect(mocks.getTaskAiSessions).toHaveBeenLastCalledWith("task-1", undefined, "20", 20);
    });
    expect(await screen.findByRole("button", { name: /Codex older/ })).toBeTruthy();
  });

  it("채팅 상세 메시지는 markdown과 mermaid 다이어그램을 렌더링하고 위험한 HTML은 제거한다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/markdown-mermaid-ai-chat",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/markdown-mermaid-ai-chat",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/markdown-mermaid-ai-chat",
      repoPath: "/repo",
      sources: [],
      nextCursor: null,
      sessions: [
        { id: "claude-1", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Markdown diagram chat", firstUserPrompt: "Show diagram", messageCount: 1, sourceRef: "claude.jsonl" },
      ],
    });
    mocks.getTaskAiSessionDetail.mockResolvedValue({
      sessionId: "claude-1",
      provider: "claude",
      title: "Markdown diagram chat",
      matchedPath: "/repo",
      sourceRef: "claude.jsonl",
      nextCursor: null,
      messages: [
        {
          role: "assistant",
          timestamp: "2026-01-01T00:03:00.000Z",
          text: "markdown preview",
          fullText: [
            "## 작업 요약",
            "",
            "- 첫 번째 항목",
            "",
            "```ts",
            "const value = 1;",
            "```",
            "",
            "```mermaid",
            "graph TD",
            "  A[Start] --> B[Done]",
            "```",
            "",
            '<script>alert("xss")</script>',
            '<img src="x" style="color: red" onerror="alert(1)" alt="unsafe image">',
            '[위험 링크](javascript:alert("link"))',
            '<svg><script>markdown-svg-xss</script></svg>',
          ].join("\n"),
          isTruncated: false,
        },
      ],
    });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));
    fireEvent.click(await screen.findByRole("button", { name: /Markdown diagram chat/ }));

    expect(await screen.findByRole("heading", { level: 2, name: "작업 요약" })).toBeTruthy();
    expect(screen.getByText("첫 번째 항목").tagName).toBe("LI");
    expect(screen.getByText("const value = 1;").closest("pre")).toBeTruthy();
    expect(await screen.findByTestId("rendered-mermaid-svg")).toBeTruthy();
    expect(screen.getByText(/graph TD/)).toBeTruthy();
    expect(screen.queryByText(/alert\("xss"\)/)).toBeNull();
    expect(screen.queryByText("markdown-svg-xss")).toBeNull();
    expect(screen.queryByText("svg-xss")).toBeNull();
    expect(document.querySelector('.ai-session-markdown a[href^="javascript:"]')).toBeNull();
    expect(document.querySelector(".ai-session-markdown [style]")).toBeNull();
    expect(document.querySelector(".ai-session-markdown [onerror]")).toBeNull();
    expect(document.querySelector(".ai-session-markdown svg")).toBeNull();
    expect(document.querySelector("[onload]")).toBeNull();
    expect(mocks.mermaidInitialize).toHaveBeenCalledWith(expect.objectContaining({
      securityLevel: "strict",
      startOnLoad: false,
      theme: "dark",
    }));
    expect(mocks.mermaidRender.mock.calls[0]?.[0]).toMatch(/^ai-session-mermaid-/);
  });

  it("채팅 상세 메시지는 최신순 페이지를 받고 더보기로 이전 메시지를 이어 붙인다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/paged-ai-detail",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/paged-ai-detail",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/paged-ai-detail",
      repoPath: "/repo",
      sources: [],
      nextCursor: null,
      sessions: [
        { id: "claude-1", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Claude paged chat", firstUserPrompt: "Claude prompt", messageCount: 80, sourceRef: "claude.jsonl" },
      ],
    });
    mocks.getTaskAiSessionDetail
      .mockResolvedValueOnce({
        sessionId: "claude-1",
        provider: "claude",
        title: "Claude paged chat",
        matchedPath: "/repo",
        sourceRef: "claude.jsonl",
        nextCursor: "40",
        messages: [
          { role: "assistant", timestamp: "2026-01-01T00:04:00.000Z", text: "Newest answer", fullText: "Newest answer", isTruncated: false },
        ],
      })
      .mockResolvedValueOnce({
        sessionId: "claude-1",
        provider: "claude",
        title: "Claude paged chat",
        matchedPath: "/repo",
        sourceRef: "claude.jsonl",
        nextCursor: null,
        messages: [
          { role: "user", timestamp: "2026-01-01T00:03:00.000Z", text: "Older prompt", fullText: "Older prompt", isTruncated: false },
        ],
      });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));
    fireEvent.click(await screen.findByRole("button", { name: /Claude paged chat/ }));

    expect(await screen.findByText("Newest answer")).toBeTruthy();
    expect(mocks.getTaskAiSessionDetail).toHaveBeenLastCalledWith(
      "task-1",
      "claude",
      "claude-1",
      "claude.jsonl",
      null,
      40,
      undefined,
      undefined,
    );

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.loadOlderMessages" }));

    await waitFor(() => {
      expect(mocks.getTaskAiSessionDetail).toHaveBeenLastCalledWith(
        "task-1",
        "claude",
        "claude-1",
        "claude.jsonl",
        "40",
        40,
        undefined,
        undefined,
      );
    });
    expect(await screen.findByText("Older prompt")).toBeTruthy();
  });

  it("역할 필터 변경 뒤 늦게 도착한 이전 메시지 페이지를 상세에 섞지 않는다", async () => {
    const staleOlderMessages = createDeferred<Awaited<ReturnType<typeof mocks.getTaskAiSessionDetail>>>();
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/stale-role-detail",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/stale-role-detail",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/stale-role-detail",
      repoPath: "/repo",
      sources: [],
      nextCursor: null,
      sessions: [
        { id: "claude-1", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Claude filtered chat", firstUserPrompt: "Claude prompt", messageCount: 80, sourceRef: "claude.jsonl" },
      ],
    });
    mocks.getTaskAiSessionDetail
      .mockResolvedValueOnce({
        sessionId: "claude-1",
        provider: "claude",
        title: "Claude filtered chat",
        matchedPath: "/repo",
        sourceRef: "claude.jsonl",
        nextCursor: "40",
        messages: [
          { role: "assistant", timestamp: "2026-01-01T00:04:00.000Z", text: "Current assistant answer", fullText: "Current assistant answer", isTruncated: false },
        ],
      })
      .mockReturnValueOnce(staleOlderMessages.promise)
      .mockResolvedValueOnce({
        sessionId: "claude-1",
        provider: "claude",
        title: "Claude filtered chat",
        matchedPath: "/repo",
        sourceRef: "claude.jsonl",
        nextCursor: null,
        messages: [
          { role: "assistant", timestamp: "2026-01-01T00:04:00.000Z", text: "Filtered assistant answer", fullText: "Filtered assistant answer", isTruncated: false },
        ],
      });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));
    fireEvent.click(await screen.findByRole("button", { name: /Claude filtered chat/ }));
    expect(await screen.findByText("Current assistant answer")).toBeTruthy();

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.loadOlderMessages" }));
    await waitFor(() => {
      expect(mocks.getTaskAiSessionDetail).toHaveBeenLastCalledWith(
        "task-1",
        "claude",
        "claude-1",
        "claude.jsonl",
        "40",
        40,
        undefined,
        undefined,
      );
    });

    fireEvent.click(screen.getByRole("button", { name: "aiSessions.roles.assistant" }));
    await waitFor(() => {
      expect(mocks.getTaskAiSessionDetail).toHaveBeenLastCalledWith(
        "task-1",
        "claude",
        "claude-1",
        "claude.jsonl",
        null,
        40,
        undefined,
        ["assistant"],
      );
    });
    expect(await screen.findByText("Filtered assistant answer")).toBeTruthy();

    await act(async () => {
      staleOlderMessages.resolve({
        sessionId: "claude-1",
        provider: "claude",
        title: "Claude filtered chat",
        matchedPath: "/repo",
        sourceRef: "claude.jsonl",
        nextCursor: null,
        messages: [
          { role: "user", timestamp: "2026-01-01T00:03:00.000Z", text: "Stale user prompt", fullText: "Stale user prompt", isTruncated: false },
        ],
      });
    });

    expect(screen.getByText("Filtered assistant answer")).toBeTruthy();
    expect(screen.queryByText("Stale user prompt")).toBeNull();
  });

  it("채팅 상세를 선택하면 최신 메시지 위치로 자동 스크롤하고 시간순으로 표시한다", async () => {
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/auto-scroll-ai-chat",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/auto-scroll-ai-chat",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/auto-scroll-ai-chat",
      repoPath: "/repo",
      sources: [],
      nextCursor: null,
      sessions: [
        { id: "claude-1", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo__worktrees/auto-scroll-ai-chat", matchScope: "worktree", title: "Claude scrolled chat", firstUserPrompt: "Older prompt", messageCount: 2, sourceRef: "claude.jsonl" },
      ],
    });
    mocks.getTaskAiSessionDetail.mockResolvedValue({
      sessionId: "claude-1",
      provider: "claude",
      title: "Claude scrolled chat",
      matchedPath: "/repo__worktrees/auto-scroll-ai-chat",
      sourceRef: "claude.jsonl",
      nextCursor: null,
      messages: [
        { role: "assistant", timestamp: "2026-01-01T00:03:00.000Z", text: "Newest answer", fullText: "Newest answer", isTruncated: false },
        { role: "user", timestamp: "2026-01-01T00:02:00.000Z", text: "Older prompt", fullText: "Older prompt", isTruncated: false },
      ],
    });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));
    fireEvent.click(await screen.findByRole("button", { name: /Claude scrolled chat/ }));

    const messagePane = await screen.findByTestId("ai-session-messages");
    await waitFor(() => {
      expect(scrollIntoView).toHaveBeenCalledWith({ block: "end" });
    });
    expect(messagePane.textContent?.indexOf("Older prompt")).toBeLessThan(messagePane.textContent?.indexOf("Newest answer") ?? -1);
  });

  it("채팅 화면에서 Claude/Codex/OpenCode/Gemini 세션을 한 목록에 표시하고 provider를 구분한다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/unified-ai-history",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/unified-ai-history",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/unified-ai-history",
      repoPath: "/repo",
      sources: [],
      sessions: [
        { id: "gemini-1", provider: "gemini", startedAt: null, updatedAt: "2026-01-01T00:04:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Gemini answer", firstUserPrompt: "Gemini prompt", messageCount: 3, sourceRef: "gemini.json" },
        { id: "claude-1", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Claude answer", firstUserPrompt: "Claude prompt", messageCount: 2, sourceRef: "claude.jsonl" },
        { id: "opencode-1", provider: "opencode", startedAt: null, updatedAt: "2026-01-01T00:02:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "OpenCode answer", firstUserPrompt: "OpenCode prompt", messageCount: 4, sourceRef: "opencode-1" },
        { id: "codex-1", provider: "codex", startedAt: null, updatedAt: "2026-01-01T00:01:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Codex answer", firstUserPrompt: "Codex prompt", messageCount: 5, sourceRef: "codex.jsonl" },
      ],
    });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));

    expect(await screen.findByTestId("ai-session-list")).toBeTruthy();
    for (const provider of ["gemini", "claude", "opencode", "codex"]) {
      const icon = screen.getByTestId(`ai-session-provider-${provider}`);
      expect(icon).toBeTruthy();
      expect(icon.getAttribute("data-icon-source")).toBe("lobehub-icons");
    }
    expect(screen.getByRole("button", { name: /Gemini answer/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Claude answer/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /OpenCode answer/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Codex answer/ })).toBeTruthy();
  });

  it("AI provider 아이콘 rail은 OR 조건으로 세션 목록을 필터링한다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/provider-or-filter",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/provider-or-filter",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/provider-or-filter",
      repoPath: "/repo",
      sources: [],
      sessions: [
        { id: "gemini-1", provider: "gemini", startedAt: null, updatedAt: "2026-01-01T00:04:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Gemini answer", firstUserPrompt: "Gemini prompt", messageCount: 3, sourceRef: "gemini.json" },
        { id: "claude-1", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Claude answer", firstUserPrompt: "Claude prompt", messageCount: 2, sourceRef: "claude.jsonl" },
        { id: "opencode-1", provider: "opencode", startedAt: null, updatedAt: "2026-01-01T00:02:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "OpenCode answer", firstUserPrompt: "OpenCode prompt", messageCount: 4, sourceRef: "opencode-1" },
        { id: "codex-1", provider: "codex", startedAt: null, updatedAt: "2026-01-01T00:01:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Codex answer", firstUserPrompt: "Codex prompt", messageCount: 5, sourceRef: "codex.jsonl" },
      ],
    });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));
    await screen.findByTestId("ai-session-list");

    fireEvent.click(screen.getByTestId("ai-session-filter-claude"));
    expect(screen.getByRole("button", { name: /Claude answer/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Gemini answer/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /OpenCode answer/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Codex answer/ })).toBeNull();

    fireEvent.click(screen.getByTestId("ai-session-filter-gemini"));
    expect(screen.getByRole("button", { name: /Claude answer/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Gemini answer/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /OpenCode answer/ })).toBeNull();
    expect(screen.queryByRole("button", { name: /Codex answer/ })).toBeNull();

    fireEvent.click(screen.getByTestId("ai-session-filter-claude"));
    expect(screen.queryByRole("button", { name: /Claude answer/ })).toBeNull();
    expect(screen.getByRole("button", { name: /Gemini answer/ })).toBeTruthy();
  });

  it("AI 채팅 검색은 입력한 검색어로 히스토리를 다시 조회하고 결과 세션을 보여준다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/ai-chat-search",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/ai-chat-search",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/ai-chat-search",
      repoPath: "/repo",
      sources: [],
      sessions: [
        { id: "gemini-1", provider: "gemini", startedAt: null, updatedAt: "2026-01-01T00:04:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Gemini architecture answer", firstUserPrompt: "Find database migration chat", messageCount: 3, sourceRef: "gemini.json" },
      ],
    });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));
    const searchInput = screen.getByLabelText("aiSessions.searchLabel");
    fireEvent.change(searchInput, { target: { value: "database migration" } });
    fireEvent.submit(searchInput.closest("form")!);

    await waitFor(() => {
      expect(mocks.getTaskAiSessions).toHaveBeenCalledWith("task-1", "database migration", null, 20);
    });
    expect(await screen.findByRole("button", { name: /Gemini architecture answer/ })).toBeTruthy();
  });

  it("검색이 바뀐 뒤 늦게 도착한 이전 세션 페이지를 현재 결과에 섞지 않는다", async () => {
    const staleAppend = createDeferred<Awaited<ReturnType<typeof mocks.getTaskAiSessions>>>();
    const filteredSearch = createDeferred<Awaited<ReturnType<typeof mocks.getTaskAiSessions>>>();
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/stale-session-page",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/stale-session-page",
    });
    mocks.getTaskAiSessions
      .mockResolvedValueOnce({
        isRemote: false,
        targetPath: "/repo__worktrees/stale-session-page",
        repoPath: "/repo",
        sources: [],
        nextCursor: "20",
        sessions: [
          { id: "initial-session", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Initial chat", firstUserPrompt: "Initial prompt", messageCount: 2, sourceRef: "initial.jsonl" },
        ],
      })
      .mockReturnValueOnce(staleAppend.promise)
      .mockReturnValueOnce(filteredSearch.promise);

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));
    expect(await screen.findByRole("button", { name: /Initial chat/ })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "aiSessions.loadMoreSessions" }));
    await waitFor(() => {
      expect(mocks.getTaskAiSessions).toHaveBeenLastCalledWith("task-1", undefined, "20", 20);
    });

    const searchInput = screen.getByLabelText("aiSessions.searchLabel");
    fireEvent.change(searchInput, { target: { value: "filtered topic" } });
    fireEvent.submit(searchInput.closest("form")!);
    await waitFor(() => {
      expect(mocks.getTaskAiSessions).toHaveBeenLastCalledWith("task-1", "filtered topic", null, 20);
    });

    await act(async () => {
      filteredSearch.resolve({
        isRemote: false,
        targetPath: "/repo__worktrees/stale-session-page",
        repoPath: "/repo",
        sources: [],
        nextCursor: null,
        sessions: [
          { id: "filtered-session", provider: "gemini", startedAt: null, updatedAt: "2026-01-01T00:04:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Filtered chat", firstUserPrompt: "Filtered prompt", messageCount: 1, sourceRef: "filtered.json" },
        ],
      });
    });
    expect(await screen.findByRole("button", { name: /Filtered chat/ })).toBeTruthy();

    await act(async () => {
      staleAppend.resolve({
        isRemote: false,
        targetPath: "/repo__worktrees/stale-session-page",
        repoPath: "/repo",
        sources: [],
        nextCursor: null,
        sessions: [
          { id: "stale-session", provider: "codex", startedAt: null, updatedAt: "2026-01-01T00:02:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Stale old query chat", firstUserPrompt: "Old prompt", messageCount: 1, sourceRef: "stale.jsonl" },
        ],
      });
    });

    expect(screen.getByRole("button", { name: /Filtered chat/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Stale old query chat/ })).toBeNull();
  });

  it("AI 채팅 상세는 사용자 입력, 시스템 입력, AI 답변과 추가 역할 필터를 전달한다", async () => {
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(true);
    mocks.getTaskById.mockResolvedValue({
      id: "task-1",
      title: "task title",
      description: null,
      branchName: "feat/role-filter",
      baseBranch: "main",
      prUrl: null,
      sessionType: null,
      sessionName: null,
      sshHost: null,
      projectId: "project-1",
      project: { id: "project-1", name: "kanvibe" },
      status: "todo",
      agentType: null,
      worktreePath: "/repo__worktrees/role-filter",
    });
    mocks.getTaskAiSessions.mockResolvedValue({
      isRemote: false,
      targetPath: "/repo__worktrees/role-filter",
      repoPath: "/repo",
      sources: [],
      sessions: [
        { id: "claude-1", provider: "claude", startedAt: null, updatedAt: "2026-01-01T00:03:00.000Z", matchedPath: "/repo", matchScope: "worktree", title: "Claude answer", firstUserPrompt: "Claude prompt", messageCount: 2, sourceRef: "claude.jsonl" },
      ],
    });
    mocks.getTaskAiSessionDetail.mockResolvedValue({
      sessionId: "claude-1",
      provider: "claude",
      title: "Claude answer",
      matchedPath: "/repo",
      sourceRef: "claude.jsonl",
      nextCursor: null,
      messages: [
        { role: "assistant", timestamp: "2026-01-01T00:03:00.000Z", text: "AI reply", fullText: "AI reply", isTruncated: false },
        { role: "system", timestamp: "2026-01-01T00:02:00.000Z", text: "System prompt", fullText: "System prompt", isTruncated: false },
        { role: "user", timestamp: "2026-01-01T00:01:00.000Z", text: "User prompt", fullText: "User prompt", isTruncated: false },
      ],
    });

    render(<TaskDetailRoute />);

    fireEvent.click(await screen.findByRole("button", { name: "aiSessions.inlineChat" }));
    fireEvent.click(await screen.findByRole("button", { name: /Claude answer/ }));

    await waitFor(() => {
      expect(mocks.getTaskAiSessionDetail).toHaveBeenLastCalledWith(
        "task-1",
        "claude",
        "claude-1",
        "claude.jsonl",
        null,
        expect.any(Number),
        undefined,
        undefined,
      );
    });

    fireEvent.click(screen.getByRole("button", { name: "aiSessions.roles.system" }));

    await waitFor(() => {
      expect(mocks.getTaskAiSessionDetail).toHaveBeenLastCalledWith(
        "task-1",
        "claude",
        "claude-1",
        "claude.jsonl",
        null,
        expect.any(Number),
        undefined,
        ["system"],
      );
    });

    expect(screen.getByRole("button", { name: "aiSessions.roles.developer" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "aiSessions.roles.reasoning" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "aiSessions.roles.tool" })).toBeTruthy();
  });
});
