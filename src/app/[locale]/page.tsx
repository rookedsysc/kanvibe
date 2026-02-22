import Board from "@/components/Board";
import { getTasksByStatus } from "@/app/actions/kanban";
import { getAllProjects } from "@/app/actions/project";
import { getSidebarDefaultCollapsed, getDoneAlertDismissed, getNotificationSettings } from "@/app/actions/appSettings";
import { getAvailableHosts } from "@/lib/sshConfig";

export const dynamic = "force-dynamic";

export default async function HomePage() {
  const [{ tasks, doneTotal, doneLimit }, sshHosts, projects, sidebarDefaultCollapsed, doneAlertDismissed, notificationSettings] =
    await Promise.all([
      getTasksByStatus(),
      getAvailableHosts(),
      getAllProjects(),
      getSidebarDefaultCollapsed(),
      getDoneAlertDismissed(),
      getNotificationSettings(),
    ]);

  return (
    <Board
      initialTasks={tasks}
      initialDoneTotal={doneTotal}
      initialDoneLimit={doneLimit}
      sshHosts={sshHosts}
      projects={projects}
      sidebarDefaultCollapsed={sidebarDefaultCollapsed}
      doneAlertDismissed={doneAlertDismissed}
      notificationSettings={notificationSettings}
    />
  );
}
