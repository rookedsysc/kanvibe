import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import WorkspaceTaskList from "@/desktop/renderer/components/WorkspaceTaskList";
import { TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import type { BoardSortPreference } from "@/desktop/shared/boardSort";

const mocks = vi.hoisted(() => ({
  sortPreference: { keys: [] } as BoardSortPreference,
}));

vi.mock("next-intl", () => ({
  useLocale: () => "en",
  useTranslations: (namespace: string) => (key: string) => `${namespace}.${key}`,
}));

vi.mock("@/desktop/renderer/hooks/useBoardSortPreference", () => ({
  useBoardSortPreference: () => [mocks.sortPreference, vi.fn()],
}));

const PROJECTS = [
  { id: "project-api", name: "api", repoPath: "/repos/api", defaultBranch: "main", isWorktree: false },
  { id: "project-web", name: "web", repoPath: "/repos/web", defaultBranch: "main", isWorktree: false },
] as unknown as Project[];

function createTask(overrides: Partial<KanbanTask> & Pick<KanbanTask, "id" | "title" | "status">): KanbanTask {
  return {
    description: null,
    branchName: `feat/${overrides.id}`,
    worktreePath: null,
    sessionType: null,
    sessionName: null,
    sshHost: null,
    agentType: null,
    project: null,
    projectId: "project-api",
    baseBranch: "main",
    priority: null,
    prUrl: null,
    createdAt: new Date("2026-11-01T00:00:00Z"),
    updatedAt: new Date("2026-11-01T00:00:00Z"),
    ...overrides,
  } as KanbanTask;
}

const TASKS: KanbanTask[] = [
  createTask({ id: "login", title: "Login page", status: TaskStatus.PROGRESS }),
  createTask({ id: "api-root", title: "API root", status: TaskStatus.TODO, branchName: "main" }),
  createTask({ id: "billing", title: "Billing export", status: TaskStatus.REVIEW, projectId: "project-web" }),
  createTask({ id: "archive", title: "Archive cleanup", status: TaskStatus.DONE, projectId: "project-web" }),
];

function renderTaskList({
  initialEntry = "/ko",
  onSelectTask = vi.fn(),
  selectedTaskId = null,
}: { initialEntry?: string; onSelectTask?: (taskId: string) => void; selectedTaskId?: string | null } = {}) {
  render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <WorkspaceTaskList
        tasks={TASKS}
        projects={PROJECTS}
        selectedTaskId={selectedTaskId}
        onSelectTask={onSelectTask}
        hasMoreDoneTasks={false}
        onLoadMoreDoneTasks={vi.fn()}
      />
    </MemoryRouter>,
  );
}

function getListedTaskTitles() {
  const taskList = screen.getByRole("list", { name: "workspace.taskListLabel" });
  return within(taskList).queryAllByTestId("workspace-task-title").map((title) => title.textContent);
}

describe("WorkspaceTaskList", () => {
  beforeEach(() => {
    mocks.sortPreference = { keys: [] };
    sessionStorage.clear();
  });

  it("필터가 없으면 done을 포함한 모든 task를 상태 순서대로 나열한다", () => {
    renderTaskList();

    expect(getListedTaskTitles()).toEqual(["API root", "Login page", "Billing export", "Archive cleanup"]);
  });

  it("검색어는 제목·브랜치·프로젝트 이름에서 대소문자 구분 없이 찾는다", () => {
    // Given
    renderTaskList();
    const searchInput = screen.getByRole("searchbox", { name: "workspace.searchPlaceholder" });

    // When
    fireEvent.change(searchInput, { target: { value: "WEB" } });

    // Then
    expect(getListedTaskTitles()).toEqual(["Billing export", "Archive cleanup"]);

    // When
    fireEvent.change(searchInput, { target: { value: "feat/login" } });

    // Then
    expect(getListedTaskTitles()).toEqual(["Login page"]);
  });

  it("상태 칩은 여러 개를 함께 고를 수 있고, 다시 누르면 해제된다", () => {
    // Given
    renderTaskList();
    const statusFilter = screen.getByRole("group", { name: "workspace.statusFilterLabel" });

    // When
    fireEvent.click(within(statusFilter).getByRole("button", { name: "board.columns.progress" }));
    fireEvent.click(within(statusFilter).getByRole("button", { name: "board.columns.done" }));

    // Then
    expect(getListedTaskTitles()).toEqual(["Login page", "Archive cleanup"]);
    expect(within(statusFilter).getByRole("button", { name: "board.columns.done" }).getAttribute("aria-pressed")).toBe("true");

    // When
    fireEvent.click(within(statusFilter).getByRole("button", { name: "board.columns.done" }));

    // Then
    expect(getListedTaskTitles()).toEqual(["Login page"]);
  });

  it("URL의 프로젝트 필터를 칸반과 같이 따른다", () => {
    renderTaskList({ initialEntry: "/ko?projects=project-web" });

    expect(getListedTaskTitles()).toEqual(["Billing export", "Archive cleanup"]);
  });

  it("Project 종류 필터는 프로젝트 기본 브랜치 task만 남긴다", () => {
    // Given
    renderTaskList();
    const kindFilter = screen.getByRole("group", { name: "board.taskKindFilter.label" });

    // When
    fireEvent.click(within(kindFilter).getByRole("button", { name: "board.taskKindFilter.options.project" }));

    // Then
    expect(getListedTaskTitles()).toEqual(["API root"]);
  });

  it("정렬 설정이 있으면 같은 상태 안에서 그 기준으로 늘어놓는다", () => {
    // Given
    mocks.sortPreference = { keys: [{ field: "title", direction: "desc" }] };
    const extraTodo = createTask({ id: "zeta", title: "Zeta todo", status: TaskStatus.TODO });

    // When
    render(
      <MemoryRouter initialEntries={["/ko"]}>
        <WorkspaceTaskList
          tasks={[...TASKS, extraTodo]}
          projects={PROJECTS}
          selectedTaskId={null}
          onSelectTask={vi.fn()}
          hasMoreDoneTasks={false}
          onLoadMoreDoneTasks={vi.fn()}
        />
      </MemoryRouter>,
    );

    // Then
    expect(getListedTaskTitles()).toEqual(["Zeta todo", "API root", "Login page", "Billing export", "Archive cleanup"]);
  });

  it("task를 누르면 그 task를 선택하고, 선택된 task는 aria-current로 표시한다", () => {
    // Given
    const handleSelectTask = vi.fn();
    renderTaskList({ onSelectTask: handleSelectTask, selectedTaskId: "billing" });

    // When
    fireEvent.click(screen.getByRole("button", { name: /Login page/ }));

    // Then
    expect(handleSelectTask).toHaveBeenCalledExactlyOnceWith("login");
    expect(screen.getByRole("button", { name: /Billing export/ }).getAttribute("aria-current")).toBe("true");
  });

  it("필터 결과가 없으면 안내 문구를 보여 준다", () => {
    // Given
    renderTaskList();

    // When
    fireEvent.change(screen.getByRole("searchbox", { name: "workspace.searchPlaceholder" }), { target: { value: "no-such-task" } });

    // Then
    expect(getListedTaskTitles()).toEqual([]);
    expect(screen.getByText("workspace.noMatchingTasks")).toBeTruthy();
  });
});
