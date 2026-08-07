import { forwardRef, useEffect, useImperativeHandle } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { createEvent, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import Board from "../Board";
import { deleteTask, moveTaskToColumn, reorderTasks } from "@/desktop/renderer/actions/kanban";
import { runBackgroundTaskSyncNow } from "@/desktop/renderer/actions/backgroundTaskSync";
import { useTaskKindFilterParams } from "@/desktop/renderer/hooks/useTaskKindFilterParams";
import { useBoardSortPreference } from "@/desktop/renderer/hooks/useBoardSortPreference";
import { TaskPriority } from "@/entities/TaskPriority";
import type { BoardSortPreference } from "@/desktop/shared/boardSort";
import { SessionType, TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import type { TasksByStatus } from "@/desktop/renderer/actions/kanban";
import {
  BoardCommandProvider,
  useBoardCommands,
  useHasBoardShortcutBlocker,
} from "@/desktop/renderer/components/BoardCommandProvider";

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
  useLocale: () => "ko",
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
      <button
        type="button"
        onClick={() =>
          onDragEnd?.({
            draggableId: "task-1",
            source: { droppableId: TaskStatus.TODO, index: 0 },
            destination: { droppableId: TaskStatus.REVIEW, index: 0 },
          })
        }
      >
        trigger-cross-column-drag-end
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

vi.mock("@/desktop/renderer/actions/backgroundTaskSync", () => ({
  runBackgroundTaskSyncNow: vi.fn().mockResolvedValue({ reviewNeeded: false, boardUpdated: false }),
}));

vi.mock("@/desktop/renderer/hooks/useAutoRefresh", () => ({
  useAutoRefresh: vi.fn(),
}));

vi.mock("@/desktop/renderer/hooks/useProjectFilterParams", () => ({
  useProjectFilterParams: vi.fn().mockReturnValue([[], vi.fn()]),
}));

vi.mock("@/desktop/renderer/hooks/useTaskKindFilterParams", () => ({
  TASK_KIND_FILTER_VALUES: ["project", "task", "all"],
  useTaskKindFilterParams: vi.fn().mockReturnValue(["all", vi.fn()]),
}));

vi.mock("@/desktop/renderer/hooks/useBoardSortPreference", () => ({
  useBoardSortPreference: vi.fn().mockReturnValue([{ keys: [], mode: "sort-first" }, vi.fn()]),
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
    onDelete,
  }: {
    onStatusChange: (status: TaskStatus) => void;
    onDelete: () => void;
  }) => (
    <div data-testid="task-context-menu">
      <button type="button" onClick={() => onStatusChange(TaskStatus.REVIEW)}>
        change-status-review
      </button>
      <button type="button" onClick={onDelete}>
        delete-task
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
    isOpen,
    defaultSessionType,
    defaultProjectId,
    defaultBaseBranch,
  }: {
    isOpen: boolean;
    defaultSessionType: SessionType;
    defaultProjectId?: string;
    defaultBaseBranch?: string;
  }) => isOpen ? (
    <div>
      <div data-testid="create-task-modal" />
      <div data-testid="create-task-default-session">{defaultSessionType}</div>
      <div data-testid="create-task-default-project">{defaultProjectId ?? ""}</div>
      <div data-testid="create-task-default-base-branch">{defaultBaseBranch ?? ""}</div>
    </div>
  ) : null,
}));

function createProject(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-1",
    name: "kanvibe",
    repoPath: "/repo/kanvibe",
    defaultBranch: "main",
    sshHost: null,
    isWorktree: false,
    color: null,
    iconDataUrl: null,
    createdAt: new Date(),
    ...overrides,
  };
}

