"use client";

import { useTranslations } from "next-intl";
import type { KanbanTask } from "@/entities/KanbanTask";
import PriorityEditor from "@/components/PriorityEditor";
import { Link } from "@/desktop/renderer/navigation";
import ProjectColorEditor from "@/components/ProjectColorEditor";
import { TaskDiffSummary } from "@/components/TaskDiffStats";
import type { DiffFile } from "@/desktop/renderer/actions/diff";

const EMPTY_DIFF_FILES: DiffFile[] = [];

interface TaskDetailInfoCardProps {
  task: KanbanTask;
  agentTagStyle: string | null;
  baseBranchTaskId: string | null;
  diffFiles?: DiffFile[];
}

export default function TaskDetailInfoCard({
  task,
  agentTagStyle,
  baseBranchTaskId,
  diffFiles = EMPTY_DIFF_FILES,
}: TaskDetailInfoCardProps) {
  const t = useTranslations("taskDetail");

  return (
    <>
      {/* 메타데이터 카드 */}
      <div className="bg-bg-surface rounded-lg p-5 shadow-sm border border-border-default">
        <h3 className="text-xs font-semibold text-text-muted uppercase tracking-wider mb-4">
          {t("info")}
        </h3>
        <dl className="space-y-3">
          {task.project && (
            <>
              <div className="flex items-center justify-between gap-2">
                <dt className="text-xs text-text-muted">{t("project")}</dt>
                <dd className="flex items-center gap-1">
                  <span
                    className="text-xs px-2 py-0.5 rounded-full font-medium text-white truncate max-w-[140px] bg-tag-project-bg"
                  >
                    {task.project.name}
                  </span>
                  <Link
                    href={baseBranchTaskId ? `/task/${baseBranchTaskId}` : "/"}
                    className="inline-flex items-center justify-center w-5 h-5 rounded-full bg-tag-project-bg hover:opacity-80 text-white transition-opacity"
                    title={task.baseBranch ?? task.project.name}
                    data-testid="shortcut-link"
                  >
                    <svg
                      width="10"
                      height="10"
                      viewBox="0 0 16 16"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                    >
                      <path
                        d="M6 4L10 8L6 12"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  </Link>
                </dd>
              </div>
              <div className="flex items-center justify-between gap-2">
                <dt className="text-xs text-text-muted">{t("projectColor")}</dt>
                <dd>
                  <ProjectColorEditor
                    projectId={task.project.id}
                    projectName={task.project.name}
                    currentColor={task.project.color}
                  />
                </dd>
              </div>
            </>
          )}
          <div className="flex items-center justify-between gap-2">
            <dt className="text-xs text-text-muted">{t("priority")}</dt>
            <dd>
              <PriorityEditor
                taskId={task.id}
                currentPriority={task.priority}
              />
            </dd>
          </div>
          {task.branchName && <TaskDiffSummary taskId={task.id} files={diffFiles} />}
          {task.agentType && (
            <div className="flex items-center justify-between gap-2">
              <dt className="text-xs text-text-muted">{t("agent")}</dt>
              <dd className={`text-xs px-2 py-0.5 rounded-full font-medium ${agentTagStyle}`}>
                {task.agentType}
              </dd>
            </div>
          )}
          {task.sessionType && (
            <div className="flex items-center justify-between gap-2">
              <dt className="text-xs text-text-muted">{t("session")}</dt>
              <dd className="text-xs bg-tag-session-bg text-tag-session-text px-2 py-0.5 rounded-full">
                {task.sessionType}
              </dd>
            </div>
          )}
          {task.sshHost && (
            <div className="flex items-center justify-between gap-2">
              <dt className="text-xs text-text-muted">{t("sshHost")}</dt>
              <dd className="text-xs font-mono bg-tag-ssh-bg text-tag-ssh-text px-2 py-0.5 rounded">
                {task.sshHost}
              </dd>
            </div>
          )}
          <div className="border-t border-border-subtle pt-3 mt-3">
            <div className="flex items-center justify-between gap-2 mb-2">
              <dt className="text-xs text-text-muted">{t("createdAt")}</dt>
              <dd className="text-xs text-text-secondary">
                {new Date(task.createdAt).toLocaleDateString()}
              </dd>
            </div>
            <div className="flex items-center justify-between gap-2">
              <dt className="text-xs text-text-muted">{t("updatedAt")}</dt>
              <dd className="text-xs text-text-secondary">
                {new Date(task.updatedAt).toLocaleDateString()}
              </dd>
            </div>
          </div>
        </dl>
      </div>
    </>
  );
}
