import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import TaskWorkspaceRoute from "@/desktop/renderer/routes/TaskWorkspaceRoute";
import { TaskStatus } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({
  getTasksByStatus: vi.fn(),
  getMoreDoneTasks: vi.fn(),
  getAllProjects: vi.fn(),
  getDefaultSessionType: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useLocale: () => "en",
  useTranslations: (namespace: string) => (key: string) => `${namespace}.${key}`,
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  getTasksByStatus: (...args: unknown[]) => mocks.getTasksByStatus(...args),
  getMoreDoneTasks: (...args: unknown[]) => mocks.getMoreDoneTasks(...args),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  getAllProjects: (...args: unknown[]) => mocks.getAllProjects(...args),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getDefaultSessionType: (...args: unknown[]) => mocks.getDefaultSessionType(...args),
}));

vi.mock("@/desktop/renderer/hooks/useBoardSortPreference", () => ({
  useBoardSortPreference: () => [{ keys: [] }, vi.fn()],
}));

vi.mock("@/desktop/renderer/hooks/useTerminalTabs", () => ({
  useTerminalTabs: () => ({ tabs: [] }),
}));

vi.mock("@/desktop/renderer/components/TerminalLoader", () => ({
  default: ({ taskId }: { taskId: string }) => <div data-testid="workspace-terminal" data-task-id={taskId} />,
}));

vi.mock("@/components/AutoConnectTerminalSession", () => ({
  default: ({ taskId, defaultSessionType }: { taskId: string; defaultSessionType: string }) => (
    <div data-testid="workspace-auto-connect" data-task-id={taskId} data-session-type={defaultSessionType} />
  ),
}));

function createTask(id: string, title: string, status: TaskStatus, sessionName: string | null) {
  return {
    id,
    title,
    status,
    description: null,
    branchName: `feat/${id}`,
    baseBranch: "main",
    worktreePath: null,
    sessionType: sessionName ? "tmux" : null,
    sessionName,
    sshHost: null,
    projectId: "project-api",
    priority: null,
    updatedAt: "2026-11-01T00:00:00Z",
    createdAt: "2026-11-01T00:00:00Z",
  };
}

function renderRoute() {
  render(
    <MemoryRouter initialEntries={["/ko"]}>
      <TaskWorkspaceRoute />
    </MemoryRouter>,
  );
}

describe("TaskWorkspaceRoute", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    sessionStorage.clear();
    mocks.getTasksByStatus.mockResolvedValue({
      tasks: {
        [TaskStatus.TODO]: [createTask("plan", "Plan sprint", TaskStatus.TODO, null)],
        [TaskStatus.PROGRESS]: [createTask("login", "Login page", TaskStatus.PROGRESS, "api-login")],
        [TaskStatus.PENDING]: [],
        [TaskStatus.REVIEW]: [createTask("billing", "Billing export", TaskStatus.REVIEW, "api-billing")],
        [TaskStatus.DONE]: [],
      },
      doneTotal: 0,
      doneLimit: 20,
    });
    mocks.getAllProjects.mockResolvedValue([
      { id: "project-api", name: "api", repoPath: "/repos/api", defaultBranch: "main", isWorktree: false },
    ]);
    mocks.getDefaultSessionType.mockResolvedValue("zellij");
  });

  it("처음에는 세션이 있는 첫 task의 터미널을 보여 주고 세션 없는 task를 자동 연결하지 않는다", async () => {
    renderRoute();

    expect((await screen.findByTestId("workspace-terminal")).dataset.taskId).toBe("login");
    expect(screen.queryByTestId("workspace-auto-connect")).toBeNull();
  });

  it("왼쪽 리스트에서 다른 task를 고르면 오른쪽 터미널이 그 task로 바뀐다", async () => {
    // Given
    renderRoute();
    await screen.findByTestId("workspace-terminal");

    // When
    fireEvent.click(screen.getByRole("button", { name: /Billing export/ }));

    // Then
    expect(screen.getByTestId("workspace-terminal").dataset.taskId).toBe("billing");
  });

  it("세션이 없는 task를 고르면 기본 세션 타입으로 자동 연결을 시작한다", async () => {
    // Given
    renderRoute();
    await screen.findByTestId("workspace-terminal");

    // When
    fireEvent.click(screen.getByRole("button", { name: /Plan sprint/ }));

    // Then
    const autoConnect = screen.getByTestId("workspace-auto-connect");
    expect(autoConnect.dataset.taskId).toBe("plan");
    expect(autoConnect.dataset.sessionType).toBe("zellij");
    expect(screen.queryByTestId("workspace-terminal")).toBeNull();
  });
});
