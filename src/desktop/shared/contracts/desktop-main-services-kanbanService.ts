import type { KanbanTask, TaskStatus, SessionType } from "@/entities/KanbanTask";
import type { TaskPriority } from "@/entities/TaskPriority";
import type { BackgroundSyncFailurePayload, TaskPrMergedDetectedPayload } from "@/desktop/shared/contracts/lib-boardNotifier";

export type TasksByStatus = Record<TaskStatus, KanbanTask[]>;

export interface TasksByStatusWithMeta {
  tasks: TasksByStatus;
  doneTotal: number;
  doneLimit: number;
}

export interface LoadMoreDoneResponse {
  tasks: KanbanTask[];
  doneTotal: number;
}

export interface SearchableTask {
  id: string;
  title: string;
  branchName: string | null;
  projectId: string | null;
  projectName: string | null;
  sshHost: string | null;
  status: TaskStatus;
  updatedAt: Date;
}

export interface ActiveTaskPullRequestSyncResult {
  updatedTaskIds: string[];
  mergeEventKeys: string[];
  mergedPullRequests: TaskPrMergedDetectedPayload[];
  failures?: BackgroundSyncFailurePayload[];
}

export interface TaskPullSyncPayload {
  taskId: string;
  taskTitle: string;
  branchName: string;
  worktreePath: string;
  sshHost: string | null;
  status: "updated" | "failed";
  summary: string;
}

export interface ActiveTaskPullSyncResult {
  pulledTasks: TaskPullSyncPayload[];
}

export interface CreateTaskInput {
  title: string;
  description?: string;
  branchName?: string;
  baseBranch?: string;
  sessionType?: SessionType;
  sshHost?: string;
  projectId?: string;
  priority?: TaskPriority;
}

export interface SessionTypeChangeTargets {
  taskCount: number;
  liveSessionCount: number;
  /** 새 세션 도구를 확인해야 하는 호스트. null은 로컬이다 */
  sshHosts: (string | null)[];
}

export interface SessionTypeChangeResult {
  changedCount: number;
  failedCount: number;
}
