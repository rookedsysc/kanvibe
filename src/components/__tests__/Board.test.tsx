import { forwardRef, useImperativeHandle } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { createEvent, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import Board from "../Board";
import { moveTaskToColumn, reorderTasks } from "@/desktop/renderer/actions/kanban";
import { SessionType, TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
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
  default: ({ status, tasks }: { status: TaskStatus; tasks: KanbanTask[] }) => (
    <div data-testid="column">
      {tasks.map((task, index) => (
        <a
          key={task.id}
          href={`/task/${task.id}`}
          data-kanban-task-card="true"
          data-kanban-task-id={task.id}
          data-kanban-status={status}
          data-kanban-index={index}
        >
          {task.title}
        </a>
      ))}
    </div>
  ),
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
  default: ({
    onStatusChange,
  }: {
    onStatusChange: (status: TaskStatus) => void;
  }) => (
    <div data-testid="task-context-menu">
      <button type="button" onClick={() => onStatusChange(TaskStatus.REVIEW)}>
        change-status-review
      </button>
    </div>
  ),
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
    });
  });

  it("설정 버튼을 누르면 현재 locale의 settings 페이지로 이동한다", () => {
    // Given
    window.location.hash = "#/ko";

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
    fireEvent.click(screen.getByRole("button", { name: "settings" }));

    // Then
    expect(window.location.hash).toBe("#/ko/settings");
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

  it("task focus가 없을 때 방향키를 누르면 페이지 스크롤 대신 첫 task로 focus를 진입시킨다", async () => {
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

    const taskLink = await screen.findByRole("link", { name: "Test Task" });
    taskLink.blur();

    const event = new KeyboardEvent("keydown", {
      key: "ArrowDown",
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(taskLink);
  });

  it("포커스된 task에서 Shift+Enter를 누르면 상세 페이지를 새 창에서 연다", async () => {
    window.location.hash = "#/en";
    const openWindow = vi.spyOn(window, "open").mockImplementation(() => null);

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

    const taskLink = await screen.findByRole("link", { name: "Test Task" });
    taskLink.focus();

    const event = createEvent.keyDown(taskLink, {
      key: "Enter",
      shiftKey: true,
    });
    fireEvent(taskLink, event);

    expect(event.defaultPrevented).toBe(true);
    expect(openWindow).toHaveBeenCalledWith(`${window.location.origin}/#/en/task/task-1`, "_blank", "noopener,noreferrer");
    expect(screen.queryByTestId("task-context-menu")).toBeNull();

    openWindow.mockRestore();
  });

  it("포커스된 task에서 Shift+F10을 누르면 컨텍스트 메뉴를 연다", async () => {
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

    const taskLink = await screen.findByRole("link", { name: "Test Task" });
    taskLink.focus();

    const event = createEvent.keyDown(taskLink, {
      key: "F10",
      shiftKey: true,
    });
    fireEvent(taskLink, event);

    expect(event.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect(screen.getByTestId("task-context-menu")).toBeTruthy();
    });
  });

  it("컨텍스트 메뉴에서 상태를 선택하면 대상 컬럼 마지막으로 task를 이동한다", async () => {
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

    const taskLink = await screen.findByRole("link", { name: "Test Task" });
    taskLink.focus();

    fireEvent.keyDown(taskLink, {
      key: "F10",
      shiftKey: true,
    });

    fireEvent.click(await screen.findByRole("button", { name: "change-status-review" }));

    await waitFor(() => {
      expect(moveTaskToColumn).toHaveBeenCalledWith("task-1", TaskStatus.REVIEW, ["task-1"]);
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

  it("background sync review activation은 전역 dialog host가 처리하므로 Board에서 직접 소비하지 않는다", async () => {
    const consumePendingNotificationActivation = vi.fn().mockResolvedValue({
      id: "n-review",
      title: "Background sync review",
      body: "Review pending items",
      taskId: null,
      relativePath: "/ko",
      locale: "ko",
      isRead: false,
      createdAt: "2026-05-01T00:00:00.000Z",
      dedupeKey: "background-review-1",
      action: {
        type: "background-sync-review",
        payload: {
          mergedPullRequests: [
            {
              taskId: "task-1",
              taskTitle: "Test Task",
              branchName: "feature/pr-sync",
              prUrl: "https://github.com/kanvibe/kanvibe/pull/210",
              mergedAt: "2026-04-30T02:00:00Z",
            },
          ],
          registeredWorktrees: [],
        },
      },
    });
    const onNotificationActivated = vi.fn(() => () => {});
    window.kanvibeDesktop = {
      isDesktop: true,
      consumePendingNotificationActivation,
      onNotificationActivated,
    } as never;

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
      expect(screen.getAllByTestId("column")).toHaveLength(5);
    });

    expect(consumePendingNotificationActivation).not.toHaveBeenCalled();
    expect(onNotificationActivated).not.toHaveBeenCalled();
    expect(screen.queryByText("https://github.com/kanvibe/kanvibe/pull/210")).toBeNull();
  });

  it("중앙 board command 요청이 오면 branch TODO 기본값으로 create modal을 연다", async () => {
    render(
      <MemoryRouter initialEntries={["/ko"]}>
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
        </BoardCommandProvider>
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole("button", { name: "request branch todo" }));

    await waitFor(() => {
      expect(screen.getByTestId("create-task-default-project").textContent).toBe("project-1");
      expect(screen.getByTestId("create-task-default-base-branch").textContent).toBe("feat/from-search");
    });
  });
});
