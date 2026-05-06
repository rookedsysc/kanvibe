import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BoardCommandProvider } from "@/desktop/renderer/components/BoardCommandProvider";
import TaskDetailRoute from "@/desktop/renderer/routes/TaskDetailRoute";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";

const TASK_DETAIL_CACHE_KEY = "kanvibe:route-cache:task-detail:task-1";
const BOARD_FOCUS_TASK_CACHE_KEY = "kanvibe:route-cache:board-focus-task";

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

vi.mock("react-router-dom", () => ({
  useParams: () => ({ id: "task-1" }),
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  Link: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  redirect: (...args: unknown[]) => mocks.redirect(...args),
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

    fireEvent.click(screen.getByRole("button", { name: "aiSessions.inlineChat" }));

    expect(await screen.findByTestId("inline-ai-chat")).toBeTruthy();
    expect(screen.getByText("Please fix the UI")).toBeTruthy();
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
      shiftKey: true,
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
      shiftKey: true,
    });

    expect(mocks.forward).toHaveBeenCalledTimes(1);
    expect(mocks.back).toHaveBeenCalledTimes(1);
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
});
