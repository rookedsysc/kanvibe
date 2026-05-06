import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import BoardRoute from "@/desktop/renderer/routes/BoardRoute";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS } from "@/desktop/renderer/utils/loadingTimeout";

const BOARD_CACHE_KEY = "kanvibe:route-cache:board";
const BOARD_FOCUS_TASK_CACHE_KEY = "kanvibe:route-cache:board-focus-task";

const mocks = vi.hoisted(() => ({
  getTasksByStatus: vi.fn(),
  getAvailableHosts: vi.fn(),
  getAllProjects: vi.fn(),
  getSidebarDefaultCollapsed: vi.fn(),
  getDoneAlertDismissed: vi.fn(),
  getNotificationSettings: vi.fn(),
  getDefaultSessionType: vi.fn(),
  getTaskSearchShortcut: vi.fn(),
  useRefreshSignal: vi.fn(() => 0),
}));

function createDeferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

vi.mock("@/components/Board", () => ({
  default: ({
    initialTasks,
    initialFocusTaskId,
  }: {
    initialTasks: Record<string, Array<{ title: string }>>;
    initialFocusTaskId?: string | null;
  }) => (
    <>
      <div data-testid="board-titles">{Object.values(initialTasks).flat().map((task) => task.title).join(",")}</div>
      <div data-testid="board-focus-task">{initialFocusTaskId ?? ""}</div>
    </>
  ),
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  getTasksByStatus: (...args: unknown[]) => mocks.getTasksByStatus(...args),
}));

vi.mock("@/desktop/renderer/actions/project", () => ({
  getAvailableHosts: (...args: unknown[]) => mocks.getAvailableHosts(...args),
  getAllProjects: (...args: unknown[]) => mocks.getAllProjects(...args),
}));

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getSidebarDefaultCollapsed: (...args: unknown[]) => mocks.getSidebarDefaultCollapsed(...args),
  getDoneAlertDismissed: (...args: unknown[]) => mocks.getDoneAlertDismissed(...args),
  getNotificationSettings: (...args: unknown[]) => mocks.getNotificationSettings(...args),
  getDefaultSessionType: (...args: unknown[]) => mocks.getDefaultSessionType(...args),
  getTaskSearchShortcut: (...args: unknown[]) => mocks.getTaskSearchShortcut(...args),
}));

vi.mock("@/desktop/renderer/utils/refresh", () => ({
  useRefreshSignal: () => mocks.useRefreshSignal(),
}));

describe("BoardRoute", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    sessionStorage.clear();
    mocks.getAvailableHosts.mockResolvedValue([]);
    mocks.getAllProjects.mockResolvedValue([]);
    mocks.getSidebarDefaultCollapsed.mockResolvedValue(false);
    mocks.getDoneAlertDismissed.mockResolvedValue(false);
    mocks.getNotificationSettings.mockResolvedValue({ isEnabled: true, enabledStatuses: [] });
    mocks.getDefaultSessionType.mockResolvedValue("tmux");
    mocks.getTaskSearchShortcut.mockResolvedValue("Mod+Shift+O");
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("초기 데이터가 없으면 보드 형태의 skeleton을 즉시 렌더링한다", () => {
    // Given
    mocks.getTasksByStatus.mockReturnValue(new Promise(() => {}));

    // When
    render(<BoardRoute />);

    // Then
    expect(screen.getByTestId("board-route-skeleton")).toBeTruthy();
    expect(screen.queryByText("Loading...")).toBeNull();
  });

  it("캐시가 있으면 stale board를 즉시 렌더링하고 이후 최신 데이터로 갱신한다", async () => {
    // Given
    sessionStorage.setItem(BOARD_CACHE_KEY, JSON.stringify({
      tasks: {
        tasks: {
          todo: [{ id: "cached-task", title: "cached board task" }],
          progress: [],
          pending: [],
          review: [],
          done: [],
        },
        doneTotal: 0,
        doneLimit: 20,
      },
      sshHosts: [],
      projects: [],
      sidebarDefaultCollapsed: false,
      doneAlertDismissed: false,
      notificationSettings: { isEnabled: true, enabledStatuses: [] },
      defaultSessionType: "tmux",
      taskSearchShortcut: "Mod+Shift+O",
    }));
    const deferredTasks = createDeferred<{
      tasks: {
        todo: Array<{ id: string; title: string }>;
        progress: never[];
        pending: never[];
        review: never[];
        done: never[];
      };
      doneTotal: number;
      doneLimit: number;
    }>();
    mocks.getTasksByStatus.mockReturnValue(deferredTasks.promise);

    // When
    render(<BoardRoute />);

    // Then
    expect(screen.queryByText("Loading...")).toBeNull();
    expect(screen.getByTestId("board-titles").textContent).toBe("cached board task");

    deferredTasks.resolve({
      tasks: {
        todo: [{ id: "fresh-task", title: "fresh board task" }],
        progress: [],
        pending: [],
        review: [],
        done: [],
      },
      doneTotal: 0,
      doneLimit: 20,
    });

    await waitFor(() => {
      expect(screen.getByTestId("board-titles").textContent).toBe("fresh board task");
    });
  });

  it("상세 화면에서 기록한 focus task id를 한 번 소비해서 Board에 전달한다", async () => {
    sessionStorage.setItem(BOARD_FOCUS_TASK_CACHE_KEY, JSON.stringify("task-2"));
    mocks.getTasksByStatus.mockResolvedValue({
      tasks: {
        todo: [],
        progress: [{ id: "task-2", title: "focus target task" }],
        pending: [],
        review: [],
        done: [],
      },
      doneTotal: 0,
      doneLimit: 20,
    });

    render(<BoardRoute />);

    await waitFor(() => {
      expect(screen.getByTestId("board-focus-task").textContent).toBe("task-2");
    });
    expect(sessionStorage.getItem(BOARD_FOCUS_TASK_CACHE_KEY)).toBeNull();
  });

  it("초기 데이터 로딩이 실패해도 Loading 화면에 고착되지 않는다", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.getTasksByStatus.mockRejectedValue(new Error("database failed"));

    try {
      render(<BoardRoute />);

      await waitFor(() => {
        expect(screen.queryByText("Loading...")).toBeNull();
      });
      expect(screen.getByTestId("board-titles").textContent).toBe("");
    } finally {
      consoleError.mockRestore();
    }
  });

  it("should not stay on the loading screen when initial data loading never settles", async () => {
    // Given
    vi.useFakeTimers();
    mocks.getTasksByStatus.mockReturnValue(new Promise(() => {}));

    // When
    render(<BoardRoute />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(INITIAL_DESKTOP_LOAD_TIMEOUT_MS);
    });

    // Then
    expect(screen.queryByText("Loading...")).toBeNull();
    expect(screen.getByTestId("board-titles").textContent).toBe("");
  });
});
