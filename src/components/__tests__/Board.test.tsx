import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import Board from "../Board";
import { reorderTasks } from "@/desktop/renderer/actions/kanban";
import { SessionType, TaskStatus } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import type { TasksByStatus } from "@/desktop/renderer/actions/kanban";

function mockNavigatorPlatform(platform: string) {
  Object.defineProperty(window.navigator, "platform", {
    configurable: true,
    value: platform,
  });
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
  deleteTask: vi.fn(),
  getMoreDoneTasks: vi.fn().mockResolvedValue({ tasks: [], doneTotal: 0 }),
  moveTaskToColumn: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/auth", () => ({
  logoutAction: vi.fn(),
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
  default: () => <div data-testid="project-selector" />,
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
  default: ({ defaultSessionType }: { defaultSessionType: SessionType }) => (
    <div data-testid="create-task-default-session">{defaultSessionType}</div>
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

describe("Board defaultSessionType sync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    delete window.kanvibeDesktop;
    mockNavigatorPlatform("Linux x86_64");
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
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "trigger-drag-end" }));

    await waitFor(() => {
      expect(reorderTasks).toHaveBeenCalledWith(TaskStatus.TODO, ["task-1"]);
    });
  });

  it("맥 데스크톱 앱에서는 헤더 상단에 추가 여백을 준다", async () => {
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
      />,
    );

    await waitFor(() => {
      expect(container.querySelector("header")?.className).toContain("pt-10");
    });
  });
});
