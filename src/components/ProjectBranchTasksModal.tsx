"use client";

import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { useRouter } from "@/i18n/navigation";
import { getTasksByStatus } from "@/app/actions/kanban";
import { TaskStatus } from "@/entities/KanbanTask";
import type { KanbanTask } from "@/entities/KanbanTask";

interface ProjectBranchTasksModalProps {
  projectId: string;
  currentBranchName: string | null;
  onClose: () => void;
}

const COLUMNS = [
  { status: TaskStatus.PROGRESS, labelKey: "progress", colorClass: "bg-status-progress" },
  { status: TaskStatus.PENDING, labelKey: "pending", colorClass: "bg-purple-500" },
  { status: TaskStatus.REVIEW, labelKey: "review", colorClass: "bg-status-review" },
];

export default function ProjectBranchTasksModal({
  projectId,
  currentBranchName,
  onClose,
}: ProjectBranchTasksModalProps) {
  const t = useTranslations("board.columns");
  const tc = useTranslations("common");
  const router = useRouter();
  const [tasksByStatus, setTasksByStatus] = useState<Record<TaskStatus, KanbanTask[]>>({
    [TaskStatus.TODO]: [],
    [TaskStatus.PROGRESS]: [],
    [TaskStatus.PENDING]: [],
    [TaskStatus.REVIEW]: [],
    [TaskStatus.DONE]: [],
  }); // 모달에서는 TODO, PENDING, REVIEW만 표시
  const [isLoading, setIsLoading] = useState(true);

  // 같은 프로젝트의 작업 로드
  useEffect(() => {
    (async () => {
      try {
        const result = await getTasksByStatus();
        // 같은 projectId로 필터링
        const filtered: typeof tasksByStatus = {
          [TaskStatus.TODO]: [],
          [TaskStatus.PROGRESS]: [],
          [TaskStatus.PENDING]: [],
          [TaskStatus.REVIEW]: [],
          [TaskStatus.DONE]: [],
        };

        Object.entries(result.tasks).forEach(([status, tasks]) => {
          filtered[status as TaskStatus] = tasks.filter(
            (task) => task.projectId === projectId
          );
        });

        setTasksByStatus(filtered);
      } catch (error) {
        console.error("Failed to load tasks:", error);
      } finally {
        setIsLoading(false);
      }
    })();
  }, [projectId]);

  const handleTaskClick = (taskId: string) => {
    router.push(`/task/${taskId}`);
    onClose();
  };

  const totalTasks = Object.values(tasksByStatus).reduce((sum, tasks) => sum + tasks.length, 0);

  return (
    <div
      className="fixed inset-0 z-[400] flex items-center justify-center bg-bg-overlay"
      onClick={onClose}
    >
      <div
        className="w-full max-w-6xl max-h-[85vh] overflow-hidden bg-bg-page rounded-xl border border-border-default shadow-lg flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        {/* 헤더 */}
        <div className="flex items-center justify-between gap-2 p-6 border-b border-border-subtle bg-bg-surface">
          <div>
            <h2 className="text-lg font-semibold text-text-primary">프로젝트 작업</h2>
            <p className="text-xs text-text-secondary mt-1">총 {totalTasks}개</p>
          </div>
          <button
            onClick={onClose}
            className="text-text-secondary hover:text-text-primary transition-colors"
            aria-label="Close"
          >
            <svg
              width="20"
              height="20"
              viewBox="0 0 16 16"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
            >
              <path d="M12 4L4 12M4 4L12 12" strokeLinecap="round" />
            </svg>
          </button>
        </div>

        {/* 본문 - Kanban 보드 */}
        {isLoading ? (
          <div className="flex-1 flex items-center justify-center">
            <p className="text-sm text-text-secondary">로딩 중...</p>
          </div>
        ) : (
          <div className="flex-1 overflow-x-auto p-6">
            <div className="flex gap-4 min-w-full">
              {COLUMNS.map((column) => (
                <div key={column.status} className="flex-shrink-0 w-80">
                  {/* 칼럼 헤더 */}
                  <div className="flex items-center gap-2 mb-3 pb-3 border-b border-border-subtle">
                    <div className={`w-2 h-2 rounded-full ${column.colorClass}`} />
                    <h3 className="text-sm font-semibold text-text-primary">
                      {t(column.labelKey)}
                    </h3>
                    <span className="ml-auto text-xs text-text-muted">
                      {tasksByStatus[column.status].length}
                    </span>
                  </div>

                  {/* 작업 목록 */}
                  <div className="space-y-2">
                    {tasksByStatus[column.status].length === 0 ? (
                      <div className="text-xs text-text-muted text-center py-8">
                        작업 없음
                      </div>
                    ) : (
                      tasksByStatus[column.status].map((task) => (
                        <div
                          key={task.id}
                          className="p-3 rounded-lg bg-bg-surface border border-border-default hover:border-brand-primary hover:shadow-sm transition-all group flex items-start justify-between gap-2"
                        >
                          <button
                            onClick={() => handleTaskClick(task.id)}
                            className="flex-1 text-left"
                          >
                            <h4 className="text-sm font-medium text-text-primary group-hover:text-brand-primary transition-colors truncate">
                              {task.title}
                            </h4>
                            {task.branchName && (
                              <p className="text-xs text-text-secondary mt-1 truncate">
                                {task.branchName}
                              </p>
                            )}
                          </button>
                          <button
                            onClick={() => handleTaskClick(task.id)}
                            className="flex-shrink-0 w-6 h-6 rounded-full bg-brand-primary hover:opacity-80 text-white flex items-center justify-center transition-opacity"
                            aria-label="Open task"
                            title={task.title}
                          >
                            <svg
                              width="12"
                              height="12"
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
                        </div>
                      ))
                    )}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
