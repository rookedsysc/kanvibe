"use client";

import { Fragment, useMemo } from "react";
import { useTranslations } from "next-intl";
import { Droppable } from "@hello-pangea/dnd";
import TaskCard from "./TaskCard";
import ProjectTaskGroup from "./ProjectTaskGroup";
import type { KanbanTask, TaskStatus } from "@/entities/KanbanTask";

interface ColumnProps {
  status: TaskStatus;
  tasks: KanbanTask[];
  label: string;
  colorClass: string;
  onContextMenu: (e: React.MouseEvent, task: KanbanTask) => void;
  projectNameMap: Record<string, string>;
  projectColorMap: Record<string, string>;
  totalCount?: number;
  hasMore?: boolean;
  onLoadMore?: () => void;
  isLoadingMore?: boolean;
}

interface TaskGroup {
  projectName: string | null;
  tasks: KanbanTask[];
}

/** 태스크 배열을 프로젝트별 연속 그룹으로 분할한다. DnD 순서를 유지하며 같은 프로젝트가 인접하면 하나의 그룹으로 묶는다 */
function buildContiguousGroups(
  tasks: KanbanTask[],
  projectNameMap: Record<string, string>,
): TaskGroup[] {
  const groups: TaskGroup[] = [];
  let current: { name: string | null; tasks: KanbanTask[] } | null = null;

  for (const task of tasks) {
    const name = task.projectId
      ? (projectNameMap[task.projectId] ?? null)
      : null;

    if (current && current.name === name) {
      current.tasks.push(task);
    } else {
      if (current) groups.push(finalizeGroup(current));
      current = { name, tasks: [task] };
    }
  }
  if (current) groups.push(finalizeGroup(current));

  return groups;
}

function finalizeGroup(
  raw: { name: string | null; tasks: KanbanTask[] },
): TaskGroup {
  if (!raw.name) {
    return {
      projectName: null,
      tasks: raw.tasks,
    };
  }

  return {
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
  projectColorMap,
  totalCount,
  hasMore,
  onLoadMore,
  isLoadingMore,
}: ColumnProps) {
  const t = useTranslations("board");

  const groups = useMemo(
    () => buildContiguousGroups(tasks, projectNameMap),
    [tasks, projectNameMap],
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
    <div className="flex-1 min-w-[284px] max-w-[340px]">
      <div className="flex h-10 items-center gap-2 px-2">
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
                ? "rounded-lg bg-brand-subtle ring-1 ring-inset ring-border-brand"
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
                        isBaseProject={
                          !!task.worktreePath &&
                          !task.worktreePath.includes("__worktrees")
                        }
                      />
                    ))}
                  </Fragment>
                );
              }

              const color = projectColorMap[group.projectName] ?? "#93C5FD";

              return (
                <ProjectTaskGroup
                  key={`${group.projectName}-${groupIdx}`}
                >
                  {group.tasks.map((task, localIdx) => (
                    <TaskCard
                      key={task.id}
                      task={task}
                      index={startIndex + localIdx}
                      onContextMenu={onContextMenu}
                      projectName={group.projectName ?? undefined}
                      projectColor={color}
                      isBaseProject={
                        !!task.worktreePath &&
                        !task.worktreePath.includes("__worktrees")
                      }
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
          </div>
        )}
      </Droppable>
    </div>
  );
}
