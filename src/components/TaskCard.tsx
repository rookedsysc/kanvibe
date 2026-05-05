"use client";

import { Draggable } from "@hello-pangea/dnd";
import { Link } from "@/desktop/renderer/navigation";
import type { KanbanTask } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";

interface TaskCardProps {
  task: KanbanTask;
  index: number;
  onContextMenu: (e: React.MouseEvent, task: KanbanTask) => void;
  projectName?: string;
  projectColor?: string;
  isBaseProject?: boolean;
}

const agentTagColors: Record<string, string> = {
  claude: "bg-tag-claude-bg text-tag-claude-text",
  gemini: "bg-tag-gemini-bg text-tag-gemini-text",
  codex: "bg-tag-codex-bg text-tag-codex-text",
};

const priorityConfig: Record<TaskPriority, { label: string; colorClass: string }> = {
  [TaskPriority.LOW]: { label: "P3", colorClass: "bg-priority-low-bg text-priority-low-text" },
  [TaskPriority.MEDIUM]: { label: "P2", colorClass: "bg-priority-medium-bg text-priority-medium-text" },
  [TaskPriority.HIGH]: { label: "P1", colorClass: "bg-priority-high-bg text-priority-high-text" },
};

export default function TaskCard({ task, index, onContextMenu, projectName, projectColor, isBaseProject }: TaskCardProps) {
  return (
    <Draggable draggableId={task.id} index={index}>
      {(provided, snapshot) => (
        <Link href={`/task/${task.id}`}>
          <div
            ref={provided.innerRef}
            {...provided.draggableProps}
            {...provided.dragHandleProps}
            onContextMenu={(e) => onContextMenu(e, task)}
            className={`group relative mb-1.5 overflow-hidden rounded-md px-2.5 py-2 transition-colors cursor-pointer ${
              snapshot.isDragging
                ? "bg-bg-surface shadow-md ring-1 ring-border-brand"
                : "hover:bg-bg-surface/70"
            }`}
          >
            {/* Base 프로젝트 리본 배지 */}
            {isBaseProject && (
              <div className="absolute -right-7 top-2.5 rotate-45 bg-tag-base-bg text-tag-base-text text-[10px] font-bold px-7 py-0.5 pointer-events-none select-none">
                Base
              </div>
            )}

            {projectName && (
              <div className="mb-1 flex items-center gap-1.5 pl-3.5">
                <span
                  className="h-1.5 w-1.5 shrink-0 rounded-full"
                  style={{ backgroundColor: projectColor }}
                />
                <span
                  className="truncate text-[10px] font-medium"
                  style={{ color: projectColor }}
                >
                  {projectName}
                </span>
              </div>
            )}

            <div className="flex items-start gap-2">
              <span className="mt-1 h-1.5 w-1.5 shrink-0 rounded-full bg-border-strong transition-colors group-hover:bg-brand-primary" />
              <div className="min-w-0 flex-1">
                <h3 className="truncate text-[13px] font-medium leading-5 text-text-primary">
                  {task.title}
                </h3>

                {task.description && (
                  <p className="mt-0.5 line-clamp-1 text-[11px] leading-4 text-text-muted">
                    {task.description}
                  </p>
                )}
              </div>
            </div>

            <div className="mt-1.5 flex items-center gap-1.5 overflow-hidden">
              {task.prUrl && (
                <span
                  role="link"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    window.open(task.prUrl!, "_blank", "noopener,noreferrer");
                  }}
                  className="inline-flex items-center gap-1 rounded border border-border-subtle bg-tag-pr-bg px-1.5 py-0.5 text-[10px] text-tag-pr-text transition-opacity hover:opacity-80"
                >
                  <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
                    <path d="M7.177 3.073L9.573.677A.25.25 0 0110 .854v4.792a.25.25 0 01-.427.177L7.177 3.427a.25.25 0 010-.354zM3.75 2.5a.75.75 0 100 1.5.75.75 0 000-1.5zm-2.25.75a2.25 2.25 0 113 2.122v5.256a2.251 2.251 0 11-1.5 0V5.372A2.25 2.25 0 011.5 3.25zM11 2.5h-1V4h1a1 1 0 011 1v5.628a2.251 2.251 0 101.5 0V5A2.5 2.5 0 0011 2.5zm1 10.25a.75.75 0 111.5 0 .75.75 0 01-1.5 0zM3.75 12a.75.75 0 100 1.5.75.75 0 000-1.5z"/>
                  </svg>
                  PR
                </span>
              )}

              {task.agentType && (
                <span
                  className={`rounded border border-border-subtle px-1.5 py-0.5 text-[10px] ${
                    agentTagColors[task.agentType] || "bg-tag-neutral-bg text-tag-neutral-text"
                  }`}
                >
                  {task.agentType}
                </span>
              )}

              {task.sessionType && (
                <span className="rounded border border-border-subtle bg-tag-session-bg px-1.5 py-0.5 text-[10px] text-tag-session-text">
                  {task.sessionType}
                </span>
              )}

              {task.sshHost && (
                <span className="rounded border border-border-subtle bg-tag-ssh-bg px-1.5 py-0.5 text-[10px] text-tag-ssh-text">
                  {task.sshHost}
                </span>
              )}

              {task.priority && (
                <span
                  className={`ml-auto rounded border border-border-subtle px-1.5 py-0.5 text-[10px] font-semibold ${priorityConfig[task.priority].colorClass}`}
                >
                  {priorityConfig[task.priority].label}
                </span>
              )}
            </div>
          </div>
        </Link>
      )}
    </Draggable>
  );
}
