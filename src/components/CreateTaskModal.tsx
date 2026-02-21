"use client";

import { useState, useEffect, useTransition } from "react";
import { useTranslations } from "next-intl";
import { useRouter } from "@/i18n/navigation";
import { createTask } from "@/app/actions/kanban";
import { getProjectBranches } from "@/app/actions/project";
import { SessionType } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import type { Project } from "@/entities/Project";
import ProjectSelector from "./ProjectSelector";
import PrioritySelector from "./PrioritySelector";
import BranchSearchInput from "./BranchSearchInput";

interface CreateTaskModalProps {
  isOpen: boolean;
  onClose: () => void;
  sshHosts: string[];
  projects: Project[];
  defaultProjectId?: string;
  defaultBaseBranch?: string;
}

export default function CreateTaskModal({
  isOpen,
  onClose,
  sshHosts,
  projects,
  defaultProjectId,
  defaultBaseBranch,
}: CreateTaskModalProps) {
  const t = useTranslations("task");
  const tc = useTranslations("common");
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const [selectedProjectId, setSelectedProjectId] = useState(defaultProjectId || "");
  const [branches, setBranches] = useState<string[]>([]);
  const [baseBranch, setBaseBranch] = useState("");
  const [priority, setPriority] = useState<TaskPriority | null>(null);

  /** 모달이 열릴 때 필터에서 선택된 프로젝트를 자동 설정한다 */
  useEffect(() => {
    if (isOpen && defaultProjectId) {
      setSelectedProjectId(defaultProjectId);
    }
  }, [isOpen, defaultProjectId]);

  /** worktree가 아닌 프로젝트만 필터링한다 */
  const availableProjects = projects.filter((p) => !p.isWorktree);

  useEffect(() => {
    if (!selectedProjectId) {
      setBranches([]);
      setBaseBranch("");
      return;
    }

    const selectedProject = projects.find((p) => p.id === selectedProjectId);
    if (selectedProject) {
      setBaseBranch(defaultBaseBranch || selectedProject.defaultBranch);
    }

    getProjectBranches(selectedProjectId).then((result) => {
      setBranches(result);
      /** defaultBaseBranch가 브랜치 목록에 없으면 옵션에 추가한다 */
      if (defaultBaseBranch && !result.includes(defaultBaseBranch)) {
        setBranches((prev) => [defaultBaseBranch, ...prev]);
      }
    });
  }, [selectedProjectId, projects, defaultBaseBranch]);

  if (!isOpen) return null;

  function handleSubmit(formData: FormData) {
    const branchName = formData.get("branchName") as string;
    if (!branchName || !selectedProjectId) return;

    startTransition(async () => {
      const created = await createTask({
        title: branchName,
        description: (formData.get("description") as string) || undefined,
        branchName,
        baseBranch: baseBranch || undefined,
        sessionType: (formData.get("sessionType") as SessionType) || undefined,
        sshHost: (formData.get("sshHost") as string) || undefined,
        projectId: selectedProjectId,
        priority: priority || undefined,
      });
      onClose();
      router.push(`/task/${created.id}`);
    });
  }

  return (
    <div className="fixed inset-0 z-[400] flex items-center justify-center bg-bg-overlay">
      <div className="w-full max-w-md bg-bg-surface rounded-xl border border-border-default shadow-lg p-6">
        <h2 className="text-lg font-semibold text-text-primary mb-4">
          {t("createTitle")}
        </h2>

        <form action={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("project")} *
            </label>
            <ProjectSelector
              projects={availableProjects}
              selectedProjectId={selectedProjectId}
              onSelect={setSelectedProjectId}
              placeholder={t("projectSelect")}
              searchPlaceholder={t("projectSearch")}
            />
          </div>

          {selectedProjectId && (
            <div>
              <label className="block text-sm text-text-secondary mb-1">
                {t("baseBranch")}
              </label>
              <BranchSearchInput
                branches={branches.length > 0 ? branches : baseBranch ? [baseBranch] : []}
                value={baseBranch}
                onChange={setBaseBranch}
              />
            </div>
          )}

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("branchName")} *
            </label>
            <input
              name="branchName"
              required
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary font-mono transition-colors"
              placeholder={t("branchPlaceholder")}
            />
          </div>

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("descriptionLabel")}
            </label>
            <textarea
              name="description"
              rows={3}
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary resize-none transition-colors"
              placeholder={t("descriptionPlaceholder")}
            />
          </div>

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("priority")}
            </label>
            <PrioritySelector value={priority} onChange={setPriority} />
          </div>

          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("sessionType")}
            </label>
            <select
              name="sessionType"
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
            >
              <option value="tmux">tmux</option>
              <option value="zellij">zellij</option>
            </select>
          </div>

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
              disabled={isPending || !selectedProjectId}
              className="px-4 py-2 text-sm bg-brand-primary hover:bg-brand-hover disabled:opacity-50 text-text-inverse rounded-md font-medium transition-colors"
            >
              {isPending ? tc("creating") : tc("create")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
