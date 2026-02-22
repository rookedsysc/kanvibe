import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import Board from "../Board";
import { SessionType, TaskStatus } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import type { TasksByStatus } from "@/app/actions/kanban";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

vi.mock("@hello-pangea/dnd", () => ({
  DragDropContext: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

vi.mock("@/app/actions/kanban", () => ({
  reorderTasks: vi.fn(),
  deleteTask: vi.fn(),
  getMoreDoneTasks: vi.fn().mockResolvedValue({ tasks: [], doneTotal: 0 }),
  moveTaskToColumn: vi.fn(),
}));

vi.mock("@/app/actions/auth", () => ({
  logoutAction: vi.fn(),
}));

vi.mock("@/hooks/useAutoRefresh", () => ({
  useAutoRefresh: vi.fn(),
}));

vi.mock("@/hooks/useProjectFilterParams", () => ({
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

describe("Board defaultSessionType sync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
});
