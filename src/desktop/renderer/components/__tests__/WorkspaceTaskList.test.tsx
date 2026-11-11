import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { setShortcutCaptureActive } from "@/desktop/renderer/utils/shortcutBindings";
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
  return within(taskList).queryAllByRole("heading", { level: 3 }).map((title) => title.textContent);
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
  it("↑↓는 정렬된 표시 순서로 선택하고 경계에서 멈춘다", () => {
    const onSelectTask = vi.fn();
    renderTaskList({ onSelectTask, selectedTaskId: "login" });
    const login = screen.getByRole("button", { name: /Login page/ });
    act(() => login.focus());
    fireEvent.keyDown(login, { key: "ArrowDown" });
    expect(onSelectTask).toHaveBeenLastCalledWith("billing");
    expect(document.activeElement).toBe(screen.getByRole("button", { name: /Billing export/ }));
    const first = screen.getByRole("button", { name: /API root/ });
    fireEvent.keyDown(first, { key: "ArrowUp" });
    expect(onSelectTask).not.toHaveBeenCalledWith(undefined);
    expect(document.activeElement).toBe(first);
  });

  it("목록 Enter는 상세 경로 대신 터미널 포커스를 요청한다", async () => {
    renderTaskList({ selectedTaskId: "login" });
    const listener = vi.fn();
    window.addEventListener("kanvibe:request-terminal-focus", listener);
    fireEvent.keyDown(screen.getByRole("button", { name: /Login page/ }), { key: "Enter" });
    await waitFor(() => expect(listener).toHaveBeenCalledTimes(1));
    window.removeEventListener("kanvibe:request-terminal-focus", listener);
  });

  it("선택된 작업은 필터 밖이어도 보이고 사용자 검색 변경은 그대로 필터링한다", () => {
    renderTaskList({ initialEntry: "/ko?projects=project-web", selectedTaskId: "login" });
    expect(screen.getByRole("button", { name: /Login page/ }).getAttribute("aria-current")).toBe("true");
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "no-such-task" } });
    expect(getListedTaskTitles()).toEqual([]);
  });

  it("Mod+Alt 단독 release는 선택된 목록 카드에 포커스를 둔다", () => {
    renderTaskList({ selectedTaskId: "login" });
    const search = screen.getByRole("searchbox");
    act(() => search.focus());
    fireEvent.keyDown(window, { key: "Alt", ctrlKey: true, altKey: true });
    fireEvent.keyUp(window, { key: "Alt", ctrlKey: true });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: /Login page/ }));
  });

  it("같은 작업을 재검색해도 새 검색 요청이면 필터로 숨긴 선택을 다시 보여 준다", () => {
    const onSelectTask = vi.fn();
    const props = { tasks: TASKS, projects: PROJECTS, selectedTaskId: "login", onSelectTask, hasMoreDoneTasks: false, onLoadMoreDoneTasks: vi.fn() };
    const { rerender } = render(<MemoryRouter><WorkspaceTaskList {...props} selectionRequestKey="search-1" /></MemoryRouter>);
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "Billing" } });
    expect(screen.queryByRole("button", { name: /Login page/ })).toBeNull();
    rerender(<MemoryRouter><WorkspaceTaskList {...props} selectionRequestKey="search-2" /></MemoryRouter>);
    expect(screen.getByRole("button", { name: /Login page/ }).getAttribute("aria-current")).toBe("true");
  });

  it("선택이 숨겨져 있으면 목록 포커스 단축키는 첫 표시 작업도 함께 선택한다", () => {
    const onSelectTask = vi.fn();
    renderTaskList({ selectedTaskId: "login", onSelectTask });
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "Billing" } });
    const billing = screen.getByRole("button", { name: /Billing export/ });
    expect(billing.tabIndex).toBe(0);
    fireEvent.keyDown(window, { key: "Alt", ctrlKey: true, altKey: true });
    fireEvent.keyUp(window, { key: "Alt", ctrlKey: true });
    expect(document.activeElement).toBe(billing);
    expect(onSelectTask).toHaveBeenLastCalledWith("billing");
  });

  it("색상/아이콘을 저장하지 않은 프로젝트도 Kanban의 이니셜 마커를 표시한다", () => {
    renderTaskList();
    expect(within(screen.getByRole("button", { name: /Login page/ })).getByTestId("project-initial-icon").textContent).toBe("a");
  });

  it("worktree 프로젝트 카드도 메인 프로젝트 이름·아이콘을 사용한다", () => {
    const projects = [
      { ...PROJECTS[0], color: "#0064ff", iconDataUrl: "data:image/png;base64,aGVsbG8=" },
      { ...PROJECTS[0], id: "worktree", name: "api-worktree", repoPath: "/repos/api__worktrees/feature", isWorktree: true },
    ];
    const task = createTask({ id: "worktree-task", title: "Worktree task", status: TaskStatus.TODO, projectId: "worktree" });
    render(<MemoryRouter><WorkspaceTaskList tasks={[task]} projects={projects} selectedTaskId={task.id} onSelectTask={vi.fn()} hasMoreDoneTasks={false} onLoadMoreDoneTasks={vi.fn()} /></MemoryRouter>);
    const card = screen.getByRole("button", { name: /Worktree task/ });
    expect(within(card).getByText("api")).toBeTruthy();
    expect(within(card).getByRole("img", { name: "api icon" }).getAttribute("src")).toBe(projects[0].iconDataUrl);
  });

  it("Electron 포커스 요청도 모달과 단축키 녹화 중에는 무시한다", () => {
    let requestFocus = () => {};
    const previousApi = window.kanvibeDesktop;
    window.kanvibeDesktop = { onWorkspaceListFocusShortcut: (listener: () => void) => { requestFocus = listener; return () => {}; } } as typeof window.kanvibeDesktop;
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    try {
      renderTaskList({ selectedTaskId: "login" });
      const search = screen.getByRole("searchbox");
      act(() => search.focus());
      document.body.append(dialog);
      act(() => requestFocus());
      expect(document.activeElement).toBe(search);
      dialog.remove();
      setShortcutCaptureActive(true);
      act(() => requestFocus());
      expect(document.activeElement).toBe(search);
      setShortcutCaptureActive(false);
      act(() => requestFocus());
      expect(document.activeElement).toBe(screen.getByRole("button", { name: /Login page/ }));
    } finally {
      dialog.remove();
      setShortcutCaptureActive(false);
      window.kanvibeDesktop = previousApi;
    }
  });

});
