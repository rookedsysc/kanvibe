"use client";

import { useState, useEffect, useRef, useTransition } from "react";
import { useLocale, useTranslations } from "next-intl";
import { branchFromTask } from "@/desktop/renderer/actions/kanban";
import { useProjectBranches } from "@/desktop/renderer/hooks/useProjectBranches";
import { SessionType, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import BranchSearchInput from "./BranchSearchInput";
import ProjectBranchesStatusNotice from "./ProjectBranchesStatusNotice";
import SessionTypeOptions from "./SessionTypeOptions";
import { ensureSessionDependencyWithPrompt } from "@/desktop/renderer/utils/sessionDependencyPrompt";
import { navigateToTaskDetail } from "@/desktop/renderer/utils/taskNavigation";
import { showActionStatus } from "@/desktop/renderer/components/ActionStatusToast";

interface BranchTaskModalProps {
  task: KanbanTask;
  projects: Project[];
  defaultSessionType?: string;
  onClose: () => void;
}

function findProjectDefaultBranch(projects: Project[], projectId: string) {
  return projects.find((project) => project.id === projectId)?.defaultBranch ?? "";
}

/** 기존 작업에서 브랜치를 분기하는 모달. 프로젝트, 베이스 브랜치, 새 브랜치명, 세션 타입을 선택한다 */
export default function BranchTaskModal({
  task,
  projects,
  defaultSessionType,
  onClose,
}: BranchTaskModalProps) {
  const t = useTranslations("branch");
  const tc = useTranslations("common");
  const locale = useLocale();
  const [isPending, startTransition] = useTransition();
  const [selectedProjectId, setSelectedProjectId] = useState(
    projects[0]?.id || ""
  );
  const [baseBranch, setBaseBranch] = useState(() =>
    findProjectDefaultBranch(projects, projects[0]?.id || "")
  );
  const [branchName, setBranchName] = useState("");
  const [sessionType, setSessionType] = useState<SessionType>(
    (defaultSessionType as SessionType) || SessionType.HERDR
  );
  const [error, setError] = useState<string | null>(null);
  /** 응답을 기다리는 동안 제출을 한 번만 받는다. 상태값은 같은 틱의 연속 제출을 막지 못해 ref로 막는다 */
  const isSubmittingRef = useRef(false);
  /** 분기 중 모달을 닫으면 결과를 토스트로 알리기 위해 열려 있는지 기억한다 */
  const isModalMountedRef = useRef(true);
  const selectedProjectName = projects.find((project) => project.id === selectedProjectId)?.name;
  const {
    branches,
    status: branchesStatus,
    failureReason: branchesFailureReason,
    reload: reloadBranches,
  } = useProjectBranches(selectedProjectId);

  useEffect(() => {
    isModalMountedRef.current = true;
    return () => {
      isModalMountedRef.current = false;
    };
  }, []);

  useEscapeKey(onClose);

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (isSubmittingRef.current) return;
    setError(null);

    if (!selectedProjectId || !branchName) {
      setError(t("validationError"));
      return;
    }

    isSubmittingRef.current = true;
    startTransition(async () => {
      try {
        if (sessionType === SessionType.HERDR && !await ensureSessionDependencyWithPrompt(
          sessionType, projects.find((project) => project.id === selectedProjectId)?.sshHost, tc,
        )) return;

        const branchedTask = await branchFromTask(
          task.id,
          selectedProjectId,
          baseBranch,
          branchName,
          sessionType
        );
        if (isModalMountedRef.current) {
          onClose();
          return;
        }

        showActionStatus({
          kind: "success",
          message: t("succeeded", { branchName }),
          action: branchedTask
            ? { label: tc("open"), onAction: () => void navigateToTaskDetail(branchedTask.id, { currentLocale: locale }) }
            : undefined,
        });
      } catch (err) {
        const reason = err instanceof Error ? err.message : t("failedError");
        if (isModalMountedRef.current) {
          setError(reason);
        } else {
          showActionStatus({ kind: "failure", message: t("failedWithReason", { reason }) });
        }
      } finally {
        isSubmittingRef.current = false;
      }
    });
  }

  function handleProjectChange(projectId: string) {
    setSelectedProjectId(projectId);
    setBaseBranch(findProjectDefaultBranch(projects, projectId));
  }

  return (
    <div className="fixed inset-0 z-[400] flex items-center justify-center bg-bg-overlay">
      <div className="w-full max-w-md bg-bg-surface rounded-xl border border-border-default shadow-lg p-6">
        <h2 className="text-lg font-semibold text-text-primary mb-1">
          {t("title")}
        </h2>
        <p className="text-sm text-text-secondary mb-4 truncate">
          {task.title}
        </p>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("projectRequired")}
            </label>
            <select
              value={selectedProjectId}
              onChange={(e) => handleProjectChange(e.target.value)}
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
            >
              {projects.length === 0 && (
                <option value="">{t("noProjects")}</option>
              )}
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.name}
                  {project.sshHost ? ` (${project.sshHost})` : ""}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("baseBranch")}
            </label>
            <BranchSearchInput
              branches={branches.length > 0 ? branches : baseBranch ? [baseBranch] : []}
              value={baseBranch}
              onChange={setBaseBranch}
              projectName={selectedProjectName}
            />
            <ProjectBranchesStatusNotice
              status={branchesStatus}
              failureReason={branchesFailureReason}
              onRetry={reloadBranches}
            />
          </div>

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("newBranchName")}
            </label>
            <input
              value={branchName}
              onChange={(e) => setBranchName(e.target.value)}
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary font-mono transition-colors"
              placeholder={t("branchPlaceholder")}
            />
          </div>

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("sessionType")}
            </label>
            <select
              value={sessionType}
              onChange={(e) => setSessionType(e.target.value as SessionType)}
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
            >
              <SessionTypeOptions />
            </select>
          </div>

          {error && <p className="text-xs text-status-error">{error}</p>}

          <div className="flex justify-end gap-3 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-sm text-text-muted hover:text-text-primary transition-colors"
            >
              {isPending ? tc("closeAndContinue") : tc("cancel")}
            </button>
            <button
              type="submit"
              disabled={isPending || projects.length === 0}
              className="px-4 py-2 text-sm bg-brand-primary hover:bg-brand-hover disabled:opacity-50 text-text-inverse rounded-md font-medium transition-colors"
            >
              {isPending ? t("submitting") : t("submit")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
