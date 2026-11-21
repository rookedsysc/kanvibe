import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ProjectBranchTasksModal from "../ProjectBranchTasksModal";
import { TaskStatus, type KanbanTask } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({
  getTasksByStatus: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: (namespace: string) => (key: string) => `${namespace}.${key}`,
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  usePathname: () => "/ko",
  useRouter: () => ({ push: vi.fn() }),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  getTasksByStatus: (...args: unknown[]) => mocks.getTasksByStatus(...args),
}));

vi.mock("@/desktop/renderer/utils/taskNavigation", () => ({
  navigateToTaskDetail: vi.fn(),
}));

function createTasks(progressTasks: Partial<KanbanTask>[]) {
  return {
    tasks: {
      [TaskStatus.TODO]: [],
      [TaskStatus.PROGRESS]: progressTasks,
      [TaskStatus.PENDING]: [],
      [TaskStatus.REVIEW]: [],
      [TaskStatus.DONE]: [],
    },
  };
}

describe("ProjectBranchTasksModal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("작업 목록 조회가 실패하면 작업 없음 대신 사유와 다시 시도를 보이고 다시 불러온다", async () => {
    mocks.getTasksByStatus
      .mockRejectedValueOnce(new Error("db closed"))
      .mockResolvedValueOnce(createTasks([{ id: "task-1", title: "Login page", projectId: "project-1", branchName: "feat/login" }]));
    render(<ProjectBranchTasksModal projectId="project-1" currentBranchName={null} onClose={vi.fn()} />);

    expect((await screen.findByRole("alert")).textContent).toContain("common.loadFailed");
    expect(screen.queryByText("작업 없음")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));

    await waitFor(() => {
      expect(screen.getByText("Login page")).toBeTruthy();
    });
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
