import { useEffect, useState } from "react";
import Board from "@/components/Board";
import { getDoneAlertDismissed, getDefaultSessionType, getNotificationSettings, getSidebarDefaultCollapsed, getTaskSearchShortcut } from "@/desktop/renderer/actions/appSettings";
import { getTasksByStatus } from "@/desktop/renderer/actions/kanban";
import { getAllProjects, getAvailableHosts } from "@/desktop/renderer/actions/project";
import { buildRouteCacheKey, readRouteCache, writeRouteCache } from "@/desktop/renderer/utils/routeCache";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import { consumeBoardFocusTask } from "@/desktop/renderer/utils/boardFocusTarget";
import { DEFAULT_TASK_SEARCH_SHORTCUT } from "@/desktop/renderer/utils/keyboardShortcut";
import { INITIAL_DESKTOP_LOAD_TIMEOUT_MS, logDesktopInitialLoadTimeout } from "@/desktop/renderer/utils/loadingTimeout";
import { SessionType, TaskStatus } from "@/entities/KanbanTask";

interface BoardData {
  tasks: Awaited<ReturnType<typeof getTasksByStatus>>;
  sshHosts: string[];
  projects: Awaited<ReturnType<typeof getAllProjects>>;
  sidebarDefaultCollapsed: boolean;
  doneAlertDismissed: boolean;
  notificationSettings: Awaited<ReturnType<typeof getNotificationSettings>>;
  defaultSessionType: Awaited<ReturnType<typeof getDefaultSessionType>>;
  taskSearchShortcut: Awaited<ReturnType<typeof getTaskSearchShortcut>>;
}

const BOARD_ROUTE_CACHE_KEY = buildRouteCacheKey("board");
const BOARD_SKELETON_COLUMN_COUNT = 5;
const BOARD_SKELETON_TASK_COUNTS = [3, 2, 3, 2, 2];
const BOARD_SKELETON_BADGE_WIDTHS = ["w-9", "w-7", "w-9"];
const BOARD_SKELETON_TITLE_WIDTHS = ["w-11/12", "w-4/5", "w-3/4"];
const BOARD_SKELETON_DESCRIPTION_WIDTHS = ["w-2/3", "w-1/2", "w-3/5"];
const BOARD_SKELETON_STATUS_DOT_CLASSES = [
  "bg-status-todo",
  "bg-status-progress",
  "bg-status-pending",
  "bg-status-review",
  "bg-status-done",
];

function createEmptyBoardData(): BoardData {
  return {
    tasks: {
      tasks: {
        [TaskStatus.TODO]: [],
        [TaskStatus.PROGRESS]: [],
        [TaskStatus.PENDING]: [],
        [TaskStatus.REVIEW]: [],
        [TaskStatus.DONE]: [],
      },
      doneTotal: 0,
      doneLimit: 20,
    },
    sshHosts: [],
    projects: [],
    sidebarDefaultCollapsed: false,
    doneAlertDismissed: false,
    notificationSettings: { isEnabled: true, enabledStatuses: ["progress", "pending", "review"] },
    defaultSessionType: SessionType.TMUX,
    taskSearchShortcut: DEFAULT_TASK_SEARCH_SHORTCUT,
  };
}

function BoardTaskCardSkeleton({ index }: { index: number }) {
  return (
    <div className="rounded-md border border-border-subtle px-2.5 py-2">
      <div className="mb-2 grid grid-cols-[6px_minmax(0,1fr)] items-center gap-2">
        <div className="h-1.5 w-1.5 rounded-full bg-border-strong" />
        <div className="h-3 w-24 rounded bg-border-subtle" />
      </div>
      <div className={`h-3.5 rounded bg-border-default ${BOARD_SKELETON_TITLE_WIDTHS[index % BOARD_SKELETON_TITLE_WIDTHS.length]}`} />
      <div className={`mt-2 h-2.5 rounded bg-border-subtle ${BOARD_SKELETON_DESCRIPTION_WIDTHS[index % BOARD_SKELETON_DESCRIPTION_WIDTHS.length]}`} />
      <div className="mt-3 flex gap-1.5">
        {BOARD_SKELETON_BADGE_WIDTHS.map((badgeWidth, badgeIndex) => (
          <div key={badgeIndex} className={`h-4 rounded border border-border-subtle bg-bg-page ${badgeWidth}`} />
        ))}
      </div>
    </div>
  );
}

