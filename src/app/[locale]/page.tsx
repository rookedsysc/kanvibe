"use client";

import { useState, useEffect } from "react";
import Board from "@/components/Board";
import { ipcKanban, ipcProject, ipcSettings, ipcApp } from "@/lib/ipc";
import type { TasksByStatus } from "@/lib/ipc";
import type { Project } from "@/entities/Project";
import type { SessionType } from "@/entities/KanbanTask";

export default function HomePage() {
  const [loading, setLoading] = useState(true);
  const [tasks, setTasks] = useState<TasksByStatus | null>(null);
  const [doneTotal, setDoneTotal] = useState(0);
  const [doneLimit, setDoneLimit] = useState(20);
  const [sshHosts, setSshHosts] = useState<string[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [sidebarDefaultCollapsed, setSidebarDefaultCollapsed] = useState(false);
  const [doneAlertDismissed, setDoneAlertDismissed] = useState(false);
  const [notificationSettings, setNotificationSettings] = useState({
    isEnabled: true,
    enabledStatuses: ["progress", "pending", "review"],
  });
  const [defaultSessionType, setDefaultSessionType] = useState<SessionType | null>(null);

  useEffect(() => {
    async function load() {
      try {
        const [tasksResult, hosts, projs, collapsed, dismissed, notifSettings, sessionType] =
          await Promise.all([
            ipcKanban.getTasksByStatus(),
            ipcApp.getAvailableHosts(),
            ipcProject.getAll(),
            ipcSettings.getSidebarDefaultCollapsed(),
            ipcSettings.getDoneAlertDismissed(),
            ipcSettings.getNotificationSettings(),
            ipcSettings.getDefaultSessionType(),
          ]);

        setTasks(tasksResult.tasks);
        setDoneTotal(tasksResult.doneTotal);
        setDoneLimit(tasksResult.doneLimit);
        setSshHosts(hosts);
        setProjects(projs);
        setSidebarDefaultCollapsed(collapsed);
        setDoneAlertDismissed(dismissed);
        setNotificationSettings(notifSettings);
        setDefaultSessionType(sessionType);
      } catch (error) {
        console.error("초기 데이터 로딩 실패:", error);
      } finally {
        setLoading(false);
      }
    }
    load();
  }, []);

  if (loading || !tasks || !defaultSessionType) {
    return (
      <div className="h-screen flex items-center justify-center bg-bg-page">
        <div className="animate-spin rounded-full h-8 w-8 border-2 border-brand-primary border-t-transparent" />
      </div>
    );
  }

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
      defaultSessionType={defaultSessionType}
    />
  );
}
