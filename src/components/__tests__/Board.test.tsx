import { forwardRef, useImperativeHandle } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import Board from "../Board";
import { reorderTasks, updateTaskStatus } from "@/desktop/renderer/actions/kanban";
import { SessionType, TaskStatus } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import type { TasksByStatus } from "@/desktop/renderer/actions/kanban";
import { BoardCommandProvider, useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";

function mockNavigatorPlatform(platform: string) {
  Object.defineProperty(window.navigator, "platform", {
    configurable: true,
    value: platform,
  });
}

function mockWindowFind(implementation?: (query: string, ...args: unknown[]) => boolean) {
  const findMock = vi.fn(implementation ?? (() => true));
  Object.defineProperty(window, "find", {
    configurable: true,
    value: findMock,
  });

  return findMock;
}

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

vi.mock("@hello-pangea/dnd", () => ({
  DragDropContext: ({ children, onDragEnd }: { children: React.ReactNode; onDragEnd?: (result: unknown) => void }) => (
    <div>
      <button
        type="button"
        onClick={() =>
          onDragEnd?.({
            draggableId: "task-1",
            source: { droppableId: TaskStatus.TODO, index: 0 },
            destination: { droppableId: TaskStatus.TODO, index: 0 },
          })
        }
      >
        trigger-drag-end
      </button>
      {children}
    </div>
  ),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  reorderTasks: vi.fn(),
  updateTaskStatus: vi.fn(),
  deleteTask: vi.fn(),
  getMoreDoneTasks: vi.fn().mockResolvedValue({ tasks: [], doneTotal: 0 }),
  moveTaskToColumn: vi.fn(),
}));

vi.mock("@/desktop/renderer/hooks/useAutoRefresh", () => ({
  useAutoRefresh: vi.fn(),
}));

vi.mock("@/desktop/renderer/hooks/useProjectFilterParams", () => ({
  useProjectFilterParams: vi.fn().mockReturnValue([[], vi.fn()]),
}));

vi.mock("../Column", () => ({
  default: () => <div data-testid="column" />,
}));

vi.mock("../ProjectSelector", () => ({
  default: forwardRef(function MockProjectSelector(_props, ref) {
    useImperativeHandle(ref, () => ({
      open() {},
      close() {},
      focus() {},
    }), []);

    return <div data-testid="project-selector" />;
  }),
}));

vi.mock("../NotificationCenterButton", () => ({
  default: forwardRef(function MockNotificationCenterButton(_props, ref) {
    useImperativeHandle(ref, () => ({
      open() {},
      close() {},
      toggle() {},
    }), []);

    return <button type="button" aria-label="notifications" />;
  }),
}));

vi.mock("../TaskContextMenu", () => ({
  default: () => <div data-testid="task-context-menu" />,
}));

vi.mock("../DoneConfirmDialog", () => ({
  default: () => <div data-testid="done-confirm-dialog" />,
}));

vi.mock("../BranchTaskModal", () => ({
  default: () => <div data-testid="branch-task-modal" />,
}));

vi.mock("../CreateTaskModal", () => ({
  default: ({
    defaultSessionType,
    defaultProjectId,
    defaultBaseBranch,
  }: {
    defaultSessionType: SessionType;
    defaultProjectId?: string;
    defaultBaseBranch?: string;
  }) => (
    <div>
      <div data-testid="create-task-default-session">{defaultSessionType}</div>
      <div data-testid="create-task-default-project">{defaultProjectId ?? ""}</div>
      <div data-testid="create-task-default-base-branch">{defaultBaseBranch ?? ""}</div>
    </div>
  ),
}));

vi.mock("../ProjectSettings", () => ({
  default: ({
    defaultSessionType,
    onDefaultSessionTypeChange,
  }: {
    defaultSessionType: SessionType;
    onDefaultSessionTypeChange?: (sessionType: SessionType) => void;
  }) => (
    <div>
      <div data-testid="project-settings-default-session">{defaultSessionType}</div>
      <button onClick={() => onDefaultSessionTypeChange?.(SessionType.ZELLIJ)}>change-default-session</button>
    </div>
  ),
}));

function createProject(): Project {
  return {
    id: "project-1",
    name: "kanvibe",
    repoPath: "/repo/kanvibe",
    defaultBranch: "main",
    sshHost: null,
    isWorktree: false,
    color: null,
    createdAt: new Date(),
  };
}

function createEmptyTasks(): TasksByStatus {
  return {
    [TaskStatus.TODO]: [],
    [TaskStatus.PROGRESS]: [],
    [TaskStatus.PENDING]: [],
    [TaskStatus.REVIEW]: [],
    [TaskStatus.DONE]: [],
  };
}

function createTasksWithTodo(): TasksByStatus {
  return {
    [TaskStatus.TODO]: [
      {
        id: "task-1",
        title: "Test Task",
        description: null,
        status: TaskStatus.TODO,
        branchName: null,
        worktreePath: null,
        sessionType: null,
        sessionName: null,
        sshHost: null,
        agentType: null,
        project: null,
        projectId: "project-1",
        baseBranch: null,
        prUrl: null,
        priority: null,
        displayOrder: 0,
        createdAt: new Date(),
        updatedAt: new Date(),
      },
    ],
    [TaskStatus.PROGRESS]: [],
    [TaskStatus.PENDING]: [],
    [TaskStatus.REVIEW]: [],
    [TaskStatus.DONE]: [],
  };
}

function BoardCommandRequester() {
  const boardCommands = useBoardCommands();

  return (
    <button
      type="button"
      onClick={() => boardCommands.requestCreateBranchTodo({
        projectId: "project-1",
        baseBranch: "feat/from-search",
      })}
    >
      request branch todo
    </button>
  );
}

describe("Board defaultSessionType sync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    delete window.kanvibeDesktop;
    mockNavigatorPlatform("Linux x86_64");
    mockWindowFind();
  });

  it("defaultSessionType prop이 변경되면 내부 상태와 하위 컴포넌트가 동기화된다", async () => {
    // Given
    const baseProps = {
      initialTasks: createEmptyTasks(),
      initialDoneTotal: 0,
      initialDoneLimit: 20,
      sshHosts: [],
      projects: [createProject()],
      sidebarDefaultCollapsed: false,
      doneAlertDismissed: false,
      notificationSettings: { isEnabled: true, enabledStatuses: ["progress", "pending", "review"] },
      taskSearchShortcut: "Mod+Shift+O",
    };

    const { rerender } = render(<Board {...baseProps} defaultSessionType={SessionType.TMUX} />);

    // When
    rerender(<Board {...baseProps} defaultSessionType={SessionType.ZELLIJ} />);

    // Then
    await waitFor(() => {
      expect(screen.getByTestId("create-task-default-session").textContent).toBe(SessionType.ZELLIJ);
      expect(screen.getByTestId("project-settings-default-session").textContent).toBe(SessionType.ZELLIJ);
    });
  });

  it("ProjectSettings에서 변경 콜백을 호출하면 Board 상태가 즉시 반영된다", async () => {
    // Given
    render(
      <Board
        initialTasks={createEmptyTasks()}
        initialDoneTotal={0}
        initialDoneLimit={20}
        sshHosts={[]}
        projects={[createProject()]}
        sidebarDefaultCollapsed={false}
        doneAlertDismissed={false}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
      />,
    );

    // When
    fireEvent.click(screen.getByRole("button", { name: "change-default-session" }));

    // Then
    await waitFor(() => {
      expect(screen.getByTestId("create-task-default-session").textContent).toBe(SessionType.ZELLIJ);
    });
  });

  it("드래그 종료 시 reorder action을 이벤트 이후에 호출한다", async () => {
    render(
      <Board
        initialTasks={createTasksWithTodo()}
        initialDoneTotal={0}
        initialDoneLimit={20}
        sshHosts={[]}
        projects={[createProject()]}
        sidebarDefaultCollapsed={false}
        doneAlertDismissed={false}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "trigger-drag-end" }));

    await waitFor(() => {
      expect(reorderTasks).toHaveBeenCalledWith(TaskStatus.TODO, ["task-1"]);
    });
  });

  it("맥 데스크톱 앱에서는 보드 컨트롤을 타이틀바 한 줄에 배치한다", async () => {
    window.kanvibeDesktop = { isDesktop: true };
    mockNavigatorPlatform("MacIntel");

    const { container } = render(
      <Board
        initialTasks={createEmptyTasks()}
        initialDoneTotal={0}
        initialDoneLimit={20}
        sshHosts={[]}
        projects={[createProject()]}
        sidebarDefaultCollapsed={false}
        doneAlertDismissed={false}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
      />,
    );

    await waitFor(() => {
      const headerClassName = container.querySelector("header")?.className;
      expect(headerClassName).toContain("pt-10");
      expect(headerClassName).toContain("pl-20");
      expect(headerClassName).toContain("bg-bg-page");
      expect(headerClassName).not.toContain("h-10");
      expect(headerClassName).not.toContain("border-b");
    });
  });

  it("인증 UI가 없는 보드 상단에는 로그아웃 버튼을 표시하지 않는다", () => {
    render(
      <Board
        initialTasks={createEmptyTasks()}
        initialDoneTotal={0}
        initialDoneLimit={20}
        sshHosts={[]}
        projects={[createProject()]}
        sidebarDefaultCollapsed={false}
        doneAlertDismissed={false}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
      />,
    );

    expect(screen.queryByRole("button", { name: "logout" })).toBeNull();
  });

  it("리눅스 보드에서 Ctrl+F를 누르면 페이지 검색 바를 연다", async () => {
    render(
      <Board
        initialTasks={createEmptyTasks()}
        initialDoneTotal={0}
        initialDoneLimit={20}
        sshHosts={[]}
        projects={[createProject()]}
        sidebarDefaultCollapsed={false}
        doneAlertDismissed={false}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
      />,
    );

    fireEvent.keyDown(window, {
      key: "f",
      ctrlKey: true,
    });

    await waitFor(() => {
      expect(screen.getByPlaceholderText("pageFind.placeholder")).toBeTruthy();
    });
  });

  it("보드 검색 바에서 Enter와 Shift+Enter로 순방향/역방향 찾기를 호출한다", async () => {
    const findMock = mockWindowFind();

    render(
      <Board
        initialTasks={createEmptyTasks()}
        initialDoneTotal={0}
        initialDoneLimit={20}
        sshHosts={[]}
        projects={[createProject()]}
        sidebarDefaultCollapsed={false}
        doneAlertDismissed={false}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
      />,
    );

    fireEvent.keyDown(window, {
      key: "f",
      ctrlKey: true,
    });

    const input = await screen.findByPlaceholderText("pageFind.placeholder");
    fireEvent.change(input, {
      target: { value: "kanvibe" },
    });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });

    expect(findMock).toHaveBeenNthCalledWith(1, "kanvibe", false, false, true, false, false, false);
    expect(findMock).toHaveBeenNthCalledWith(2, "kanvibe", false, true, true, false, false, false);
  });

  it("보드 검색 바에서 Escape를 누르면 검색 UI를 닫는다", async () => {
    render(
      <Board
        initialTasks={createEmptyTasks()}
        initialDoneTotal={0}
        initialDoneLimit={20}
        sshHosts={[]}
        projects={[createProject()]}
        sidebarDefaultCollapsed={false}
        doneAlertDismissed={false}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
      />,
    );

    fireEvent.keyDown(window, {
      key: "f",
      ctrlKey: true,
    });

    const input = await screen.findByPlaceholderText("pageFind.placeholder");
    fireEvent.keyDown(input, { key: "Escape" });

    await waitFor(() => {
      expect(screen.queryByPlaceholderText("pageFind.placeholder")).toBeNull();
    });
  });

  it("PR merge batch 이벤트를 받으면 하나의 체크리스트 모달을 띄우고 체크된 task만 Done으로 이동한다", async () => {
    const listeners: Array<(event: unknown) => void> = [];
    window.kanvibeDesktop = {
      isDesktop: true,
      onBoardEvent: vi.fn((listener) => {
        listeners.push(listener);
        return () => {};
      }),
    } as never;
    vi.mocked(updateTaskStatus).mockResolvedValue(null);

    render(
      <Board
        initialTasks={createTasksWithTodo()}
        initialDoneTotal={0}
        initialDoneLimit={20}
        sshHosts={[]}
        projects={[createProject()]}
        sidebarDefaultCollapsed={false}
        doneAlertDismissed={false}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
      />,
    );

    await waitFor(() => {
      expect(listeners).toHaveLength(1);
    });

    await act(async () => {
      listeners[0]({
        type: "task-pr-merged-detected-batch",
        mergedPullRequests: [
          {
            taskId: "task-1",
            taskTitle: "Test Task",
            branchName: "feature/pr-sync",
            prUrl: "https://github.com/kanvibe/kanvibe/pull/210",
            mergedAt: "2026-04-30T02:00:00Z",
          },
          {
            taskId: "task-2",
            taskTitle: "Docs Task",
            branchName: "docs/pr-sync",
            prUrl: "https://github.com/kanvibe/kanvibe/pull/211",
            mergedAt: "2026-04-30T02:05:00Z",
          },
        ],
      });
    });

    expect(screen.getByText("https://github.com/kanvibe/kanvibe/pull/210")).toBeTruthy();
    expect(screen.getByText("https://github.com/kanvibe/kanvibe/pull/211")).toBeTruthy();

    await act(async () => {
      fireEvent.click(screen.getByRole("checkbox", { name: "Docs Task" }));
    });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "confirm" }));
    });

    await waitFor(() => {
      expect(updateTaskStatus).toHaveBeenCalledWith("task-1", TaskStatus.DONE);
    });
    expect(updateTaskStatus).toHaveBeenCalledTimes(1);
  });

  it("중앙 board command 요청이 오면 branch TODO 기본값으로 create modal을 연다", async () => {
    render(
      <BoardCommandProvider>
        <BoardCommandRequester />
        <Board
          initialTasks={createTasksWithTodo()}
          initialDoneTotal={0}
          initialDoneLimit={20}
          sshHosts={[]}
          projects={[createProject()]}
          sidebarDefaultCollapsed={false}
          doneAlertDismissed={false}
          notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
          defaultSessionType={SessionType.TMUX}
          taskSearchShortcut="Mod+Shift+O"
        />
      </BoardCommandProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "request branch todo" }));

    await waitFor(() => {
      expect(screen.getByTestId("create-task-default-project").textContent).toBe("project-1");
      expect(screen.getByTestId("create-task-default-base-branch").textContent).toBe("feat/from-search");
    });
  });
});
