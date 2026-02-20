"use client";

import { Fragment, useMemo } from "react";
import { useTranslations } from "next-intl";
import { Droppable } from "@hello-pangea/dnd";
import TaskCard from "./TaskCard";
import ProjectTaskGroup, {
  buildBranchTree,
} from "./ProjectTaskGroup";
import type { KanbanTask, TaskStatus } from "@/entities/KanbanTask";

interface ColumnProps {
  status: TaskStatus;
  tasks: KanbanTask[];
  label: string;
  colorClass: string;
  onContextMenu: (e: React.MouseEvent, task: KanbanTask) => void;
  projectNameMap: Record<string, string>;
  projectColorMap: Record<string, string>;
  projectDefaultBranchMap: Record<string, string>;
  totalCount?: number;
  hasMore?: boolean;
  onLoadMore?: () => void;
  isLoadingMore?: boolean;
}

interface TaskGroup {
  projectName: string | null;
  tasks: KanbanTask[];
  hasBranchRelations: boolean;
  /** 그룹 내 기본 브랜치(defaultBranch) 태스크가 없으면 true → 자식 전용 그룹 */
  isChildGroup: boolean;
}

/** 태스크 배열을 프로젝트별 연속 그룹으로 분할한다. DnD 순서를 유지하며 같은 프로젝트가 인접하면 하나의 그룹으로 묶는다 */
function buildContiguousGroups(
  tasks: KanbanTask[],
  projectNameMap: Record<string, string>,
  projectDefaultBranchMap: Record<string, string>,
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
      if (current) groups.push(finalizeGroup(current, projectDefaultBranchMap));
      current = { name, tasks: [task] };
    }
  }
  if (current) groups.push(finalizeGroup(current, projectDefaultBranchMap));

  return groups;
}

function finalizeGroup(
  raw: { name: string | null; tasks: KanbanTask[] },
  projectDefaultBranchMap: Record<string, string>,
): TaskGroup {
  if (!raw.name) {
    return {
      projectName: null,
      tasks: raw.tasks,
      hasBranchRelations: false,
      isChildGroup: false,
    };
  }

  /** DnD 인덱스 호환성을 위해 원래 순서를 유지하고 트리 정보만 추출한다 */
  const { treeInfo } = buildBranchTree(raw.tasks);
  const hasBranchRelations = Array.from(treeInfo.values()).some(
    (info) => info.depth > 0 || info.hasChildren,
  );

  /** 그룹 내에 기본 브랜치 태스크가 있는지 확인 */
  const hasDefaultBranch = raw.tasks.some((t) => {
    const defaultBranch = t.projectId
      ? projectDefaultBranchMap[t.projectId]
      : undefined;
    return defaultBranch && t.branchName === defaultBranch;
  });

  return {
    projectName: raw.name,
    tasks: raw.tasks,
    hasBranchRelations,
    isChildGroup: !hasDefaultBranch,
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
  projectDefaultBranchMap,
  totalCount,
  hasMore,
  onLoadMore,
  isLoadingMore,
}: ColumnProps) {
  const t = useTranslations("board");

  const groups = useMemo(
    () => buildContiguousGroups(tasks, projectNameMap, projectDefaultBranchMap),
    [tasks, projectNameMap, projectDefaultBranchMap],
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
    <div className="flex-1 min-w-[280px] max-w-[350px]">
      <div className="flex items-center gap-2 mb-3 px-2">
        <div className={`w-3 h-3 rounded-full ${colorClass}`} />
        <h2 className="text-sm font-semibold text-text-secondary uppercase tracking-wide">
          {label}
        </h2>
        <span className="text-xs text-text-muted ml-auto">
          {totalCount ?? tasks.length}
        </span>
      </div>

      <Droppable droppableId={status}>
        {(provided, snapshot) => (
          <div
            ref={provided.innerRef}
            {...provided.droppableProps}
            className={`min-h-[200px] p-2 rounded-lg transition-colors ${
              snapshot.isDraggingOver
                ? "bg-brand-subtle border border-dashed border-brand-primary"
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
                  projectName={group.projectName}
                  color={color}
                  hasBranchRelations={group.hasBranchRelations}
                  isChildGroup={group.isChildGroup}
                >
                  {group.tasks.map((task, localIdx) => (
                    <TaskCard
                      key={task.id}
                      task={task}
                      index={startIndex + localIdx}
                      onContextMenu={onContextMenu}
                      projectName={undefined}
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
                className="w-full mt-2 py-2 text-sm text-text-muted hover:text-text-secondary hover:bg-bg-surface rounded-md transition-colors disabled:opacity-50"
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
