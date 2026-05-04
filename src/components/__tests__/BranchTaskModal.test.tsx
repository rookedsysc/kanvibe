import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BranchTaskModal from "../BranchTaskModal";
import type { Project } from "@/entities/Project";
import type { KanbanTask } from "@/entities/KanbanTask";

const { mockBranchFromTask, mockEnsureSessionDependencyWithPrompt, mockGetProjectBranches } = vi.hoisted(() => ({
  mockBranchFromTask: vi.fn(),
  mockEnsureSessionDependencyWithPrompt: vi.fn(),
  mockGetProjectBranches: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  branchFromTask: (...args: unknown[]) => mockBranchFromTask(...args),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  getProjectBranches: (...args: unknown[]) => mockGetProjectBranches(...args),
}));

vi.mock("@/desktop/renderer/utils/sessionDependencyPrompt", () => ({
  ensureSessionDependencyWithPrompt: (...args: unknown[]) => mockEnsureSessionDependencyWithPrompt(...args),
}));

vi.mock("../BranchSearchInput", () => ({
  default: ({ value }: { value: string }) => (
    <input aria-label="baseBranch" readOnly value={value} />
  ),
}));

function createProject(overrides?: Partial<Project>): Project {
  return {
    id: "project-remote",
    name: "kanvibe",
    repoPath: "/repo/kanvibe",
    defaultBranch: "main",
    sshHost: "remote-box",
    isWorktree: false,
    color: null,
    createdAt: new Date(),
    ...overrides,
  };
}

function createTask(overrides?: Partial<KanbanTask>): KanbanTask {
  return {
    id: "task-1",
    title: "Parent task",
    description: null,
    status: "todo",
    branchName: "main",
    baseBranch: "main",
    sessionType: null,
    sessionName: null,
    worktreePath: null,
    sshHost: null,
    projectId: "project-remote",
    priority: null,
    displayOrder: 0,
    prUrl: null,
    createdAt: new Date(),
    updatedAt: new Date(),
    project: null,
    ...overrides,
  } as KanbanTask;
}

describe("BranchTaskModal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetProjectBranches.mockResolvedValue(["main", "develop"]);
    mockBranchFromTask.mockResolvedValue(createTask());
  });

  it("원격 프로젝트 branch 생성은 세션 도구 프롬프트 없이 분기 요청을 보낸다", async () => {
    // Given
    const onClose = vi.fn();

    render(
      <BranchTaskModal
        task={createTask()}
        projects={[createProject()]}
        defaultSessionType="tmux"
        onClose={onClose}
      />,
    );

    await waitFor(() => {
      expect(mockGetProjectBranches).toHaveBeenCalledWith("project-remote");
    });

    // When
    fireEvent.change(screen.getByPlaceholderText("branchPlaceholder"), {
      target: { value: "feat/remote-fast" },
    });
    fireEvent.click(screen.getByRole("button", { name: "submit" }));

    // Then
    await waitFor(() => {
      expect(mockBranchFromTask).toHaveBeenCalledWith(
        "task-1",
        "project-remote",
        "main",
        "feat/remote-fast",
        "tmux",
      );
    });
    expect(mockEnsureSessionDependencyWithPrompt).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
