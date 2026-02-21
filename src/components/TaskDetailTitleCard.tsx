"use client";

import { useState } from "react";
import type { KanbanTask } from "@/entities/KanbanTask";
import TaskStatusBadge from "@/components/TaskStatusBadge";
import ProjectBranchTasksModal from "@/components/ProjectBranchTasksModal";

interface TaskDetailTitleCardProps {
  task: KanbanTask;
}

export default function TaskDetailTitleCard({ task }: TaskDetailTitleCardProps) {
  const [showBranchTasksModal, setShowBranchTasksModal] = useState(false);

  return (
    <>
      <div className="bg-bg-surface rounded-lg p-5 shadow-sm border border-border-default">
        <div className="flex items-center gap-2 mb-3">
          <TaskStatusBadge status={task.status} />
        </div>
        <div className="flex items-center justify-between gap-2">
          <h1 className="text-xl font-bold text-text-primary leading-tight">
            {task.title}
          </h1>
          {task.branchName && task.projectId && (
            <button
              onClick={() => setShowBranchTasksModal(true)}
              className="flex-shrink-0 w-6 h-6 rounded-full bg-brand-primary hover:opacity-80 text-white flex items-center justify-center transition-opacity"
              aria-label="View other tasks in this project"
              title="다른 작업 보기"
            >
              <svg
                width="16"
                height="16"
                viewBox="0 0 16 16"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
              >
                <path
                  d="M6 12L10 8L6 4"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </button>
          )}
        </div>
        {task.description && (
          <p className="text-sm text-text-secondary mt-3 leading-relaxed">
            {task.description}
          </p>
        )}
      </div>

      {/* 모달 */}
      {showBranchTasksModal && task.projectId && (
        <ProjectBranchTasksModal
          projectId={task.projectId}
          currentBranchName={task.branchName}
          onClose={() => setShowBranchTasksModal(false)}
        />
      )}
    </>
  );
}
