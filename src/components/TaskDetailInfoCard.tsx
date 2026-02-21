"use client";

import { useTranslations } from "next-intl";
import type { KanbanTask } from "@/entities/KanbanTask";
import PriorityEditor from "@/components/PriorityEditor";
import { Link } from "@/i18n/navigation";
import ProjectColorEditor from "@/components/ProjectColorEditor";
import { computeProjectColor } from "@/lib/projectColor";

interface TaskDetailInfoCardProps {
  task: KanbanTask;
  agentTagStyle: string | null;
  baseBranchTaskId: string | null;
}

export default function TaskDetailInfoCard({
  task,
  agentTagStyle,
  baseBranchTaskId,
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
                    className="text-xs px-2 py-0.5 rounded-full font-medium text-white truncate max-w-[140px]"
                    style={{ backgroundColor: task.project.color || computeProjectColor(task.project.name) }}
                  >
                    {task.project.name}
                  </span>
                  <Link
                    href={baseBranchTaskId ? `/task/${baseBranchTaskId}` : "/"}
                    className="inline-flex items-center justify-center w-5 h-5 rounded-full bg-bg-page border border-border-default hover:border-brand-primary hover:text-text-brand text-text-muted transition-colors"
                    title={task.baseBranch ?? task.project.name}
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
          {task.prUrl && (
            <div className="flex items-center justify-between gap-2">
              <dt className="text-xs text-text-muted">{t("prLink")}</dt>
              <dd>
                <a
                  href={task.prUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-1 text-xs bg-tag-pr-bg text-tag-pr-text px-2 py-0.5 rounded hover:opacity-80 transition-opacity"
                >
                  <svg
                    width="12"
                    height="12"
                    viewBox="0 0 16 16"
                    fill="currentColor"
                  >
                    <path d="M7.177 3.073L9.573.677A.25.25 0 0110 .854v4.792a.25.25 0 01-.427.177L7.177 3.427a.25.25 0 010-.354zM3.75 2.5a.75.75 0 100 1.5.75.75 0 000-1.5zm-2.25.75a2.25 2.25 0 113 2.122v5.256a2.251 2.251 0 11-1.5 0V5.372A2.25 2.25 0 011.5 3.25zM11 2.5h-1V4h1a1 1 0 011 1v5.628a2.251 2.251 0 101.5 0V5A2.5 2.5 0 0011 2.5zm1 10.25a.75.75 0 111.5 0 .75.75 0 01-1.5 0zM3.75 12a.75.75 0 100 1.5.75.75 0 000-1.5z" />
                  </svg>
                  PR
                  <svg
                    width="10"
                    height="10"
                    viewBox="0 0 16 16"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.5"
                  >
                    <path
                      d="M4 12L12 4M12 4H6M12 4v6"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                  </svg>
                </a>
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
