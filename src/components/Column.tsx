"use client";

import { useTranslations } from "next-intl";
import { Droppable } from "@hello-pangea/dnd";
import TaskCard from "./TaskCard";
import type { KanbanTask, TaskStatus } from "@/entities/KanbanTask";

interface ColumnProps {
  status: TaskStatus;
  tasks: KanbanTask[];
  label: string;
  colorClass: string;
  onContextMenu: (e: React.MouseEvent, task: KanbanTask) => void;
  projectNameMap: Record<string, string>;
  totalCount?: number;
  hasMore?: boolean;
  onLoadMore?: () => void;
  isLoadingMore?: boolean;
}

export default function Column({ status, tasks, label, colorClass, onContextMenu, projectNameMap, totalCount, hasMore, onLoadMore, isLoadingMore }: ColumnProps) {
  const t = useTranslations("board");

  return (
    <div className="flex-1 min-w-[280px] max-w-[350px]">
      <div className="flex items-center gap-2 mb-3 px-2">
        <div className={`w-3 h-3 rounded-full ${colorClass}`} />
        <h2 className="text-sm font-semibold text-text-secondary uppercase tracking-wide">
          {label}
        </h2>
        <span className="text-xs text-text-muted ml-auto">{totalCount ?? tasks.length}</span>
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
            {tasks.map((task, index) => (
              <TaskCard
                key={task.id}
                task={task}
                index={index}
                onContextMenu={onContextMenu}
                projectName={task.projectId ? projectNameMap[task.projectId] : undefined}
                isBaseProject={!!task.worktreePath && !task.worktreePath.includes("__worktrees")}
              />
            ))}
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
