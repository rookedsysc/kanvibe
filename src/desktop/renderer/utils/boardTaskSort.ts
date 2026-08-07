import type { KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import { TaskPriority } from "@/entities/TaskPriority";
import { compareDisplayRank } from "@/desktop/shared/displayRank";
import type { BoardSortField, BoardSortKey, BoardSortPreference } from "@/desktop/shared/boardSort";

/** 우선순위 오름차순은 "높은 순"이다. 급한 것이 위로 와야 목록이 쓸모 있다 */
const PRIORITY_ORDER: Record<TaskPriority, number> = {
  [TaskPriority.HIGH]: 0,
  [TaskPriority.MEDIUM]: 1,
  [TaskPriority.LOW]: 2,
};

export interface BoardSortContext {
  /** projectId → 그 프로젝트 root task의 우선순위 */
  rootPriorityByProjectId: Map<string, TaskPriority>;
  /** projectId → 표시 이름 */
  projectNameById: Map<string, string>;
}

/** 프로젝트의 기본 브랜치를 들고 있는 task가 그 프로젝트의 root task다 */
export function isProjectRootTask(task: KanbanTask, projectLookup: Map<string, Project>): boolean {
  if (!task.projectId || !task.branchName) return false;

  const project = projectLookup.get(task.projectId);
  return Boolean(project && !project.isWorktree && task.branchName === project.defaultBranch);
}

/** 프로젝트별 root task의 우선순위를 모은다. 같은 프로젝트의 다른 task가 이 값을 기본값으로 물려받는다 */
export function buildProjectRootPriorityMap(
  tasks: KanbanTask[],
  projectLookup: Map<string, Project>,
): Map<string, TaskPriority> {
  const rootPriorityByProjectId = new Map<string, TaskPriority>();

  for (const task of tasks) {
    if (!task.priority || !task.projectId) continue;
    if (!isProjectRootTask(task, projectLookup)) continue;

    rootPriorityByProjectId.set(task.projectId, task.priority);
  }

  return rootPriorityByProjectId;
}

/**
 * task에 적용되는 우선순위를 정한다.
 * task가 자기 우선순위를 가지면 그 값이 이기고, 없으면 프로젝트 root task의 우선순위를 물려받는다.
 */
export function resolveEffectivePriority(
  task: KanbanTask,
  rootPriorityByProjectId: Map<string, TaskPriority>,
): TaskPriority | null {
  if (task.priority) return task.priority;
  if (!task.projectId) return null;

  return rootPriorityByProjectId.get(task.projectId) ?? null;
}

/** task가 우선순위를 프로젝트에서 물려받았는지. 카드에서 직접 지정한 값과 구분해 보여주기 위해 쓴다 */
export function isInheritedPriority(
  task: KanbanTask,
  rootPriorityByProjectId: Map<string, TaskPriority>,
): boolean {
  return !task.priority && resolveEffectivePriority(task, rootPriorityByProjectId) !== null;
}

function toTimestamp(value: Date | string): number {
  return value instanceof Date ? value.getTime() : new Date(value).getTime();
}

/** 정렬 기준별 비교값. 값이 없는 항목은 null로 접어 두고 방향과 무관하게 항상 뒤로 보낸다 */
function readSortValue(
  task: KanbanTask,
  field: BoardSortField,
  context: BoardSortContext,
): string | number | null {
  switch (field) {
    case "priority": {
      const priority = resolveEffectivePriority(task, context.rootPriorityByProjectId);
      return priority ? PRIORITY_ORDER[priority] : null;
    }
    case "createdAt":
      return toTimestamp(task.createdAt);
    case "updatedAt":
      return toTimestamp(task.updatedAt);
    case "title":
      return task.title;
    case "project":
      return task.projectId ? context.projectNameById.get(task.projectId) ?? null : null;
  }
}

function compareSortValues(left: string | number, right: string | number): number {
  if (typeof left === "string" && typeof right === "string") {
    return left.localeCompare(right, "ko");
  }
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

function compareBySortKeys(
  left: KanbanTask,
  right: KanbanTask,
  keys: BoardSortKey[],
  context: BoardSortContext,
): number {
  for (const { field, direction } of keys) {
    const leftValue = readSortValue(left, field, context);
    const rightValue = readSortValue(right, field, context);

    if (leftValue === null || rightValue === null) {
      if (leftValue === rightValue) continue;
      return leftValue === null ? 1 : -1;
    }

    const comparison = compareSortValues(leftValue, rightValue);
    if (comparison !== 0) {
      return direction === "desc" ? -comparison : comparison;
    }
  }

  return 0;
}

function compareByCreatedAt(left: KanbanTask, right: KanbanTask): number {
  return toTimestamp(left.createdAt) - toTimestamp(right.createdAt);
}

/**
 * 한 컬럼의 task를 정렬 설정에 따라 늘어놓는다.
 *
 * 기준이 하나도 없으면 이미 rank 순으로 들어온 목록을 그대로 두어 드래그한 자리가 유지된다.
 * 기준이 있으면 mode가 수동 배치 순서와 겹치는 방식을 정한다.
 */
export function sortTasksForBoard(
  tasks: KanbanTask[],
  preference: BoardSortPreference,
  context: BoardSortContext,
): KanbanTask[] {
  if (preference.keys.length === 0) {
    return tasks;
  }

  const sortByKeys = (candidates: KanbanTask[]) =>
    [...candidates].sort((left, right) =>
      compareBySortKeys(left, right, preference.keys, context) || compareByCreatedAt(left, right));

  if (preference.mode === "manual-off") {
    return sortByKeys(tasks);
  }

  if (preference.mode === "manual-first") {
    const pinnedTasks = tasks.filter((task) => task.isManuallyOrdered);
    const floatingTasks = tasks.filter((task) => !task.isManuallyOrdered);

    const sortedPinnedTasks = [...pinnedTasks].sort((left, right) =>
      compareDisplayRank(left.displayRank, right.displayRank) || compareByCreatedAt(left, right));

    return [...sortedPinnedTasks, ...sortByKeys(floatingTasks)];
  }

  return [...tasks].sort((left, right) =>
    compareBySortKeys(left, right, preference.keys, context)
    || compareDisplayRank(left.displayRank, right.displayRank)
    || compareByCreatedAt(left, right));
}