function BoardColumnSkeleton({ columnIndex }: { columnIndex: number }) {
  const taskCount = BOARD_SKELETON_TASK_COUNTS[columnIndex] ?? BOARD_SKELETON_TASK_COUNTS[0];

  return (
    <div className="min-w-[284px] max-w-[340px] flex-1 overflow-hidden rounded-lg border border-border-default">
      <div className="flex h-10 items-center gap-2 border-b border-border-subtle px-3">
        <div className={`h-2 w-2 rounded-full ${BOARD_SKELETON_STATUS_DOT_CLASSES[columnIndex] ?? "bg-border-strong"}`} />
        <div className="h-3 w-20 rounded bg-border-subtle" />
        <div className="ml-auto h-3 w-5 rounded bg-border-subtle" />
      </div>
      <div className="min-h-[calc(100vh-132px)] space-y-1.5 p-2">
        {Array.from({ length: taskCount }, (_, taskIndex) => (
          <BoardTaskCardSkeleton key={taskIndex} index={taskIndex + columnIndex} />
        ))}
      </div>
    </div>
  );
}

function BoardRouteSkeleton() {
  return (
    <div className="min-h-screen bg-bg-page" data-testid="board-route-skeleton" aria-hidden="true">
      <header className="flex items-center justify-end border-b border-border-default bg-bg-surface px-6 py-3">
        <div className="flex items-center gap-3">
          <div className="h-8 w-64 rounded-md border border-border-default bg-bg-page" />
          <div className="h-8 w-20 rounded-md bg-brand-primary/40" />
          <div className="h-8 w-8 rounded-md border border-border-default bg-bg-page" />
          <div className="h-8 w-8 rounded-md border border-border-default bg-bg-page" />
        </div>
      </header>
      <main className="p-6">
        <div className="flex gap-3 overflow-x-auto pb-2 opacity-80 animate-pulse">
          {Array.from({ length: BOARD_SKELETON_COLUMN_COUNT }, (_, columnIndex) => (
            <BoardColumnSkeleton key={columnIndex} columnIndex={columnIndex} />
          ))}
        </div>
      </main>
    </div>
  );
}

export default function BoardRoute() {
  const refreshSignal = useRefreshSignal(["all", "board"]);
  const [data, setData] = useState<BoardData | null>(() => readRouteCache<BoardData>(BOARD_ROUTE_CACHE_KEY));
  const [initialFocusTaskId] = useState(() => consumeBoardFocusTask());

  useEffect(() => {
    document.title = "";
  }, []);

  useEffect(() => {
    let cancelled = false;
    const loadingTimeout = window.setTimeout(() => {
      if (!cancelled) {
        logDesktopInitialLoadTimeout("board");
        setData((currentData) => currentData ?? createEmptyBoardData());
      }
    }, INITIAL_DESKTOP_LOAD_TIMEOUT_MS);

    Promise.all([
      getTasksByStatus(),
      getAvailableHosts(),
      getAllProjects(),
      getSidebarDefaultCollapsed(),
      getDoneAlertDismissed(),
      getNotificationSettings(),
      getDefaultSessionType(),
      getTaskSearchShortcut(),
    ]).then(([tasks, sshHosts, projects, sidebarDefaultCollapsed, doneAlertDismissed, notificationSettings, defaultSessionType, taskSearchShortcut]) => {
      window.clearTimeout(loadingTimeout);
      if (!cancelled) {
        const nextData = {
          tasks,
          sshHosts,
          projects,
          sidebarDefaultCollapsed,
          doneAlertDismissed,
          notificationSettings,
          defaultSessionType,
          taskSearchShortcut,
        };

        writeRouteCache(BOARD_ROUTE_CACHE_KEY, nextData);
        setData(nextData);
      }
    }).catch((error) => {
      window.clearTimeout(loadingTimeout);
      console.error("Failed to load board route data:", error);
      if (!cancelled) {
        setData((currentData) => currentData ?? createEmptyBoardData());
      }
    });

    return () => {
      cancelled = true;
      window.clearTimeout(loadingTimeout);
    };
  }, [refreshSignal]);

  if (!data) {
    return <BoardRouteSkeleton />;
  }

  return (
    <Board
      initialTasks={data.tasks.tasks}
      initialDoneTotal={data.tasks.doneTotal}
      initialDoneLimit={data.tasks.doneLimit}
      initialFocusTaskId={initialFocusTaskId}
      sshHosts={data.sshHosts}
      projects={data.projects}
      sidebarDefaultCollapsed={data.sidebarDefaultCollapsed}
      doneAlertDismissed={data.doneAlertDismissed}
      notificationSettings={data.notificationSettings}
      defaultSessionType={data.defaultSessionType}
      taskSearchShortcut={data.taskSearchShortcut}
    />
  );
}
