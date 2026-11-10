import type { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import type { SessionType } from "@/entities/KanbanTask";
import type {
  LoadMoreDoneResponse,
  TasksByStatus,
  TasksByStatusWithMeta,
  CreateTaskInput,
  SearchableTask,
  SessionTypeChangeResult,
  SessionTypeChangeTargets,
} from "@/desktop/shared/contracts/desktop-main-services-kanbanService";
import { invokeDesktop } from "@/desktop/renderer/ipc";

export type {
  TasksByStatus,
  TasksByStatusWithMeta,
  LoadMoreDoneResponse,
  CreateTaskInput,
  SearchableTask,
  SessionTypeChangeResult,
  SessionTypeChangeTargets,
};

export function getTasksByStatus(): Promise<TasksByStatusWithMeta> {
  return invokeDesktop("kanban", "getTasksByStatus");
}

export function getMoreDoneTasks(offset: number, limit?: number): Promise<LoadMoreDoneResponse> {
  return invokeDesktop("kanban", "getMoreDoneTasks", offset, limit);
}

export function getTaskById(taskId: string): Promise<KanbanTask | null> {
  return invokeDesktop("kanban", "getTaskById", taskId);
}

export function getSearchableTasks(): Promise<SearchableTask[]> {
  return invokeDesktop("kanban", "getSearchableTasks");
}

export function getTaskIdByProjectAndBranch(projectId: string, branchName: string): Promise<string | null> {
  return invokeDesktop("kanban", "getTaskIdByProjectAndBranch", projectId, branchName);
}

export function createTask(input: CreateTaskInput): Promise<KanbanTask> {
  return invokeDesktop("kanban", "createTask", input);
}

export function updateTaskStatus(taskId: string, newStatus: TaskStatus): Promise<KanbanTask | null> {
  return invokeDesktop("kanban", "updateTaskStatus", taskId, newStatus);
}

export function updateTask(
  taskId: string,
  updates: Partial<Pick<KanbanTask, "title" | "description" | "priority">>,
): Promise<KanbanTask | null> {
  return invokeDesktop("kanban", "updateTask", taskId, updates);
}

export function updateProjectColor(projectId: string, color: string): Promise<void> {
  return invokeDesktop("kanban", "updateProjectColor", projectId, color);
}

export function deleteTask(taskId: string): Promise<boolean> {
  return invokeDesktop("kanban", "deleteTask", taskId);
}

export function deleteTasks(taskIds: string[]): Promise<string[]> {
  return invokeDesktop("kanban", "deleteTasks", taskIds);
}

export function branchFromTask(
  taskId: string,
  projectId: string,
  baseBranch: string,
  branchName: string,
  sessionType: SessionType,
): Promise<KanbanTask | null> {
  return invokeDesktop("kanban", "branchFromTask", taskId, projectId, baseBranch, branchName, sessionType);
}

export function connectTerminalSession(taskId: string, sessionType: SessionType): Promise<KanbanTask | null> {
  return invokeDesktop("kanban", "connectTerminalSession", taskId, sessionType);
}

export function isTaskSessionAlive(taskId: string): Promise<boolean> {
  return invokeDesktop("kanban", "isTaskSessionAlive", taskId);
}

export function changeTaskSessionType(
  taskId: string,
  sessionType: SessionType,
  shouldCloseExistingSession: boolean,
): Promise<KanbanTask | null> {
  return invokeDesktop("kanban", "changeTaskSessionType", taskId, sessionType, shouldCloseExistingSession);
}

export function getSessionTypeChangeTargets(sessionType: SessionType): Promise<SessionTypeChangeTargets> {
  return invokeDesktop("kanban", "getSessionTypeChangeTargets", sessionType);
}

export function changeAllTaskSessionTypes(
  sessionType: SessionType,
  shouldCloseExistingSessions: boolean,
): Promise<SessionTypeChangeResult> {
  return invokeDesktop("kanban", "changeAllTaskSessionTypes", sessionType, shouldCloseExistingSessions);
}

export function moveTaskToColumn(taskId: string, newStatus: TaskStatus): Promise<void> {
  return invokeDesktop("kanban", "moveTaskToColumn", taskId, newStatus);
}

export function fetchAndSavePrUrl(taskId: string): Promise<string | null> {
  return invokeDesktop("kanban", "fetchAndSavePrUrl", taskId);
}
