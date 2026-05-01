import { useEffect, useState } from "react";
import Board from "@/components/Board";
import { getDoneAlertDismissed, getDefaultSessionType, getNotificationSettings, getSidebarDefaultCollapsed, getTaskSearchShortcut } from "@/desktop/renderer/actions/appSettings";
import { getTasksByStatus } from "@/desktop/renderer/actions/kanban";
import { getAllProjects, getAvailableHosts } from "@/desktop/renderer/actions/project";
import { buildRouteCacheKey, readRouteCache, writeRouteCache } from "@/desktop/renderer/utils/routeCache";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import { DEFAULT_TASK_SEARCH_SHORTCUT } from "@/desktop/renderer/utils/keyboardShortcut";
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

export default function BoardRoute() {
  const refreshSignal = useRefreshSignal(["all", "board"]);
  const [data, setData] = useState<BoardData | null>(() => readRouteCache<BoardData>(BOARD_ROUTE_CACHE_KEY));

  useEffect(() => {
    document.title = "";
  }, []);

  useEffect(() => {
    let cancelled = false;

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
      console.error("Failed to load board route data:", error);
      if (!cancelled) {
        setData((currentData) => currentData ?? createEmptyBoardData());
      }
    });

    return () => {
      cancelled = true;
    };
  }, [refreshSignal]);

  if (!data) {
    return <div className="min-h-screen flex items-center justify-center bg-bg-page text-text-muted">Loading...</div>;
  }

  return (
    <Board
      initialTasks={data.tasks.tasks}
      initialDoneTotal={data.tasks.doneTotal}
      initialDoneLimit={data.tasks.doneLimit}
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
