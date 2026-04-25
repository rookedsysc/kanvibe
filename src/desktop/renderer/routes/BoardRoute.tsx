import { useEffect, useState } from "react";
import Board from "@/components/Board";
import { getDoneAlertDismissed, getDefaultSessionType, getNotificationSettings, getSidebarDefaultCollapsed } from "@/desktop/renderer/actions/appSettings";
import { getTasksByStatus } from "@/desktop/renderer/actions/kanban";
import { getAllProjects, getAvailableHosts } from "@/desktop/renderer/actions/project";
import { buildRouteCacheKey, readRouteCache, writeRouteCache } from "@/desktop/renderer/utils/routeCache";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";

interface BoardData {
  tasks: Awaited<ReturnType<typeof getTasksByStatus>>;
  sshHosts: string[];
  projects: Awaited<ReturnType<typeof getAllProjects>>;
  sidebarDefaultCollapsed: boolean;
  doneAlertDismissed: boolean;
  notificationSettings: Awaited<ReturnType<typeof getNotificationSettings>>;
  defaultSessionType: Awaited<ReturnType<typeof getDefaultSessionType>>;
}

const BOARD_ROUTE_CACHE_KEY = buildRouteCacheKey("board");

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
    ]).then(([tasks, sshHosts, projects, sidebarDefaultCollapsed, doneAlertDismissed, notificationSettings, defaultSessionType]) => {
      if (!cancelled) {
        const nextData = {
          tasks,
          sshHosts,
          projects,
          sidebarDefaultCollapsed,
          doneAlertDismissed,
          notificationSettings,
          defaultSessionType,
        };

        writeRouteCache(BOARD_ROUTE_CACHE_KEY, nextData);
        setData(nextData);
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
    />
  );
}
