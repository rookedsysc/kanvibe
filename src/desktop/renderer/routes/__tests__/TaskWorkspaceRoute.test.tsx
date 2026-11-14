import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TaskQuickSearchDialog from "@/desktop/renderer/components/TaskQuickSearchDialog";
import TaskWorkspaceRoute from "@/desktop/renderer/routes/TaskWorkspaceRoute";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";
import { TaskStatus } from "@/entities/KanbanTask";

const mocks = vi.hoisted(() => ({
  getTasksByStatus: vi.fn(),
  getTaskById: vi.fn(),
  getSearchableTasks: vi.fn(),
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
  getTaskById: (...args: unknown[]) => mocks.getTaskById(...args),
  getSearchableTasks: (...args: unknown[]) => mocks.getSearchableTasks(...args),
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

function renderRoute(initialEntry = "/ko") {
  render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <TaskWorkspaceRoute />
    </MemoryRouter>,
  );
}

describe("TaskWorkspaceRoute", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getTasksByStatus.mockReset();
    Object.defineProperty(Element.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
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
  it("검색 query의 task를 선택하고 미로딩 DONE task도 불러와 좌우에 표시한다", async () => {
    mocks.getTaskById.mockResolvedValue(createTask("old-done", "Old completed task", TaskStatus.DONE, "done-session"));
    renderRoute("/ko?taskId=old-done");
    const card = await screen.findByRole("button", { name: /Old completed task/ });
    expect(card.getAttribute("aria-current")).toBe("true");
    expect(screen.getByTestId("workspace-terminal").dataset.taskId).toBe("old-done");
  });

  it("검색으로 불러온 DONE과 더보기 중복 응답은 다음 페이지를 건너뛰지 않는다", async () => {
    const baseline = await mocks.getTasksByStatus();
    const firstPage = Array.from({ length: 20 }, (_, index) => createTask(`done-${index}`, `Completed ${index}`, TaskStatus.DONE, "done"));
    const nextPage = Array.from({ length: 20 }, (_, index) => createTask(`done-${index + 20}`, `Completed ${index + 20}`, TaskStatus.DONE, "done"));
    mocks.getTasksByStatus.mockResolvedValue({ ...baseline, tasks: { ...baseline.tasks, done: firstPage }, doneTotal: 60 });
    mocks.getTaskById.mockResolvedValue(createTask("done-59", "Last completed task", TaskStatus.DONE, "done"));
    mocks.getMoreDoneTasks.mockResolvedValue({ tasks: nextPage, doneTotal: 60 });
    renderRoute("/ko?taskId=done-59");
    await screen.findByRole("button", { name: /Last completed task/ });
    fireEvent.click(screen.getByRole("button", { name: "workspace.loadMoreDone" }));
    fireEvent.click(screen.getByRole("button", { name: "workspace.loadMoreDone" }));
    await screen.findByRole("button", { name: /Completed 39/ });
    expect(mocks.getMoreDoneTasks).toHaveBeenCalledTimes(1);
    expect(mocks.getMoreDoneTasks).toHaveBeenNthCalledWith(1, 20);
    fireEvent.click(screen.getByRole("button", { name: "workspace.loadMoreDone" }));
    await waitFor(() => expect(mocks.getMoreDoneTasks).toHaveBeenLastCalledWith(40));
  });

  it("완료 task 더 불러오기가 실패하면 사유와 다시 시도를 보이고 다시 시도로 같은 페이지를 요청한다", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    const baseline = await mocks.getTasksByStatus();
    const firstPage = Array.from({ length: 20 }, (_, index) => createTask(`done-${index}`, `Completed ${index}`, TaskStatus.DONE, "done"));
    const nextPage = Array.from({ length: 20 }, (_, index) => createTask(`done-${index + 20}`, `Completed ${index + 20}`, TaskStatus.DONE, "done"));
    mocks.getTasksByStatus.mockResolvedValue({ ...baseline, tasks: { ...baseline.tasks, done: firstPage }, doneTotal: 60 });
    mocks.getMoreDoneTasks.mockRejectedValueOnce(new Error("db locked")).mockResolvedValueOnce({ tasks: nextPage, doneTotal: 60 });

    try {
      renderRoute();
      fireEvent.click(await screen.findByRole("button", { name: "workspace.loadMoreDone" }));

      expect((await screen.findByRole("alert")).textContent).toContain("common.loadFailed");

      fireEvent.click(screen.getByRole("button", { name: "common.retry" }));

      await screen.findByRole("button", { name: /Completed 39/ });
      expect(screen.queryByRole("alert")).toBeNull();
      expect(mocks.getMoreDoneTasks.mock.calls.map(([offset]) => offset)).toEqual([20, 20]);
    } finally {
      consoleError.mockRestore();
    }
  });

  it("실제 라우터의 검색 선택은 locale을 중복하지 않고 Worktree에 머문다", async () => {
    mocks.getSearchableTasks.mockResolvedValue([{ ...createTask("billing", "Billing export", TaskStatus.REVIEW, "api-billing"), projectName: "api" }]);
    render(<MemoryRouter initialEntries={["/ko"]}><Routes>
      <Route path="/ko" element={<><TaskWorkspaceRoute /><TaskQuickSearchDialog shortcut="Ctrl+K" /></>} />
      <Route path="*" element={<p>Wrong route</p>} />
    </Routes></MemoryRouter>);
    await screen.findByTestId("workspace-terminal");
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const input = await screen.findByRole("textbox");
    await screen.findByText("feat/billing");
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(screen.getByTestId("workspace-terminal").dataset.taskId).toBe("billing"));
    expect(screen.queryByText("Wrong route")).toBeNull();
    expect(screen.getByRole("button", { name: /Billing export/ }).getAttribute("aria-current")).toBe("true");
  });


  describe("초기 조회 지연과 실패", () => {
    afterEach(() => {
      vi.useRealTimers();
    });

    it("조회가 실패하면 빈 task 목록 대신 실패 사유와 재시도를 보이고 재시도로 다시 불러온다", async () => {
      // Given
      const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
      const loadedTasks = await mocks.getTasksByStatus();
      mocks.getTasksByStatus
        .mockRejectedValueOnce(new Error("db closed"))
        .mockResolvedValueOnce(loadedTasks);

      try {
        // When
        renderRoute();

        // Then
        expect((await screen.findByRole("alert")).textContent).toContain("common.loadFailed");
        expect(screen.queryAllByText("Login page")).toHaveLength(0);

        fireEvent.click(screen.getByRole("button", { name: "common.retry" }));

        expect((await screen.findAllByText("Login page")).length).toBeGreaterThan(0);
        expect(screen.queryByRole("alert")).toBeNull();
      } finally {
        consoleError.mockRestore();
      }
    });

    it("조회가 오래 걸리면 빈 task 목록을 만들지 않고 지연 안내를 띄운 채 기다린다", async () => {
      // Given
      vi.useFakeTimers();
      mocks.getTasksByStatus.mockReturnValue(new Promise(() => {}));

      // When
      renderRoute();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(INITIAL_DESKTOP_LOAD_TIMEOUT_MS);
      });

      // Then
      expect(screen.getByRole("status").textContent).toBe("common.loadDelayed");
      expect(screen.queryByRole("list")).toBeNull();
    });

    it("불러온 목록을 새로 고치다 실패하면 기존 목록을 유지하고 갱신 실패 배너를 보인다", async () => {
      // Given
      const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
      renderRoute();
      await screen.findAllByText("Login page");
      mocks.getTasksByStatus.mockRejectedValueOnce(new Error("db closed"));

      try {
        // When
        act(() => {
          triggerDesktopRefresh("board");
        });

        // Then
        expect((await screen.findByRole("alert")).textContent).toContain("common.refreshFailed");
        expect(screen.getAllByText("Login page").length).toBeGreaterThan(0);
      } finally {
        consoleError.mockRestore();
      }
    });
  });
});
