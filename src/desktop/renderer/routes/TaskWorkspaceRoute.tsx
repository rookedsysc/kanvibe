import { useEffect, useMemo, useState } from "react";
import { useLocation, useNavigationType, useSearchParams } from "react-router-dom";
import WorkspaceTaskList from "@/desktop/renderer/components/WorkspaceTaskList";
import WorkspaceTerminalPane from "@/desktop/renderer/components/WorkspaceTerminalPane";
import { getDefaultSessionType } from "@/desktop/renderer/actions/appSettings";
import { getMoreDoneTasks, getTaskById, getTasksByStatus } from "@/desktop/renderer/actions/kanban";
import { getAllProjects } from "@/desktop/renderer/actions/project";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";
import { SessionType, TASK_STATUS_ORDER, TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";

interface WorkspaceData {
  tasks: KanbanTask[];
  doneTotal: number;
  doneOffset: number;
  projects: Project[];
  defaultSessionType: SessionType;
}

function hasTerminalSession(task: KanbanTask): boolean {
  return Boolean(task.sessionType && task.sessionName);
}

/**
 * 처음 들어왔을 때 고를 task. 세션이 있는 task만 고른다.
 * 세션이 없는 task를 고르면 자동 연결이 돌아, 화면을 연 것만으로 원하지 않은 세션이 생긴다.
 */
function findInitialTaskId(tasks: KanbanTask[]): string | null {
  for (const status of TASK_STATUS_ORDER) {
    const connectedTask = tasks.find((task) => task.status === status && hasTerminalSession(task));
    if (connectedTask) return connectedTask.id;
  }

  return null;
}

/** cmux처럼 왼쪽 task 리스트에서 고른 task의 터미널을 오른쪽에 띄우는 보드 뷰 */
export default function TaskWorkspaceRoute() {
  const refreshSignal = useRefreshSignal(["all", "board"]);
  const [data, setData] = useState<WorkspaceData | null>(null);
  const [searchParams, setSearchParams] = useSearchParams();
  const requestedTaskId = searchParams.get("taskId");
  const location = useLocation();
  const navigationType = useNavigationType();
  const selectionRequestKey = requestedTaskId && navigationType === "PUSH" ? location.key : undefined;
  const [defaultTaskId, setDefaultTaskId] = useState<string | null>(null);
  const selectedTaskId = requestedTaskId && data?.tasks.some((task) => task.id === requestedTaskId)
    ? requestedTaskId
    : defaultTaskId;

  function selectTask(taskId: string) {
    setSearchParams((current) => {
      const next = new URLSearchParams(current);
      next.set("taskId", taskId);
      return next;
    }, { replace: true });
  }

  useEffect(() => {
    document.title = "";
  }, []);

  useEffect(() => {
    let cancelled = false;

    Promise.all([getTasksByStatus(), getAllProjects(), getDefaultSessionType()])
      .then(([tasksByStatus, projects, defaultSessionType]) => {
        if (cancelled) return;

        const tasks = TASK_STATUS_ORDER.flatMap((status) => tasksByStatus.tasks[status]);
        setData({ tasks, doneTotal: tasksByStatus.doneTotal, doneOffset: tasksByStatus.tasks[TaskStatus.DONE].length, projects, defaultSessionType });
        setDefaultTaskId((current) => (
          current && tasks.some((task) => task.id === current) ? current : findInitialTaskId(tasks)
        ));
      })
      .catch((error) => {
        console.error("Failed to load workspace route data:", error);
        if (!cancelled) {
          setData((current) => current ?? { tasks: [], doneTotal: 0, doneOffset: 0, projects: [], defaultSessionType: SessionType.HERDR });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [refreshSignal]);

  useEffect(() => {
    if (!data || !requestedTaskId || data.tasks.some((task) => task.id === requestedTaskId)) return;
    let cancelled = false;
    void getTaskById(requestedTaskId).then((task) => {
      if (cancelled || !task) return;
      setData((current) => current && !current.tasks.some((loaded) => loaded.id === task.id)
        ? { ...current, tasks: [...current.tasks, task] }
        : current);
    }).catch((error) => console.error("Failed to load selected workspace task:", error));
    return () => { cancelled = true; };
  }, [data, requestedTaskId]);

  const selectedTask = useMemo(
    () => data?.tasks.find((task) => task.id === selectedTaskId) ?? null,
    [data, selectedTaskId],
  );

  if (!data) {
    return <div className="h-screen bg-bg-page" data-testid="workspace-route-loading" />;
  }

  const doneOffset = data.doneOffset;
  const selectedProjectName = selectedTask?.projectId
    ? data.projects.find((project) => project.id === selectedTask.projectId)?.name ?? null
    : null;

  function replaceTask(updatedTask: KanbanTask) {
    setData((current) => current && {
      ...current,
      tasks: current.tasks.map((task) => (task.id === updatedTask.id ? { ...task, ...updatedTask } : task)),
    });
  }

  function loadMoreDoneTasks() {
    const requestedOffset = doneOffset;
    void getMoreDoneTasks(requestedOffset)
      .then(({ tasks: moreDoneTasks, doneTotal }) => {
        setData((current) => {
          if (!current) return current;

          const loadedTaskIds = new Set(current.tasks.map((task) => task.id));
          const newDoneTasks = moreDoneTasks.filter((task) => !loadedTaskIds.has(task.id));
          return { ...current, tasks: [...current.tasks, ...newDoneTasks], doneOffset: Math.max(current.doneOffset, requestedOffset + moreDoneTasks.length), doneTotal };
        });
      })
      .catch((error) => {
        console.error("Failed to load more done tasks:", error);
      });
  }

  return (
    <div data-workspace-view="true" className="flex h-screen overflow-hidden bg-bg-page">
      <WorkspaceTaskList
        tasks={data.tasks}
        projects={data.projects}
        selectedTaskId={selectedTaskId}
        selectionRequestKey={selectionRequestKey}
        onSelectTask={selectTask}
        hasMoreDoneTasks={data.doneOffset < data.doneTotal}
        onLoadMoreDoneTasks={loadMoreDoneTasks}
      />
      <WorkspaceTerminalPane
        task={selectedTask}
        projectName={selectedProjectName}
        defaultSessionType={data.defaultSessionType}
        onTaskConnected={replaceTask}
      />
    </div>
  );
}
