import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
  default: ({ value, onChange, autoFocus }: { value: string; onChange: (branch: string) => void; autoFocus?: boolean }) => (
    <div>
      <input aria-label="baseBranch" readOnly value={value} autoFocus={autoFocus} />
      <div data-testid="branch-search-input">{value}</div>
      <button type="button" tabIndex={-1} onClick={() => onChange("develop")}>
        select develop
      </button>
    </div>
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

describe("CreateTaskModal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetProjectBranches.mockResolvedValue(["main", "develop"]);
  });

  it("로컬 프로젝트 task 생성도 세션 도구 프롬프트 없이 생성 요청을 보낸다", async () => {
    // Given
    const onClose = vi.fn();
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(false);
    mockCreateTask.mockResolvedValue({ id: "task-local-fast" });
    const localProject = createProject({ sshHost: null });

    render(
      <CreateTaskModal
        isOpen
        onClose={onClose}
        sshHosts={["remote-box"]}
        projects={[localProject]}
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
    expect(mockEnsureSessionDependencyWithPrompt).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
    expect(mockPush).toHaveBeenCalledWith("/task/task-local-fast");
  });

  it("원격 프로젝트 task 생성은 세션 도구 프롬프트 없이 생성 요청을 보낸다", async () => {
    // Given
    const onClose = vi.fn();
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(true);
    mockCreateTask.mockResolvedValue({ id: "task-remote-fast" });

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
    fireEvent.change(screen.getByPlaceholderText("branchPlaceholder"), { target: { value: "fix/remote-fast" } });
    fireEvent.click(screen.getByRole("button", { name: "create" }));

    // Then
    await waitFor(() => {
      expect(mockCreateTask).toHaveBeenCalledWith({
        title: "fix/remote-fast",
        description: undefined,
        branchName: "fix/remote-fast",
        baseBranch: "main",
        sessionType: "tmux",
        sshHost: undefined,
        projectId: "project-remote",
        priority: undefined,
      });
    });
    expect(mockEnsureSessionDependencyWithPrompt).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
    expect(mockPush).toHaveBeenCalledWith("/task/task-remote-fast");
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

  it("Escape를 누르면 모달을 닫는다", async () => {
    // Given
    const onClose = vi.fn();

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
    fireEvent.keyDown(window, { key: "Escape" });

    // Then
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("기본 프로젝트가 있으면 베이스 브랜치에서 시작해 Tab과 Shift+Tab으로 주요 입력 사이를 이동한다", async () => {
    // Given
    const user = userEvent.setup();

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

    const baseBranchInput = screen.getByLabelText("baseBranch");
    const branchNameInput = screen.getByPlaceholderText("branchPlaceholder");
    const descriptionInput = screen.getByPlaceholderText("descriptionPlaceholder");

    // Then
    await waitFor(() => {
      expect(document.activeElement).toBe(baseBranchInput);
    });

    // When & Then
    await user.tab();
    expect(document.activeElement).toBe(branchNameInput);

    await user.tab();
    expect(document.activeElement).toBe(descriptionInput);

    await user.tab({ shift: true });
    expect(document.activeElement).toBe(branchNameInput);

    await user.tab({ shift: true });
    expect(document.activeElement).toBe(baseBranchInput);
  });

  it("포커스가 모달 입력에 있을 때 Escape를 누르면 모달을 닫는다", async () => {
    // Given
    const user = userEvent.setup();
    const onClose = vi.fn();

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
      expect(document.activeElement).toBe(screen.getByLabelText("baseBranch"));
    });

    // When
    await user.keyboard("{Escape}");

    // Then
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("입력창에서 Enter를 누르면 작업을 생성한다", async () => {
    // Given
    const onClose = vi.fn();
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(true);
    mockCreateTask.mockResolvedValue({ id: "task-enter" });

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
    const branchNameInput = screen.getByPlaceholderText("branchPlaceholder");
    fireEvent.change(branchNameInput, { target: { value: "fix/enter-submit" } });
    fireEvent.keyDown(branchNameInput, { key: "Enter" });

    // Then
    await waitFor(() => {
      expect(mockCreateTask).toHaveBeenCalledWith({
        title: "fix/enter-submit",
        description: undefined,
        branchName: "fix/enter-submit",
        baseBranch: "main",
        sessionType: "tmux",
        sshHost: undefined,
        projectId: "project-remote",
        priority: undefined,
      });
    });
    expect(onClose).toHaveBeenCalled();
    expect(mockPush).toHaveBeenCalledWith("/task/task-enter");
  });

  it("프로젝트 목록이 새로고침되어도 사용자가 선택한 베이스 브랜치를 유지한다", async () => {
    // Given
    const project = createProject();
    const { rerender } = render(
      <CreateTaskModal
        isOpen
        onClose={vi.fn()}
        sshHosts={["remote-box"]}
        projects={[project]}
        defaultProjectId="project-remote"
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("branch-search-input").textContent).toBe("main");
    });

    // When
    fireEvent.click(screen.getByRole("button", { name: "select develop" }));
    expect(screen.getByTestId("branch-search-input").textContent).toBe("develop");

    rerender(
      <CreateTaskModal
        isOpen
        onClose={vi.fn()}
        sshHosts={["remote-box"]}
        projects={[createProject({ createdAt: new Date("2026-05-01T00:00:00.000Z") })]}
        defaultProjectId="project-remote"
      />,
    );

    // Then
    await waitFor(() => {
      expect(screen.getByTestId("branch-search-input").textContent).toBe("develop");
    });
  });
});
