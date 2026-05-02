"use client";

import { useState, useEffect, useTransition, useCallback } from "react";
import { useTranslations } from "next-intl";
import { useRouter } from "@/desktop/renderer/navigation";
import { createTask } from "@/desktop/renderer/actions/kanban";
import { getProjectBranches } from "@/desktop/renderer/actions/project";
import { ensureSessionDependencyWithPrompt } from "@/desktop/renderer/utils/sessionDependencyPrompt";
import { SessionType } from "@/entities/KanbanTask";
import { TaskPriority } from "@/entities/TaskPriority";
import type { Project } from "@/entities/Project";
import { useEscapeKey } from "@/hooks/useEscapeKey";
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
  defaultSessionType?: string;
}

type CreateTaskModalContentProps = Pick<
  CreateTaskModalProps,
  "onClose" | "projects" | "defaultProjectId" | "defaultBaseBranch" | "defaultSessionType"
>;

function findProjectDefaultBranch(projects: Project[], projectId: string) {
  return projects.find((p) => p.id === projectId)?.defaultBranch ?? "";
}

function resolveInitialBaseBranch(
  projects: Project[],
  projectId: string,
  defaultBaseBranch?: string,
) {
  if (!projectId) return "";
  return defaultBaseBranch || findProjectDefaultBranch(projects, projectId);
}

export default function CreateTaskModal({
  isOpen,
  onClose,
  projects,
  defaultProjectId,
  defaultBaseBranch,
  defaultSessionType,
}: CreateTaskModalProps) {
  if (!isOpen) return null;

  return (
    <CreateTaskModalContent
      onClose={onClose}
      projects={projects}
      defaultProjectId={defaultProjectId}
      defaultBaseBranch={defaultBaseBranch}
      defaultSessionType={defaultSessionType}
    />
  );
}

function CreateTaskModalContent({
  onClose,
  projects,
  defaultProjectId,
  defaultBaseBranch,
  defaultSessionType,
}: CreateTaskModalContentProps) {
  const t = useTranslations("task");
  const tc = useTranslations("common");
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const initialProjectId = defaultProjectId || "";
  const [selectedProjectId, setSelectedProjectId] = useState(initialProjectId);
  const [branches, setBranches] = useState<string[]>([]);
  const [baseBranch, setBaseBranch] = useState(() =>
    resolveInitialBaseBranch(projects, initialProjectId, defaultBaseBranch)
  );
  const [priority, setPriority] = useState<TaskPriority | null>(null);
  const [error, setError] = useState<string | null>(null);

  /** worktree가 아닌 프로젝트만 필터링한다 */
  const availableProjects = projects.filter((p) => !p.isWorktree);

  const handleProjectSelect = useCallback(
    (projectId: string) => {
      setSelectedProjectId(projectId);
      setBaseBranch(projectId ? findProjectDefaultBranch(projects, projectId) : "");
      setBranches([]);
    },
    [projects],
  );

  useEffect(() => {
    if (!selectedProjectId) return;

    let isCancelled = false;
    getProjectBranches(selectedProjectId).then((result) => {
      if (isCancelled) return;

      setBranches(result);
      /** defaultBaseBranch가 브랜치 목록에 없으면 옵션에 추가한다 */
      if (defaultBaseBranch && !result.includes(defaultBaseBranch)) {
        setBranches((prev) => [defaultBaseBranch, ...prev]);
      }
    });

    return () => {
      isCancelled = true;
    };
  }, [selectedProjectId, defaultBaseBranch]);

  useEscapeKey(onClose);

  const branchOptions = baseBranch && !branches.includes(baseBranch)
    ? [baseBranch, ...branches]
    : branches;

  function handleSubmit(formData: FormData) {
    const branchName = formData.get("branchName") as string;
    if (!branchName || !selectedProjectId) return;
    setError(null);

    startTransition(async () => {
      try {
        const sessionType = (formData.get("sessionType") as SessionType) || undefined;
        const selectedProject = projects.find((project) => project.id === selectedProjectId) ?? null;

        if (sessionType) {
          const isReady = await ensureSessionDependencyWithPrompt(sessionType, selectedProject?.sshHost, tc);
          if (!isReady) {
            return;
          }
        }

        const created = await createTask({
          title: branchName,
          description: (formData.get("description") as string) || undefined,
          branchName,
          baseBranch: baseBranch || undefined,
          sessionType,
          sshHost: (formData.get("sshHost") as string) || undefined,
          projectId: selectedProjectId,
          priority: priority || undefined,
        });
        onClose();
        router.push(`/task/${created.id}`);
      } catch (error) {
        setError(error instanceof Error ? error.message : t("createFailed"));
      }
    });
  }

  function handleFormKeyDown(event: React.KeyboardEvent<HTMLFormElement>) {
    if (
      event.key !== "Enter" ||
      event.defaultPrevented ||
      event.nativeEvent.isComposing ||
      event.shiftKey ||
      event.altKey ||
      event.ctrlKey ||
      event.metaKey
    ) {
      return;
    }

    if (event.target instanceof HTMLButtonElement) {
      return;
    }

    event.preventDefault();
    event.currentTarget.requestSubmit();
  }

  return (
    <div className="fixed inset-0 z-[400] flex items-center justify-center bg-bg-overlay">
      <div className="w-full max-w-md bg-bg-surface rounded-xl border border-border-default shadow-lg p-6">
        <h2 className="text-lg font-semibold text-text-primary mb-4">
          {t("createTitle")}
        </h2>

        <form action={handleSubmit} className="space-y-4" onKeyDown={handleFormKeyDown}>
          <div>
            <label className="block text-sm text-text-secondary mb-1">
              {t("project")} *
            </label>
            <ProjectSelector
              projects={availableProjects}
              selectedProjectId={selectedProjectId}
              onSelect={handleProjectSelect}
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
                branches={branchOptions}
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
              defaultValue={defaultSessionType || "tmux"}
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
