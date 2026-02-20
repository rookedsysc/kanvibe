"use client";

import { useState, useRef, useEffect, useTransition } from "react";
import { useRouter } from "@/i18n/navigation";
import { useTranslations } from "next-intl";
import type { KanbanTask } from "@/entities/KanbanTask";
import { updateTask } from "@/app/actions/kanban";
import TaskStatusBadge from "@/components/TaskStatusBadge";
import ProjectBranchTasksModal from "@/components/ProjectBranchTasksModal";

interface TaskDetailTitleCardProps {
  task: KanbanTask;
  taskId: string;
}

export default function TaskDetailTitleCard({ task, taskId }: TaskDetailTitleCardProps) {
  const [showBranchTasksModal, setShowBranchTasksModal] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [draft, setDraft] = useState(task.description ?? "");
  const [isPending, startTransition] = useTransition();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const router = useRouter();
  const t = useTranslations("taskDetail");
  const tc = useTranslations("common");

  /** 편집 모드 진입 시 textarea에 포커스하고 커서를 끝으로 이동 */
  useEffect(() => {
    if (isEditing && textareaRef.current) {
      textareaRef.current.focus();
      textareaRef.current.selectionStart = textareaRef.current.value.length;
    }
  }, [isEditing]);

  function handleStartEditing() {
    setDraft(task.description ?? "");
    setIsEditing(true);
  }

  function handleCancel() {
    setIsEditing(false);
    setDraft(task.description ?? "");
  }

  function handleSave() {
    const trimmed = draft.trim();
    const newDescription = trimmed || null;

    if (newDescription === task.description) {
      setIsEditing(false);
      return;
    }

    startTransition(async () => {
      await updateTask(taskId, { description: newDescription });
      setIsEditing(false);
      router.refresh();
    });
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Escape") {
      handleCancel();
    }
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      handleSave();
    }
  }

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
              className="flex-shrink-0 w-6 h-6 rounded-full bg-tag-project-bg hover:opacity-80 text-tag-project-text flex items-center justify-center transition-opacity"
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

        {/* description 인라인 수정 영역 */}
        <div className={isPending ? "opacity-50 pointer-events-none mt-3" : "mt-3"}>
          {isEditing ? (
            <div>
              <textarea
                ref={textareaRef}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={handleKeyDown}
                rows={4}
                className="w-full text-sm text-text-primary bg-bg-page border border-border-default rounded-md p-2 leading-relaxed resize-y focus:outline-none focus:border-brand-primary"
              />
              <div className="flex justify-end gap-2 mt-2">
                <button
                  type="button"
                  onClick={handleCancel}
                  className="px-3 py-1.5 text-xs text-text-secondary bg-bg-page border border-border-default rounded-md hover:border-border-strong transition-colors"
                >
                  {tc("cancel")}
                </button>
                <button
                  type="button"
                  onClick={handleSave}
                  className="px-3 py-1.5 text-xs text-white bg-brand-primary rounded-md hover:opacity-90 transition-opacity"
                >
                  {tc("save")}
                </button>
              </div>
            </div>
          ) : (
            <button
              type="button"
              onClick={handleStartEditing}
              className="w-full text-left cursor-pointer group"
            >
              {task.description ? (
                <p className="text-sm text-text-secondary leading-relaxed group-hover:text-text-primary transition-colors">
                  {task.description}
                </p>
              ) : (
                <p className="text-sm text-text-muted italic group-hover:text-text-secondary transition-colors">
                  {t("addDescription")}
                </p>
              )}
            </button>
          )}
        </div>
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