function createTask(overrides: Partial<KanbanTask> & Pick<KanbanTask, "id" | "title" | "status">): KanbanTask {
  return {
    description: null,
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
    displayRank: "8",
    createdAt: new Date(),
    updatedAt: new Date(),
    ...overrides,
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
        displayRank: "8",
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

function createTasksWithTodoAndProgress(): TasksByStatus {
  return {
    [TaskStatus.TODO]: [
      {
        id: "task-1",
        title: "Todo Task",
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
        displayRank: "8",
        createdAt: new Date(),
        updatedAt: new Date(),
      },
    ],
    [TaskStatus.PROGRESS]: [
      {
        id: "task-2",
        title: "Progress Task",
        description: null,
        status: TaskStatus.PROGRESS,
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
        displayRank: "8",
        createdAt: new Date(),
        updatedAt: new Date(),
      },
    ],
    [TaskStatus.PENDING]: [],
    [TaskStatus.REVIEW]: [],
    [TaskStatus.DONE]: [],
  };
}

function createTaskKindFilterTasks(): TasksByStatus {
  return {
    [TaskStatus.TODO]: [
      createTask({
        id: "project-root-task",
        title: "Root Project Task",
        status: TaskStatus.TODO,
        branchName: "main",
        baseBranch: "main",
      }),
      createTask({
        id: "branch-worktree-task",
        title: "Branch Worktree Task",
        status: TaskStatus.TODO,
        branchName: "feat/filter-ui",
        baseBranch: "main",
        worktreePath: "/repo/kanvibe__worktrees/feat-filter-ui",
      }),
      createTask({
        id: "plain-task",
        title: "Plain Task",
        status: TaskStatus.TODO,
      }),
    ],
    [TaskStatus.PROGRESS]: [],
    [TaskStatus.PENDING]: [],
    [TaskStatus.REVIEW]: [],
    [TaskStatus.DONE]: [],
  };
}

function renderBoardForTaskKindFilter() {
  return render(
    <Board
      initialTasks={createTaskKindFilterTasks()}
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
}

function expectTaskKindFilterVisibleTasks(names: string[]) {
  for (const name of ["Root Project Task", "Branch Worktree Task", "Plain Task"]) {
    const assertion = expect(screen.queryByRole("link", { name }));
    if (names.includes(name)) {
      assertion.toBeTruthy();
    } else {
      assertion.toBeNull();
    }
  }
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

function BoardShortcutBlocker() {
  const boardCommands = useBoardCommands();
  const hasShortcutBlocker = useHasBoardShortcutBlocker();

  useEffect(() => boardCommands.registerShortcutBlocker(), [boardCommands]);

  return (
    <span data-testid="shortcut-blocker-state">
      {hasShortcutBlocker ? "blocked" : "open"}
    </span>
  );
}

describe("Board defaultSessionType sync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useTaskKindFilterParams).mockReturnValue(["all", vi.fn()] as const);
    vi.mocked(useBoardSortPreference).mockReturnValue([{ keys: [], mode: "sort-first" }, vi.fn()] as const);
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
    fireEvent.click(screen.getByRole("button", { name: "newTask" }));

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

  it("프로젝트/태스크 필터를 전체 프로젝트 필터 왼쪽에 같은 높이/포인트 컬러로 렌더링한다", () => {
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

    const projectSelectorContainer = screen.getByTestId("project-filter-control");
    const taskKindFilter = screen.getByTestId("task-kind-filter");
    const scanButton = screen.getByRole("button", { name: "scanTitle" });
    const newTaskButton = screen.getByRole("button", { name: "newTask" });
    const toolbarItems = Array.from(scanButton.parentElement?.children ?? []);
    const allFilterButton = screen.getByRole("button", { name: "taskKindFilter.options.all" });

    expect(taskKindFilter.getAttribute("aria-label")).toBe("taskKindFilter.label");
    expect(taskKindFilter.className).toContain("h-[34px]");
    expect(taskKindFilter.className).toContain("w-[180px]");
    expect(allFilterButton.className).toContain("bg-brand-primary");
    expect(allFilterButton.className).toContain("text-text-inverse");
    expect(projectSelectorContainer.className).toContain("w-64");
    expect(scanButton.getAttribute("aria-label")).toBe("scanTitle");
    expect(scanButton.textContent).toBe("");
    expect(toolbarItems.indexOf(taskKindFilter)).toBeLessThan(toolbarItems.indexOf(projectSelectorContainer));
    expect(toolbarItems.indexOf(projectSelectorContainer)).toBeLessThan(toolbarItems.indexOf(scanButton));
    expect(toolbarItems.indexOf(scanButton)).toBeLessThan(toolbarItems.indexOf(newTaskButton));

    fireEvent.click(scanButton);

    expect(screen.getByRole("dialog", { name: "scanTitle" })).toBeTruthy();
  });

  it("task kind filter 버튼을 프로젝트 필터 옆에 렌더링하고 선택 변경을 요청한다", () => {
    const setTaskKindFilter = vi.fn();
    vi.mocked(useTaskKindFilterParams).mockReturnValue(["all", setTaskKindFilter] as const);

    renderBoardForTaskKindFilter();

    expect(screen.getByRole("group", { name: "taskKindFilter.label" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "taskKindFilter.options.all" }).getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(screen.getByRole("button", { name: "taskKindFilter.options.project" }));

    expect(setTaskKindFilter).toHaveBeenCalledWith("project");
  });

  it("Project 필터 선택 시 기본 브랜치 root task만 표시한다", () => {
    vi.mocked(useTaskKindFilterParams).mockReturnValue(["project", vi.fn()] as const);

    renderBoardForTaskKindFilter();

    expectTaskKindFilterVisibleTasks(["Root Project Task"]);
  });

  it("Task 필터 선택 시 project root를 제외한 작업만 표시한다", () => {
    vi.mocked(useTaskKindFilterParams).mockReturnValue(["task", vi.fn()] as const);

    renderBoardForTaskKindFilter();

    expectTaskKindFilterVisibleTasks(["Branch Worktree Task", "Plain Task"]);
  });

  it("All 필터 선택 시 project root와 일반 작업을 모두 표시한다", () => {
    vi.mocked(useTaskKindFilterParams).mockReturnValue(["all", vi.fn()] as const);

    renderBoardForTaskKindFilter();

    expectTaskKindFilterVisibleTasks(["Root Project Task", "Branch Worktree Task", "Plain Task"]);
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
      expect(reorderTasks).toHaveBeenCalledWith(TaskStatus.TODO, "task-1", ["task-1"]);
    });
  });

  it("드래그로 다른 컬럼에 놓으면 목적지에서의 자리까지 함께 저장한다", async () => {
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

    fireEvent.click(screen.getByRole("button", { name: "trigger-cross-column-drag-end" }));

    await waitFor(() => {
      /** 목적지 컬럼에서 사용자가 놓은 자리가 반영된 순서가 그대로 넘어가야 rank를 그 자리로 계산한다 */
      expect(moveTaskToColumn).toHaveBeenCalledWith("task-1", TaskStatus.REVIEW, ["task-1"]);
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

  it("보드에서 /를 누르면 Ctrl+F와 같은 페이지 검색 바를 열고 Enter로 결과를 이동한다", async () => {
    const findMock = mockWindowFind();

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

    const openFindEvent = createEvent.keyDown(taskLink, { key: "/" });
    fireEvent(taskLink, openFindEvent);

    expect(openFindEvent.defaultPrevented).toBe(true);

    const input = await screen.findByPlaceholderText("pageFind.placeholder");
    fireEvent.change(input, {
      target: { value: "Test Task" },
    });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });

    expect(findMock).toHaveBeenNthCalledWith(1, "Test Task", false, false, true, false, false, false);
    expect(findMock).toHaveBeenNthCalledWith(2, "Test Task", false, true, true, false, false, false);
  });

  it("vim mode가 꺼져 있으면 /를 페이지 검색 단축키로 소비하지 않는다", async () => {
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
        vimModeEnabled={false}
      />,
    );

    const taskLink = await screen.findByRole("link", { name: "Test Task" });
    taskLink.focus();

    const openFindEvent = createEvent.keyDown(taskLink, { key: "/" });
    fireEvent(taskLink, openFindEvent);

    expect(openFindEvent.defaultPrevented).toBe(false);
    expect(screen.queryByPlaceholderText("pageFind.placeholder")).toBeNull();
  });

  it("shortcut blocker가 등록되어 있으면 Ctrl+F가 보드 검색 바를 열지 않는다", async () => {
    render(
      <MemoryRouter initialEntries={["/ko"]}>
        <BoardCommandProvider>
          <BoardShortcutBlocker />
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
          />
        </BoardCommandProvider>
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByTestId("shortcut-blocker-state").textContent).toBe("blocked");
    });

    fireEvent.keyDown(window, {
      key: "f",
      ctrlKey: true,
    });

    expect(screen.queryByPlaceholderText("pageFind.placeholder")).toBeNull();
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

  it("task focus가 없을 때 vim 이동 키를 누르면 첫 task로 focus를 진입시킨다", async () => {
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
      key: "j",
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(taskLink);
  });

  it("vim mode가 꺼져 있으면 task focus가 없을 때 vim 이동 키를 소비하지 않는다", async () => {
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
        vimModeEnabled={false}
      />,
    );

    const taskLink = await screen.findByRole("link", { name: "Test Task" });
    taskLink.blur();

    const event = new KeyboardEvent("keydown", {
      key: "j",
      bubbles: true,
      cancelable: true,
    });
    window.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
    expect(document.activeElement).not.toBe(taskLink);
  });

  it("포커스된 task에서 dd를 누르면 삭제 성공 후 task를 보드에서 제거한다", async () => {
    const confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
    vi.mocked(deleteTask).mockResolvedValueOnce(true);

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

    const firstD = createEvent.keyDown(taskLink, { key: "d" });
    fireEvent(taskLink, firstD);

    expect(firstD.defaultPrevented).toBe(true);
    expect(deleteTask).not.toHaveBeenCalled();

    const secondD = createEvent.keyDown(taskLink, { key: "d" });
    fireEvent(taskLink, secondD);

    expect(secondD.defaultPrevented).toBe(true);
    expect(confirmMock).toHaveBeenCalledWith("deleteConfirm");
    expect(deleteTask).toHaveBeenCalledWith("task-1");
    await waitFor(() => {
      expect(screen.queryByRole("link", { name: "Test Task" })).toBeNull();
    });

    confirmMock.mockRestore();
  });

  it("포커스된 task에서 dd 삭제가 완료되기 전에는 task를 숨기지 않는다", async () => {
    const confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
    let resolveDelete: (value: boolean) => void = () => {};
    vi.mocked(deleteTask).mockReturnValueOnce(new Promise((resolve) => {
      resolveDelete = resolve;
    }));

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
    fireEvent.keyDown(taskLink, { key: "d" });
    fireEvent.keyDown(taskLink, { key: "d" });

    expect(confirmMock).toHaveBeenCalledWith("deleteConfirm");
    expect(deleteTask).toHaveBeenCalledWith("task-1");
    expect(screen.getByRole("link", { name: "Test Task" })).toBeTruthy();

    resolveDelete(true);

    await waitFor(() => {
      expect(screen.queryByRole("link", { name: "Test Task" })).toBeNull();
    });

    confirmMock.mockRestore();
  });

  it("포커스된 task에서 dd 삭제가 실패하면 task를 보드에 유지한다", async () => {
    const confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
    const consoleErrorMock = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.mocked(deleteTask).mockRejectedValueOnce(new Error("cleanup failed"));

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
    fireEvent.keyDown(taskLink, { key: "d" });
    fireEvent.keyDown(taskLink, { key: "d" });

    await waitFor(() => {
      expect(consoleErrorMock).toHaveBeenCalledWith("Failed to delete task", expect.any(Error));
    });
    expect(screen.getByRole("link", { name: "Test Task" })).toBeTruthy();

    confirmMock.mockRestore();
    consoleErrorMock.mockRestore();
  });

  it("포커스된 task에서 dd를 누르는 중 보드 task 목록이 refresh되어도 삭제 sequence를 유지한다", async () => {
    const confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
    vi.mocked(deleteTask).mockResolvedValueOnce(true);

    const { rerender } = render(
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
    fireEvent(taskLink, createEvent.keyDown(taskLink, { key: "d" }));

    rerender(
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

    const refreshedTaskLink = await screen.findByRole("link", { name: "Test Task" });
    refreshedTaskLink.focus();
    const secondD = createEvent.keyDown(refreshedTaskLink, { key: "d" });
    fireEvent(refreshedTaskLink, secondD);

    expect(secondD.defaultPrevented).toBe(true);
    expect(confirmMock).toHaveBeenCalledWith("deleteConfirm");
    expect(deleteTask).toHaveBeenCalledWith("task-1");
    await waitFor(() => {
      expect(screen.queryByRole("link", { name: "Test Task" })).toBeNull();
    });

    confirmMock.mockRestore();
  });

  it("context menu 삭제도 deleteTask가 완료되기 전에는 task를 숨기지 않는다", async () => {
    const confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);
    let resolveDelete: (value: boolean) => void = () => {};
    vi.mocked(deleteTask).mockReturnValueOnce(new Promise((resolve) => {
      resolveDelete = resolve;
    }));

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
    fireEvent.keyDown(taskLink, { key: "F10", shiftKey: true });

    fireEvent.click(await screen.findByRole("button", { name: "delete-task" }));

    expect(confirmMock).toHaveBeenCalledWith("deleteConfirm");
    expect(deleteTask).toHaveBeenCalledWith("task-1");
    expect(screen.getByRole("link", { name: "Test Task" })).toBeTruthy();

    resolveDelete(true);

    await waitFor(() => {
      expect(screen.queryByRole("link", { name: "Test Task" })).toBeNull();
    });

    confirmMock.mockRestore();
  });

  it("포커스된 task에서 dd를 눌러도 확인을 취소하면 task를 삭제하지 않는다", async () => {
    const confirmMock = vi.spyOn(window, "confirm").mockReturnValue(false);

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

    fireEvent.keyDown(taskLink, { key: "d" });
    fireEvent.keyDown(taskLink, { key: "d" });

    expect(confirmMock).toHaveBeenCalledWith("deleteConfirm");
    expect(deleteTask).not.toHaveBeenCalled();

    confirmMock.mockRestore();
  });

  it("vim mode가 꺼져 있으면 포커스된 task에서 dd를 소비하지 않고 삭제하지 않는다", async () => {
    const confirmMock = vi.spyOn(window, "confirm").mockReturnValue(true);

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
        vimModeEnabled={false}
      />,
    );

    const taskLink = await screen.findByRole("link", { name: "Test Task" });
    taskLink.focus();

    const firstD = createEvent.keyDown(taskLink, { key: "d" });
    fireEvent(taskLink, firstD);
    const secondD = createEvent.keyDown(taskLink, { key: "d" });
    fireEvent(taskLink, secondD);

    expect(firstD.defaultPrevented).toBe(false);
    expect(secondD.defaultPrevented).toBe(false);
    expect(confirmMock).not.toHaveBeenCalled();
    expect(deleteTask).not.toHaveBeenCalled();

    confirmMock.mockRestore();
  });

  it("vim mode에서 n을 누르면 새 작업 모달을 즉시 연다", async () => {
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

    expect(screen.queryByTestId("create-task-modal")).toBeNull();

    const event = createEvent.keyDown(window, { key: "n" });
    fireEvent(window, event);

    expect(event.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect(screen.getByTestId("create-task-modal")).toBeTruthy();
    });
  });

  it(":move <status> 명령으로 포커스된 task를 대상 컬럼으로 이동한다", async () => {
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

    const openCommandEvent = createEvent.keyDown(taskLink, { key: ":" });
    fireEvent(taskLink, openCommandEvent);

    expect(openCommandEvent.defaultPrevented).toBe(true);

    const commandInput = await screen.findByRole("textbox", { name: "vimCommand.label" });
    fireEvent.change(commandInput, { target: { value: "move review" } });
    fireEvent.keyDown(commandInput, { key: "Enter" });

    await waitFor(() => {
      expect(moveTaskToColumn).toHaveBeenCalledWith("task-1", TaskStatus.REVIEW, ["task-1"]);
    });
  });

  it(":sync 명령으로 background task sync를 백그라운드 실행한다", async () => {
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

    const openCommandEvent = createEvent.keyDown(window, { key: ":" });
    fireEvent(window, openCommandEvent);

    expect(openCommandEvent.defaultPrevented).toBe(true);

    const commandInput = await screen.findByRole("textbox", { name: "vimCommand.label" });
    fireEvent.change(commandInput, { target: { value: "sync" } });
    fireEvent.keyDown(commandInput, { key: "Enter" });

    expect(runBackgroundTaskSyncNow).toHaveBeenCalledTimes(1);
    expect(moveTaskToColumn).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(screen.queryByRole("textbox", { name: "vimCommand.label" })).toBeNull();
    });
  });

  it(":sync 명령은 Tab 자동 완성으로 입력할 수 있다", async () => {
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

    fireEvent.keyDown(window, { key: ":" });

    const commandInput = await screen.findByRole("textbox", { name: "vimCommand.label" });
    fireEvent.change(commandInput, { target: { value: "s" } });

    const autocompleteEvent = createEvent.keyDown(commandInput, { key: "Tab" });
    fireEvent(commandInput, autocompleteEvent);

    expect(autocompleteEvent.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect((commandInput as HTMLInputElement).value).toBe("sync");
    });
  });

  it(":move 명령에서 Tab으로 상태명을 자동 완성한다", async () => {
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

    fireEvent.keyDown(taskLink, { key: ":" });

    const commandInput = await screen.findByRole("textbox", { name: "vimCommand.label" });
    fireEvent.change(commandInput, { target: { value: "move re" } });

    const autocompleteEvent = createEvent.keyDown(commandInput, { key: "Tab" });
    fireEvent(commandInput, autocompleteEvent);

    expect(autocompleteEvent.defaultPrevented).toBe(true);
    await waitFor(() => {
      expect((commandInput as HTMLInputElement).value).toBe("move review");
    });

    fireEvent.keyDown(commandInput, { key: "Enter" });

    await waitFor(() => {
      expect(moveTaskToColumn).toHaveBeenCalledWith("task-1", TaskStatus.REVIEW, ["task-1"]);
    });
  });

  it(":move 명령 자동 완성 후보가 모호하면 Tab을 소비하지 않는다", async () => {
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

    fireEvent.keyDown(taskLink, { key: ":" });

    const commandInput = await screen.findByRole("textbox", { name: "vimCommand.label" });
    fireEvent.change(commandInput, { target: { value: "move p" } });

    const autocompleteEvent = createEvent.keyDown(commandInput, { key: "Tab" });
    fireEvent(commandInput, autocompleteEvent);

    expect(autocompleteEvent.defaultPrevented).toBe(false);
    expect((commandInput as HTMLInputElement).value).toBe("move p");
  });

  it("상세 화면에서 돌아온 task id가 있으면 해당 task로 초기 focus를 시작한다", async () => {
    render(
      <Board
        initialTasks={createTasksWithTodoAndProgress()}
        initialDoneTotal={0}
        initialDoneLimit={20}
        sshHosts={[]}
        projects={[createProject()]}
        sidebarDefaultCollapsed={false}
        doneAlertDismissed={false}
        notificationSettings={{ isEnabled: true, enabledStatuses: ["progress", "pending", "review"] }}
        defaultSessionType={SessionType.TMUX}
        taskSearchShortcut="Mod+Shift+O"
        initialFocusTaskId="task-2"
      />,
    );

    const progressTaskLink = await screen.findByRole("link", { name: "Progress Task" });

    await waitFor(() => {
      expect(document.activeElement).toBe(progressTaskLink);
    });
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
  describe("정렬 순서 필터", () => {
    function renderBoardWithSortPreference(preference: BoardSortPreference, tasks: TasksByStatus) {
      vi.mocked(useBoardSortPreference).mockReturnValue([preference, vi.fn()] as const);

      render(
        <MemoryRouter initialEntries={["/ko"]}>
          <BoardCommandProvider>
            <Board
              initialTasks={tasks}
              initialDoneTotal={0}
              initialDoneLimit={20}
              sshHosts={[]}
              projects={[createProject()]}
              sidebarDefaultCollapsed={false}
              doneAlertDismissed={false}
              notificationSettings={{ isEnabled: true, enabledStatuses: [] }}
              defaultSessionType={SessionType.TMUX}
              taskSearchShortcut="Mod+Shift+O"
            />
          </BoardCommandProvider>
        </MemoryRouter>,
      );
    }

    function createPrioritizedTodoTasks(): TasksByStatus {
      return {
        ...createEmptyTasks(),
        [TaskStatus.TODO]: [
          createTask({ id: "low-task", title: "Low", status: TaskStatus.TODO, priority: TaskPriority.LOW }),
          createTask({ id: "unset-task", title: "Unset", status: TaskStatus.TODO }),
          createTask({ id: "high-task", title: "High", status: TaskStatus.TODO, priority: TaskPriority.HIGH }),
        ],
      };
    }

    function readTodoCardIds() {
      return Array.from(screen.getAllByTestId("column")[0].querySelectorAll("[data-kanban-task-id]"))
        .map((card) => (card as HTMLElement).dataset.kanbanTaskId);
    }

    it("우선순위 정렬을 켜면 카드가 그 순서로 늘어선다", async () => {
      // Given / When
      renderBoardWithSortPreference(
        { keys: [{ field: "priority", direction: "asc" }], mode: "sort-first" },
        createPrioritizedTodoTasks(),
      );

      // Then
      await waitFor(() => expect(screen.getAllByTestId("column")).toHaveLength(5));
      /** 우선순위가 없는 카드는 방향과 무관하게 뒤로 간다 */
      expect(readTodoCardIds()).toEqual(["high-task", "low-task", "unset-task"]);
    });

    it("정렬 기준이 없으면 저장된 수동 순서를 그대로 보여준다", async () => {
      // Given / When
      renderBoardWithSortPreference({ keys: [], mode: "sort-first" }, createPrioritizedTodoTasks());

      // Then
      await waitFor(() => expect(screen.getAllByTestId("column")).toHaveLength(5));
      expect(readTodoCardIds()).toEqual(["low-task", "unset-task", "high-task"]);
    });

    it("프로젝트 root task의 우선순위를 물려받은 카드도 그 값으로 정렬된다", async () => {
      // Given
      const tasks: TasksByStatus = {
        ...createEmptyTasks(),
        [TaskStatus.TODO]: [
          createTask({
            id: "medium-task",
            title: "Medium",
            status: TaskStatus.TODO,
            priority: TaskPriority.MEDIUM,
            createdAt: new Date("2026-01-01T00:00:00Z"),
          }),
          createTask({
            id: "inheriting-task",
            title: "Inheriting",
            status: TaskStatus.TODO,
            createdAt: new Date("2026-01-03T00:00:00Z"),
          }),
          createTask({
            id: "root-task",
            title: "Root",
            status: TaskStatus.TODO,
            branchName: "main",
            priority: TaskPriority.HIGH,
            createdAt: new Date("2026-01-02T00:00:00Z"),
          }),
        ],
      };

      // When
      renderBoardWithSortPreference(
        { keys: [{ field: "priority", direction: "asc" }], mode: "sort-first" },
        tasks,
      );

      // Then
      await waitFor(() => expect(screen.getAllByTestId("column")).toHaveLength(5));
      expect(readTodoCardIds()).toEqual(["root-task", "inheriting-task", "medium-task"]);
    });

    it("rank 우선 모드에서는 정렬 기준이 있어도 드래그해 만든 순서를 지킨다", async () => {
      // Given
      const tasks: TasksByStatus = {
        ...createEmptyTasks(),
        [TaskStatus.TODO]: [
          createTask({ id: "low-task", title: "Low", status: TaskStatus.TODO, priority: TaskPriority.LOW, displayRank: "2" }),
          createTask({ id: "high-task", title: "High", status: TaskStatus.TODO, priority: TaskPriority.HIGH, displayRank: "6" }),
        ],
      };

      // When
      renderBoardWithSortPreference(
        { keys: [{ field: "priority", direction: "asc" }], mode: "rank-first" },
        tasks,
      );

      // Then
      await waitFor(() => expect(screen.getAllByTestId("column")).toHaveLength(5));
      /** 우선순위가 높은 카드라도 rank가 정한 자리를 넘어서지 못한다 */
      expect(readTodoCardIds()).toEqual(["low-task", "high-task"]);
    });

    it("정렬 기준이 켜져 있어도 같은 컬럼 안 재정렬을 저장한다", async () => {
      // Given
      renderBoardWithSortPreference(
        { keys: [{ field: "priority", direction: "asc" }], mode: "sort-first" },
        createTasksWithTodo(),
      );
      await waitFor(() => expect(screen.getAllByTestId("column")).toHaveLength(5));

      // When
      fireEvent.click(screen.getByRole("button", { name: "trigger-drag-end" }));

      // Then
      /** 드래그한 자리는 rank로 남고, 정렬 기준은 그 위에서 다시 적용된다 */
      expect(reorderTasks).toHaveBeenCalled();
    });
  });
});
