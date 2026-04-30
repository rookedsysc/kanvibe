import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BoardRoute from "@/desktop/renderer/routes/BoardRoute";

const BOARD_CACHE_KEY = "kanvibe:route-cache:board";

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
  default: ({ initialTasks }: { initialTasks: Record<string, Array<{ title: string }>> }) => (
    <div data-testid="board-titles">{Object.values(initialTasks).flat().map((task) => task.title).join(",")}</div>
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
});
