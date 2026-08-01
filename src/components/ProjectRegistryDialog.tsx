"use client";

import { useEffect, useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import {
  deleteProject,
  scanAndRegisterProjects,
  type ScanResult,
} from "@/desktop/renderer/actions/project";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import type { Project } from "@/entities/Project";
import FolderSearchInput from "@/components/FolderSearchInput";
import ProjectIcon from "@/components/ProjectIcon";
import { useEscapeKey } from "@/hooks/useEscapeKey";

interface ProjectRegistryDialogProps {
  isOpen: boolean;
  onClose: () => void;
  projects: Project[];
  sshHosts: string[];
}

export default function ProjectRegistryDialog({
  isOpen,
  onClose,
  projects,
  sshHosts,
}: ProjectRegistryDialogProps) {
  const t = useTranslations("settings");
  const tc = useTranslations("common");
  const { registerShortcutBlocker } = useBoardCommands();
  const [isPending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [scanSshHost, setScanSshHost] = useState("");

  useEscapeKey(() => onClose(), { enabled: isOpen });

  useEffect(() => {
    if (!isOpen) return undefined;

    return registerShortcutBlocker();
  }, [isOpen, registerShortcutBlocker]);

  if (!isOpen) return null;

  function handleScan(formData: FormData) {
    setError(null);
    setSuccessMessage(null);
    setScanResult(null);

    const scanPath = formData.get("scanPath") as string;
    if (!scanPath) {
      setError(t("scanPathRequired"));
      return;
    }

    startTransition(async () => {
      const result = await scanAndRegisterProjects(
        scanPath,
        (formData.get("scanSshHost") as string) || undefined,
      );
      setScanResult(result);

      const messages: string[] = [];

      if (result.registered.length > 0) {
        messages.push(t("registeredCount", { count: result.registered.length }));
      }
      if (result.worktreeTasks.length > 0) {
        messages.push(t("worktreeTasksRegistered", { count: result.worktreeTasks.length }));
      }

      if (messages.length > 0) {
        setSuccessMessage(messages.join(" / "));
      } else if (result.skipped.length > 0) {
        setSuccessMessage(t("noNewProjects"));
      } else {
        setError(t("noGitRepos"));
      }
    });
  }

  function handleDelete(projectId: string, projectName: string) {
    if (!confirm(t("deleteConfirm", { name: projectName }))) return;

    startTransition(async () => {
      await deleteProject(projectId);
    });
  }

  return (
    <div
      data-shortcut-capture="true"
      className="fixed inset-0 z-[450] flex items-center justify-center p-4"
    >
      <div className="fixed inset-0 bg-bg-overlay" onClick={onClose} />
      <section
        role="dialog"
        aria-modal="true"
        aria-labelledby="project-registry-dialog-title"
        className="relative flex max-h-[85vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-border-default bg-bg-surface shadow-xl"
      >
        <header className="flex items-center justify-between border-b border-border-default px-5 py-4">
          <div>
            <h2 id="project-registry-dialog-title" className="text-sm font-semibold text-text-primary">
              {t("scanTitle")}
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={tc("close")}
            className="text-lg text-text-muted hover:text-text-primary"
          >
            &times;
          </button>
        </header>

        <div className="min-h-0 overflow-y-auto px-5 py-4">
          <form
            data-testid="project-registry-form"
            onSubmit={(event) => {
              event.preventDefault();
              handleScan(new FormData(event.currentTarget));
            }}
            className="space-y-3 rounded-lg border border-border-default bg-bg-page p-3"
          >
            {sshHosts.length > 0 && (
              <select
                name="scanSshHost"
                value={scanSshHost}
                onChange={(e) => setScanSshHost(e.target.value)}
                className="w-full rounded-md border border-border-default bg-bg-surface px-3 py-1.5 text-sm text-text-primary transition-colors focus:border-brand-primary focus:outline-none"
              >
                <option value="">{tc("local")}</option>
                {sshHosts.map((host) => (
                  <option key={host} value={host}>
                    {host}
                  </option>
                ))}
              </select>
            )}

            <FolderSearchInput
              name="scanPath"
              sshHost={scanSshHost || undefined}
              onSelect={() => {}}
            />

            <button
              type="submit"
              disabled={isPending}
              className="w-full rounded-md bg-brand-primary py-1.5 text-sm text-text-inverse transition-colors hover:bg-brand-hover disabled:opacity-50"
            >
              {isPending ? t("scanning") : t("scanButton")}
            </button>
          </form>

          {scanResult && (
            <div className="mt-3 space-y-1 rounded-lg border border-border-default bg-bg-page p-3">
              {scanResult.registered.length > 0 && (
                <div className="text-xs text-status-success">
                  {t("registered")}: {scanResult.registered.join(", ")}
                </div>
              )}
              {scanResult.skipped.length > 0 && (
                <div className="text-xs text-text-muted">
                  {t("skipped")}: {scanResult.skipped.length}{t("skippedSuffix")}
                </div>
              )}
              {scanResult.errors.length > 0 && (
                <div className="text-xs text-status-error">
                  {t("errors")}: {scanResult.errors.length}{t("errorsSuffix")}
                </div>
              )}
            </div>
          )}

          {error && <p className="mt-2 text-xs text-status-error">{error}</p>}
          {successMessage && (
            <p className="mt-2 text-xs text-status-success">{successMessage}</p>
          )}

          <div className="mt-5 space-y-3">
            <h3 className="text-xs uppercase tracking-wide text-text-muted">
              {t("projectList")} ({projects.length})
            </h3>

            {projects.length === 0 ? (
              <p className="text-sm text-text-muted">
                {t("noProjects")}
              </p>
            ) : (
              <ul className="space-y-2">
                {projects.map((project) => (
                  <li
                    key={project.id}
                    className="rounded-md bg-bg-page p-2"
                  >
                    <div className="flex items-center justify-between gap-3">
                      <div className="min-w-0">
                        <p className="flex items-center gap-1.5 truncate text-sm font-medium text-text-primary">
                          <ProjectIcon projectName={project.name} iconDataUrl={project.iconDataUrl} />
                          {project.name}
                        </p>
                        <p className="truncate font-mono text-xs text-text-muted">
                          {project.sshHost && (
                            <span className="text-tag-ssh-text">
                              {project.sshHost}:
                            </span>
                          )}
                          {project.repoPath}
                        </p>
                        <p className="text-xs text-text-muted">
                          {t("defaultBranch")}: {project.defaultBranch}
                        </p>
                      </div>
                      <button
                        type="button"
                        onClick={() => handleDelete(project.id, project.name)}
                        disabled={isPending}
                        className="shrink-0 text-xs text-status-error hover:opacity-80 disabled:opacity-50"
                      >
                        {t("deleteProject")}
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      </section>
    </div>
  );
}
