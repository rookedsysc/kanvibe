"use client";

import { useTranslations } from "next-intl";
import type { KanbanTask } from "@/entities/KanbanTask";
import PriorityEditor from "@/components/PriorityEditor";
import { Link } from "@/desktop/renderer/navigation";
import ProjectColorEditor from "@/components/ProjectColorEditor";

interface TaskDetailInfoCardProps {
  task: KanbanTask;
  agentTagStyle: string | null;
  baseBranchTaskId: string | null;
  diffFileCount?: number;
}

export default function TaskDetailInfoCard({
  task,
  agentTagStyle,
  baseBranchTaskId,
  diffFileCount,
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
          {task.branchName && (
            <div className="flex items-center justify-between gap-2">
              <dt className="text-xs text-text-muted">{t("diffFiles")}</dt>
              <dd>
                <Link
                  href={`/task/${task.id}/diff`}
                  className="inline-flex items-center gap-1.5 text-xs bg-tag-branch-bg text-tag-branch-text px-2 py-0.5 rounded hover:opacity-80 transition-opacity"
                  title={t("viewDiff")}
                >
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <circle cx="6" cy="6" r="2.5" />
                    <circle cx="18" cy="18" r="2.5" />
                    <path d="M6 8.5v4c0 2 1.5 3.5 3.5 3.5H14" />
                    <path d="M15 13l3 3-3 3" />
                  </svg>
                  {diffFileCount !== undefined && diffFileCount > 0 && (
                    <span className="font-medium">{t("diffFileCount", { count: diffFileCount })}</span>
                  )}
                </Link>
              </dd>
            </div>
          )}
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
