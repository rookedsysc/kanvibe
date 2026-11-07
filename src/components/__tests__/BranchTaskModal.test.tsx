import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  default: ({ value, branches }: { value: string; branches: string[] }) => (
    <><input aria-label="baseBranch" readOnly value={value} /><output data-testid="branches">{branches.join(",")}</output></>
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
    iconDataUrl: null,
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

  it("로컬 프로젝트 branch 생성도 세션 도구 프롬프트 없이 분기 요청을 보낸다", async () => {
    // Given
    const onClose = vi.fn();

    render(
      <BranchTaskModal
        task={createTask()}
        projects={[createProject({ sshHost: null })]}
        defaultSessionType="tmux"
        onClose={onClose}
      />,
    );

    await waitFor(() => {
      expect(mockGetProjectBranches).toHaveBeenCalledWith("project-remote");
    });

    // When
    fireEvent.change(screen.getByPlaceholderText("branchPlaceholder"), {
      target: { value: "feat/local-fast" },
    });
    fireEvent.click(screen.getByRole("button", { name: "submit" }));

    // Then
    await waitFor(() => {
      expect(mockBranchFromTask).toHaveBeenCalledWith(
        "task-1",
        "project-remote",
        "main",
        "feat/local-fast",
        "tmux",
      );
    });
    expect(mockEnsureSessionDependencyWithPrompt).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
  });
  it.each([true, false])("Herdr 설치 준비 결과 %s를 확인한 후에만 작업을 생성한다", async (ready) => {
    const onClose = vi.fn();
    mockEnsureSessionDependencyWithPrompt.mockResolvedValue(ready);
    mockBranchFromTask.mockResolvedValue({ id: "herdr-task" });
    render(<BranchTaskModal task={createTask()} projects={[createProject()]} defaultSessionType="herdr" onClose={onClose} />);
    await waitFor(() => expect(mockGetProjectBranches).toHaveBeenCalled());
    fireEvent.change(screen.getByPlaceholderText("branchPlaceholder"), { target: { value: "feature-herdr" } });
    fireEvent.click(screen.getByRole("button", { name: "submit" }));
    await waitFor(() => expect(mockEnsureSessionDependencyWithPrompt).toHaveBeenCalledWith("herdr", "remote-box", expect.any(Function)));
    await waitFor(() => expect(mockBranchFromTask).toHaveBeenCalledTimes(ready ? 1 : 0));
    expect(onClose).toHaveBeenCalledTimes(ready ? 1 : 0);
  });

  it("설정이 전달되지 않은 분기도 Herdr를 기본 선택한다", async () => {
    render(<BranchTaskModal task={createTask()} projects={[createProject()]} onClose={vi.fn()} />);
    await waitFor(() => expect(mockGetProjectBranches).toHaveBeenCalled());
    expect((screen.getByRole("option", { name: "herdr" }) as HTMLOptionElement).selected).toBe(true);
  });

  it.each(["success", "failure"])("프로젝트 변경 뒤 이전 %s 응답을 무시한다", async (outcome) => {
    let resolveOld!: (value: string[]) => void;
    let rejectOld!: (error: Error) => void;
    let resolveNew!: (value: string[]) => void;
    mockGetProjectBranches.mockImplementation((projectId: string) => new Promise<string[]>((resolve, reject) => {
      if (projectId === "A") { resolveOld = resolve; rejectOld = reject; }
      else resolveNew = resolve;
    }));
    render(<BranchTaskModal task={createTask()} projects={[createProject({ id: "A" }), createProject({ id: "B" })]} onClose={vi.fn()} />);
    fireEvent.change(screen.getAllByRole("combobox")[0], { target: { value: "B" } });
    await act(async () => { resolveNew(["B-only"]); });
    expect(screen.getByTestId("branches").textContent).toBe("B-only");
    await act(async () => {
      if (outcome === "success") resolveOld(["A-only"]);
      else rejectOld(new Error("A failed"));
    });
    expect(screen.getByTestId("branches").textContent).toBe("B-only");
  });

});
