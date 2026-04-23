import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import CreateTaskModal from "../CreateTaskModal";
import type { Project } from "@/entities/Project";

const { mockCreateTask, mockEnsureSessionDependencyWithPrompt, mockGetProjectBranches, mockPush } = vi.hoisted(() => ({
  mockCreateTask: vi.fn(),
  mockEnsureSessionDependencyWithPrompt: vi.fn(),
  mockGetProjectBranches: vi.fn(),
  mockPush: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  useRouter: () => ({ push: mockPush }),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  createTask: (...args: unknown[]) => mockCreateTask(...args),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  getProjectBranches: (...args: unknown[]) => mockGetProjectBranches(...args),
}));

vi.mock("@/desktop/renderer/utils/sessionDependencyPrompt", () => ({
  ensureSessionDependencyWithPrompt: (...args: unknown[]) => mockEnsureSessionDependencyWithPrompt(...args),
}));

vi.mock("../ProjectSelector", () => ({
  default: ({ selectedProjectId }: { selectedProjectId: string }) => (
    <div data-testid="project-selector">{selectedProjectId}</div>
  ),
}));

vi.mock("../PrioritySelector", () => ({
  default: () => <div data-testid="priority-selector" />,
}));

vi.mock("../BranchSearchInput", () => ({
  default: ({ value }: { value: string }) => <div data-testid="branch-search-input">{value}</div>,
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

describe("CreateTaskModal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetProjectBranches.mockResolvedValue(["main", "develop"]);
  });

  it("원격 의존성 프롬프트를 취소하면 작업 생성을 진행하지 않는다", async () => {
    // Given
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(false);

    render(
      <CreateTaskModal
        isOpen
        onClose={vi.fn()}
        sshHosts={["remote-box"]}
        projects={[createProject()]}
        defaultProjectId="project-remote"
      />,
    );

    await waitFor(() => {
      expect(mockGetProjectBranches).toHaveBeenCalledWith("project-remote");
    });

    // When
    fireEvent.change(screen.getByPlaceholderText("branchPlaceholder"), { target: { value: "fix/session-prompt" } });
    fireEvent.click(screen.getByRole("button", { name: "create" }));

    // Then
    await waitFor(() => {
      expect(mockEnsureSessionDependencyWithPrompt).toHaveBeenCalledWith("tmux", "remote-box", expect.any(Function));
    });
    expect(mockCreateTask).not.toHaveBeenCalled();
    expect(mockPush).not.toHaveBeenCalled();
  });

  it("의존성 확인이 실패하면 오류를 보여주고 생성 요청을 멈춘다", async () => {
    // Given
    mockEnsureSessionDependencyWithPrompt.mockRejectedValue(new Error("tmux 설치 실패"));

    render(
      <CreateTaskModal
        isOpen
        onClose={vi.fn()}
        sshHosts={["remote-box"]}
        projects={[createProject()]}
        defaultProjectId="project-remote"
      />,
    );

    await waitFor(() => {
      expect(mockGetProjectBranches).toHaveBeenCalledWith("project-remote");
    });

    // When
    fireEvent.change(screen.getByPlaceholderText("branchPlaceholder"), { target: { value: "fix/session-prompt" } });
    fireEvent.click(screen.getByRole("button", { name: "create" }));

    // Then
    await waitFor(() => {
      expect(screen.getByText("tmux 설치 실패")).toBeTruthy();
    });
    expect(mockCreateTask).not.toHaveBeenCalled();
  });

  it("의존성 준비가 끝나면 생성한 작업 상세 페이지로 이동한다", async () => {
    // Given
    const onClose = vi.fn();
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(true);
    mockCreateTask.mockResolvedValue({ id: "task-2" });

    render(
      <CreateTaskModal
        isOpen
        onClose={onClose}
        sshHosts={["remote-box"]}
        projects={[createProject()]}
        defaultProjectId="project-remote"
      />,
    );

    await waitFor(() => {
      expect(mockGetProjectBranches).toHaveBeenCalledWith("project-remote");
    });

    // When
    fireEvent.change(screen.getByPlaceholderText("branchPlaceholder"), { target: { value: "fix/session-prompt" } });
    fireEvent.click(screen.getByRole("button", { name: "create" }));

    // Then
    await waitFor(() => {
      expect(mockCreateTask).toHaveBeenCalledWith({
        title: "fix/session-prompt",
        description: undefined,
        branchName: "fix/session-prompt",
        baseBranch: "main",
        sessionType: "tmux",
        sshHost: undefined,
        projectId: "project-remote",
        priority: undefined,
      });
    });
    expect(onClose).toHaveBeenCalled();
    expect(mockPush).toHaveBeenCalledWith("/task/task-2");
  });
});
