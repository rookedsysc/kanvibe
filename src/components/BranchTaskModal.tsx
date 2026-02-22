"use client";

import { useState, useEffect, useTransition } from "react";
import { useTranslations } from "next-intl";
import { branchFromTask } from "@/app/actions/kanban";
import { getProjectBranches } from "@/app/actions/project";
import { SessionType, type KanbanTask } from "@/entities/KanbanTask";
import type { Project } from "@/entities/Project";

interface BranchTaskModalProps {
  task: KanbanTask;
  projects: Project[];
  onClose: () => void;
}

/** 기존 작업에서 브랜치를 분기하는 모달. 프로젝트, 베이스 브랜치, 새 브랜치명, 세션 타입을 선택한다 */
export default function BranchTaskModal({
  task,
  projects,
  onClose,
}: BranchTaskModalProps) {
  const t = useTranslations("branch");
  const tc = useTranslations("common");
  const [isPending, startTransition] = useTransition();
  const [selectedProjectId, setSelectedProjectId] = useState(
    projects[0]?.id || ""
  );
  const [branches, setBranches] = useState<string[]>([]);
  const [baseBranch, setBaseBranch] = useState("");
  const [branchName, setBranchName] = useState("");
  const [sessionType, setSessionType] = useState<SessionType>(
    SessionType.TMUX
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!selectedProjectId) return;

    const selectedProject = projects.find((p) => p.id === selectedProjectId);
    if (selectedProject) {
      setBaseBranch(selectedProject.defaultBranch);
    }

    getProjectBranches(selectedProjectId).then((result) => {
      setBranches(result);
    });
  }, [selectedProjectId, projects]);

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);

    if (!selectedProjectId || !branchName) {
      setError(t("validationError"));
      return;
    }

    startTransition(async () => {
      try {
        await branchFromTask(
          task.id,
          selectedProjectId,
          baseBranch,
          branchName,
          sessionType
        );
        onClose();
      } catch (err) {
        setError(
          err instanceof Error ? err.message : t("failedError")
        );
      }
    });
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
              onChange={(e) => setSelectedProjectId(e.target.value)}
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
            <select
              value={baseBranch}
              onChange={(e) => setBaseBranch(e.target.value)}
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary font-mono transition-colors"
            >
              {branches.map((branch) => (
                <option key={branch} value={branch}>
                  {branch}
                </option>
              ))}
              {branches.length === 0 && baseBranch && (
                <option value={baseBranch}>{baseBranch}</option>
              )}
            </select>
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
              <option value="tmux">tmux</option>
              <option value="zellij">zellij</option>
            </select>
          </div>

          {error && <p className="text-xs text-status-error">{error}</p>}

          <div className="flex justify-end gap-3 pt-2">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 text-sm text-text-muted hover:text-text-primary transition-colors"
            >
              {tc("cancel")}
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
