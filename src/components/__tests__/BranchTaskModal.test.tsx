import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BranchTaskModal from "../BranchTaskModal";
import ActionStatusToast from "@/desktop/renderer/components/ActionStatusToast";
import type { Project } from "@/entities/Project";
import type { KanbanTask } from "@/entities/KanbanTask";

const { mockBranchFromTask, mockEnsureSessionDependencyWithPrompt, mockGetProjectBranches } = vi.hoisted(() => ({
  mockBranchFromTask: vi.fn(),
  mockEnsureSessionDependencyWithPrompt: vi.fn(),
  mockGetProjectBranches: vi.fn(),
}));

vi.mock("next-intl", () => ({
  useLocale: () => "ko",
  useTranslations: () => (key: string) => key,
}));

const { mockRedirect } = vi.hoisted(() => ({ mockRedirect: vi.fn() }));

vi.mock("@/desktop/renderer/navigation", () => ({
  localizeHref: (href: string, currentLocale = "ko") => href.startsWith("/") ? `/${currentLocale}${href}` : href,
  redirect: (...args: unknown[]) => mockRedirect(...args),
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

describe("BranchTaskModal - 분기 중 닫기와 중복 제출", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetProjectBranches.mockResolvedValue(["main"]);
  });

  function renderModal(isOpen: boolean, onClose = vi.fn()) {
    return (
      <>
        {isOpen ? (
          <BranchTaskModal task={createTask()} projects={[createProject()]} defaultSessionType="tmux" onClose={onClose} />
        ) : null}
        <ActionStatusToast />
      </>
    );
  }

  it("분기 응답을 기다리는 동안 다시 제출해도 요청은 한 번만 보낸다", async () => {
    mockBranchFromTask.mockReturnValue(new Promise(() => {}));
    render(renderModal(true));
    await waitFor(() => {
      expect(mockGetProjectBranches).toHaveBeenCalledWith("project-remote");
    });

    const branchInput = screen.getByPlaceholderText("branchPlaceholder");
    fireEvent.change(branchInput, { target: { value: "feat/once" } });
    const form = branchInput.closest("form")!;
    fireEvent.submit(form);
    fireEvent.submit(form);
    await act(async () => {});

    expect(mockBranchFromTask).toHaveBeenCalledTimes(1);
  });

  it("분기 중 닫은 뒤 성공하면 열기 토스트를 보이고 누르면 그 task로 이동한다", async () => {
    let resolveBranch: (task: { id: string }) => void = () => {};
    mockBranchFromTask.mockReturnValue(new Promise((resolve) => {
      resolveBranch = resolve;
    }));
    const onClose = vi.fn();
    const view = render(renderModal(true, onClose));
    await waitFor(() => {
      expect(mockGetProjectBranches).toHaveBeenCalledWith("project-remote");
    });

    fireEvent.change(screen.getByPlaceholderText("branchPlaceholder"), { target: { value: "feat/bg" } });
    fireEvent.click(screen.getByRole("button", { name: "submit" }));
    await waitFor(() => {
      expect(mockBranchFromTask).toHaveBeenCalledTimes(1);
    });

    fireEvent.click(screen.getByRole("button", { name: "closeAndContinue" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    view.rerender(renderModal(false));

    resolveBranch({ id: "task-1" });

    expect((await screen.findByRole("status")).textContent).toContain("succeeded");
    fireEvent.click(screen.getByRole("button", { name: "open" }));
    await waitFor(() => {
      expect(mockRedirect).toHaveBeenCalledWith("/ko/task/task-1");
    });
  });

  it("분기 중 닫은 뒤 실패하면 실패 토스트로 알린다", async () => {
    let rejectBranch: (error: Error) => void = () => {};
    mockBranchFromTask.mockReturnValue(new Promise((_resolve, reject) => {
      rejectBranch = reject;
    }));
    const view = render(renderModal(true));
    await waitFor(() => {
      expect(mockGetProjectBranches).toHaveBeenCalledWith("project-remote");
    });

    fireEvent.change(screen.getByPlaceholderText("branchPlaceholder"), { target: { value: "feat/fail" } });
    fireEvent.click(screen.getByRole("button", { name: "submit" }));
    await waitFor(() => {
      expect(mockBranchFromTask).toHaveBeenCalledTimes(1);
    });
    view.rerender(renderModal(false));

    rejectBranch(new Error("branch exists"));

    expect((await screen.findByRole("alert")).textContent).toContain("failedWithReason");
    fireEvent.click(screen.getByRole("button", { name: "close" }));
  });
});

describe("BranchTaskModal - 브랜치 목록 조회 상태", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("브랜치 목록 조회가 실패하면 빈 목록 대신 사유와 다시 불러오기를 보인다", async () => {
    mockGetProjectBranches
      .mockRejectedValueOnce(new Error("ssh timeout"))
      .mockResolvedValueOnce(["main", "develop"]);
    render(
      <BranchTaskModal task={createTask()} projects={[createProject()]} defaultSessionType="tmux" onClose={vi.fn()} />,
    );

    expect((await screen.findByRole("alert")).textContent).toContain("loadFailed");
    fireEvent.click(screen.getByRole("button", { name: "retry" }));

    await waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
    expect(mockGetProjectBranches).toHaveBeenCalledTimes(2);
  });
});
