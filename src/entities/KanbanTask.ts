

import type { Project } from "./Project";
import type { TaskPriority } from "./TaskPriority";
export enum TaskStatus {
  TODO = "todo",
  PROGRESS = "progress",
  PENDING = "pending",
  REVIEW = "review",
  DONE = "done",
}
export const TASK_STATUS_ORDER: readonly TaskStatus[] = [
  TaskStatus.TODO,
  TaskStatus.PROGRESS,
  TaskStatus.PENDING,
  TaskStatus.REVIEW,
  TaskStatus.DONE,
];
export enum SessionType {
  TMUX = "tmux",
  ZELLIJ = "zellij",
  /** 멀티플렉서 없이 KanVibe가 PTY를 직접 소유하는 세션. 탭도 KanVibe가 관리한다 */
  TERMINAL = "terminal",
}
export interface KanbanTask {
id: string;
title: string;
description: string | null;
status: TaskStatus;
branchName: string | null;
worktreePath: string | null;
sessionType: SessionType | null;
sessionName: string | null;
sshHost: string | null;
agentType: string | null;
project: Project | null;
projectId: string | null;
baseBranch: string | null;
prUrl: string | null;
priority: TaskPriority | null;
createdAt: Date;
updatedAt: Date;
}