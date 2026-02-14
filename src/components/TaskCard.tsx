"use client";

import { Draggable } from "@hello-pangea/dnd";
import { Link } from "@/i18n/navigation";
import type { KanbanTask } from "@/entities/KanbanTask";

interface TaskCardProps {
  task: KanbanTask;
  index: number;
  onContextMenu: (e: React.MouseEvent, task: KanbanTask) => void;
  projectName?: string;
}

const agentTagColors: Record<string, string> = {
  claude: "bg-tag-claude-bg text-tag-claude-text",
  gemini: "bg-tag-gemini-bg text-tag-gemini-text",
  codex: "bg-tag-codex-bg text-tag-codex-text",
};

export default function TaskCard({ task, index, onContextMenu, projectName }: TaskCardProps) {
  return (
    <Draggable draggableId={task.id} index={index}>
      {(provided, snapshot) => (
        <Link href={`/task/${task.id}`}>
          <div
            ref={provided.innerRef}
            {...provided.draggableProps}
            {...provided.dragHandleProps}
            onContextMenu={(e) => onContextMenu(e, task)}
            className={`p-3 mb-2 rounded-lg border transition-all cursor-pointer ${
              snapshot.isDragging
                ? "bg-bg-surface border-brand-primary shadow-md"
                : "bg-bg-surface border-border-default hover:border-border-strong hover:shadow-sm"
            }`}
          >
            <h3 className="text-sm font-medium text-text-primary truncate">
              {task.title}
            </h3>

            {task.description && (
              <p className="text-xs text-text-secondary mt-1 line-clamp-2 leading-relaxed">
                {task.description}
              </p>
            )}

            <div className="flex items-center gap-2 mt-2 flex-wrap">
              {projectName && (
                <span className="text-xs px-2 py-0.5 rounded-full bg-tag-project-bg text-tag-project-text font-medium truncate max-w-[120px]">
                  {projectName}
                </span>
              )}

              {!task.branchName && task.baseBranch && (
                <span className="text-xs px-2 py-0.5 rounded-full bg-tag-base-bg text-tag-base-text font-medium">
                  Base Project
                </span>
              )}

              {task.prUrl && (
                <span
                  role="link"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    window.open(task.prUrl!, "_blank", "noopener,noreferrer");
                  }}
                  className="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full bg-tag-pr-bg text-tag-pr-text cursor-pointer hover:opacity-80 transition-opacity"
                >
                  <svg width="12" height="12" viewBox="0 0 16 16" fill="currentColor">
                    <path d="M7.177 3.073L9.573.677A.25.25 0 0110 .854v4.792a.25.25 0 01-.427.177L7.177 3.427a.25.25 0 010-.354zM3.75 2.5a.75.75 0 100 1.5.75.75 0 000-1.5zm-2.25.75a2.25 2.25 0 113 2.122v5.256a2.251 2.251 0 11-1.5 0V5.372A2.25 2.25 0 011.5 3.25zM11 2.5h-1V4h1a1 1 0 011 1v5.628a2.251 2.251 0 101.5 0V5A2.5 2.5 0 0011 2.5zm1 10.25a.75.75 0 111.5 0 .75.75 0 01-1.5 0zM3.75 12a.75.75 0 100 1.5.75.75 0 000-1.5z"/>
                  </svg>
                  PR
                </span>
              )}

              {task.agentType && (
                <span
                  className={`text-xs px-2 py-0.5 rounded-full ${
                    agentTagColors[task.agentType] || "bg-tag-neutral-bg text-tag-neutral-text"
                  }`}
                >
                  {task.agentType}
                </span>
              )}

              {task.sessionType && (
                <span className="text-xs px-2 py-0.5 rounded-full bg-tag-session-bg text-tag-session-text">
                  {task.sessionType}
                </span>
              )}

              {task.sshHost && (
                <span className="text-xs px-2 py-0.5 rounded-full bg-tag-ssh-bg text-tag-ssh-text">
                  {task.sshHost}
                </span>
              )}
            </div>
          </div>
        </Link>
      )}
    </Draggable>
  );
}
