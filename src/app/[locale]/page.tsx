import Board from "@/components/Board";
import { getTasksByStatus } from "@/app/actions/kanban";
import { getAllProjects } from "@/app/actions/project";
import { getSidebarDefaultCollapsed } from "@/app/actions/appSettings";
import { getAvailableHosts } from "@/lib/sshConfig";

export const dynamic = "force-dynamic";

export default async function HomePage() {
  const { tasks, doneTotal, doneLimit } = await getTasksByStatus();
  const sshHosts = await getAvailableHosts();
  const projects = await getAllProjects();
  const sidebarDefaultCollapsed = await getSidebarDefaultCollapsed();

  return (
    <Board
      initialTasks={tasks}
      initialDoneTotal={doneTotal}
      initialDoneLimit={doneLimit}
      sshHosts={sshHosts}
      projects={projects}
      sidebarDefaultCollapsed={sidebarDefaultCollapsed}
    />
  );
}
