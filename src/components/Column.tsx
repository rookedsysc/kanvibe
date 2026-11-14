"use client";

import { Fragment, useMemo } from "react";
import { useTranslations } from "next-intl";
import { Droppable } from "@hello-pangea/dnd";
import TaskCard, { type TaskDeletionState, type TaskMoveState } from "./TaskCard";
import ProjectTaskGroup from "./ProjectTaskGroup";
import InlineLoadError from "./InlineLoadError";
import { useFlipReflow } from "@/desktop/renderer/hooks/useFlipReflow";
import type { UnreadNotificationCountByTask } from "@/desktop/renderer/hooks/useUnreadNotificationCountByTask";
import type { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import type { TaskPriority } from "@/entities/TaskPriority";
import type { RunningAgentPane } from "@/lib/aiSessions/types";
import type { TaskDiffStats } from "@/desktop/shared/taskDiffStats";

interface ColumnProps {
  status: TaskStatus;
  tasks: KanbanTask[];
  label: string;
  colorClass: string;
  onContextMenu: (task: KanbanTask, position: { x: number; y: number }) => void;
  projectNameMap: Record<string, string>;
  projectGroupMap: Record<string, string>;
  projectColorMap: Record<string, string>;
  projectIconMap: Record<string, string>;
  unreadNotificationCountByTask?: UnreadNotificationCountByTask;
  rootPriorityByProjectId?: Map<string, TaskPriority>;
  totalCount?: number;
  hasMore?: boolean;
  onLoadMore?: () => void;
  isLoadingMore?: boolean;
  /** 더 불러오기가 실패한 사유. 있으면 버튼 아래에 알리고 다시 시도는 같은 페이지를 요청한다 */
  loadMoreError?: string | null;
  vimModeEnabled?: boolean;
  /** 보드가 한 번만 조회해 모든 카드가 나눠 쓰는 실행중 에이전트 목록 */
  runningAgentPanes?: RunningAgentPane[];
  /** 보드가 한 번만 조회해 카드마다 나눠 쓰는 태스크별 변경 집계 */
  diffStatsByTaskId?: Record<string, TaskDiffStats>;
  /** 보드가 관리하는 task별 삭제 요청 상태 */
  taskDeletionStateById?: Record<string, TaskDeletionState>;
  onRetryTaskDelete?: (task: KanbanTask) => void;
  /** 보드가 관리하는 task별 이동 저장 상태 */
  taskMoveStateById?: Record<string, TaskMoveState>;
  onRetryTaskMove?: (task: KanbanTask) => void;
}

interface TaskGroup {
  projectKey: string | null;
  projectName: string | null;
  tasks: KanbanTask[];
}

/** 태스크 배열을 프로젝트별 연속 그룹으로 분할한다. DnD 순서를 유지하며 같은 프로젝트가 인접하면 하나의 그룹으로 묶는다 */
function buildContiguousGroups(
  tasks: KanbanTask[],
  projectNameMap: Record<string, string>,
  projectGroupMap: Record<string, string>,
): TaskGroup[] {
  const groups: TaskGroup[] = [];
  let current: { key: string | null; name: string | null; tasks: KanbanTask[] } | null = null;

  for (const task of tasks) {
    const name = task.projectId
      ? (projectNameMap[task.projectId] ?? null)
      : null;

    const key = task.projectId ? (projectGroupMap[task.projectId] ?? task.projectId) : null;

    if (current && current.key === key) {
      current.tasks.push(task);
    } else {
      if (current) groups.push(finalizeGroup(current));
      current = { key, name, tasks: [task] };
    }
  }
  if (current) groups.push(finalizeGroup(current));

  return groups;
}

function finalizeGroup(
  raw: { key: string | null; name: string | null; tasks: KanbanTask[] },
): TaskGroup {
  if (!raw.name) {
    return {
      projectKey: raw.key,
      projectName: null,
      tasks: raw.tasks,
    };
  }

  return {
    projectKey: raw.key,
    projectName: raw.name,
    tasks: raw.tasks,
  };
}


export default function Column({
  status,
  tasks,
  label,
  colorClass,
  onContextMenu,
  projectNameMap,
  projectGroupMap,
  projectColorMap,
  projectIconMap,
  unreadNotificationCountByTask,
  rootPriorityByProjectId,
  totalCount,
  hasMore,
  onLoadMore,
  isLoadingMore,
  loadMoreError,
  vimModeEnabled = true,
  runningAgentPanes,
  diffStatsByTaskId,
  taskDeletionStateById,
  onRetryTaskDelete,
  taskMoveStateById,
  onRetryTaskMove,
}: ColumnProps) {
  const t = useTranslations("board");
  const tc = useTranslations("common");
  const columnRef = useFlipReflow<HTMLDivElement>(tasks.map((task) => task.id).join(","));

  const groups = useMemo(
    () => buildContiguousGroups(tasks, projectNameMap, projectGroupMap),
    [tasks, projectNameMap, projectGroupMap],
  );

  /** 그룹 간 연속적인 DnD 인덱스를 부여하기 위한 시작 인덱스 배열 */
  const groupStartIndices = useMemo(() => {
    const indices: number[] = [];
    let offset = 0;
    for (const group of groups) {
      indices.push(offset);
      offset += group.tasks.length;
    }
    return indices;
  }, [groups]);

  return (
    <div ref={columnRef} className="flex-1 min-w-[284px] max-w-[340px] overflow-hidden rounded-lg border border-border-default bg-transparent">
      <div className="flex h-10 items-center gap-2 border-b border-border-subtle px-3">
        <div className={`h-2 w-2 rounded-full ${colorClass}`} />
        <h2 className="text-[11px] font-semibold text-text-secondary uppercase">
          {label}
        </h2>
        <span className="ml-auto px-1.5 py-0.5 text-[11px] text-text-muted">
          {totalCount ?? tasks.length}
        </span>
      </div>

      <Droppable droppableId={status}>
        {(provided, snapshot) => (
          <div
            ref={provided.innerRef}
            {...provided.droppableProps}
            className={`min-h-[calc(100vh-132px)] p-2 transition-colors ${
              snapshot.isDraggingOver
                ? "bg-brand-subtle ring-1 ring-inset ring-border-brand"
                : "bg-transparent"
            }`}
          >
            {groups.map((group, groupIdx) => {
              const startIndex = groupStartIndices[groupIdx];

              /** 프로젝트 없는 태스크: 그룹 래퍼 없이 직접 렌더링 */
              if (!group.projectName) {
                return (
                  <Fragment key={`ungrouped-${groupIdx}`}>
                    {group.tasks.map((task, localIdx) => (
                      <TaskCard
                        key={task.id}
                        task={task}
                        index={startIndex + localIdx}
                        onContextMenu={onContextMenu}
                        projectName={undefined}
                        projectColor={undefined}
                        unreadNotificationCount={unreadNotificationCountByTask?.[task.id] ?? 0}
                        isBaseProject={
                          !!task.worktreePath &&
                          !task.worktreePath.includes("__worktrees")
                        }
                        rootPriorityByProjectId={rootPriorityByProjectId}
                        vimModeEnabled={vimModeEnabled}
                        runningAgentPanes={runningAgentPanes}
                        diffStats={diffStatsByTaskId?.[task.id]}
                        deletionState={taskDeletionStateById?.[task.id]}
                        onRetryDelete={onRetryTaskDelete}
                        moveState={taskMoveStateById?.[task.id]}
                        onRetryMove={onRetryTaskMove}
                      />
                    ))}
                  </Fragment>
                );
              }

              const projectId = group.tasks[0].projectId ?? "";
              const color = projectColorMap[projectId] ?? "#93C5FD";
              const iconDataUrl = projectIconMap[projectId] ?? null;

              return (
                <ProjectTaskGroup
                  key={`${group.projectKey}-${groupIdx}`}
                >
                  {group.tasks.map((task, localIdx) => (
                    <TaskCard
                      key={task.id}
                      task={task}
                      index={startIndex + localIdx}
                      onContextMenu={onContextMenu}
                      projectName={group.projectName ?? undefined}
                      projectColor={color}
                      projectIconDataUrl={iconDataUrl}
                      unreadNotificationCount={unreadNotificationCountByTask?.[task.id] ?? 0}
                      isBaseProject={
                        !!task.worktreePath &&
                        !task.worktreePath.includes("__worktrees")
                      }
                      rootPriorityByProjectId={rootPriorityByProjectId}
                      vimModeEnabled={vimModeEnabled}
                      runningAgentPanes={runningAgentPanes}
                      diffStats={diffStatsByTaskId?.[task.id]}
                      deletionState={taskDeletionStateById?.[task.id]}
                      onRetryDelete={onRetryTaskDelete}
                      moveState={taskMoveStateById?.[task.id]}
                      onRetryMove={onRetryTaskMove}
                    />
                  ))}
                </ProjectTaskGroup>
              );
            })}
            {provided.placeholder}
            {hasMore && onLoadMore && (
              <button
                onClick={onLoadMore}
                disabled={isLoadingMore}
                className="mt-2 w-full rounded-md border border-border-subtle py-2 text-xs text-text-muted transition-colors hover:bg-bg-page hover:text-text-secondary disabled:opacity-50"
              >
                {isLoadingMore ? t("loadingMore") : t("loadMore")}
              </button>
            )}
            {hasMore && onLoadMore && loadMoreError ? (
              <InlineLoadError
                className="mt-2"
                message={tc("loadFailed", { reason: loadMoreError })}
                onRetry={onLoadMore}
              />
            ) : null}
          </div>
        )}
      </Droppable>
    </div>
  );
}
