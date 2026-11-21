"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import {
  deleteProject,
  scanAndRegisterProjects,
  type ScanResult,
} from "@/desktop/renderer/actions/project";
import { useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import { showActionStatus } from "@/desktop/renderer/components/ActionStatusToast";
import { useTrackedAction } from "@/desktop/renderer/hooks/useTrackedAction";
import InlineLoadError from "@/components/InlineLoadError";
import type { Project } from "@/entities/Project";
import FolderSearchInput from "@/components/FolderSearchInput";
import ProjectIcon from "@/components/ProjectIcon";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import { matchesProjectSearch } from "@/utils/projectSearch";

function getErrorReason(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

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
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [scanSshHost, setScanSshHost] = useState("");
  const [projectSearchQuery, setProjectSearchQuery] = useState("");
  const [deleteTargetProjectId, setDeleteTargetProjectId] = useState<string | null>(null);
  /** 보드는 닫힌 대화상자를 내리므로, 닫힌 뒤 끝난 결과는 토스트로 알린다 */
  const isMountedRef = useRef(true);
  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  /** 스캔과 프로젝트 삭제는 서로 다른 작업이라 진행·실패 상태를 따로 둔다 */
  const scanAction = useTrackedAction(async (scanPath: string, sshHost: string | undefined) => {
    try {
      const result = await scanAndRegisterProjects(scanPath, sshHost);
      applyScanResult(result);
    } catch (scanError) {
      announceIfClosed("failure", t("scanFailed", { reason: getErrorReason(scanError) }));
      throw scanError;
    }
  });
  const deleteAction = useTrackedAction(async (projectId: string, projectName: string) => {
    try {
      await deleteProject(projectId);
      announceIfClosed("success", t("deleteProjectSucceeded", { name: projectName }));
    } catch (deleteError) {
      announceIfClosed("failure", t("deleteProjectFailed", { reason: getErrorReason(deleteError) }));
      throw deleteError;
    }
  });

  const filteredProjects = useMemo(
    () => projects.filter((project) => matchesProjectSearch(project, projectSearchQuery)),
    [projects, projectSearchQuery],
  );

  useEscapeKey(() => onClose(), { enabled: isOpen });

  useEffect(() => {
    if (!isOpen) return undefined;

    return registerShortcutBlocker();
  }, [isOpen, registerShortcutBlocker]);

  if (!isOpen) return null;

  function announceIfClosed(kind: "success" | "failure", message: string) {
    if (!isMountedRef.current) {
      showActionStatus({ kind, message });
    }
  }

  function applyScanResult(result: ScanResult) {
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
      announceIfClosed("success", messages.join(" / "));
    } else if (result.skipped.length > 0) {
      setSuccessMessage(t("noNewProjects"));
    } else if (result.errors.length === 0) {
      setError(t("noGitRepos"));
    }
  }

  function handleScan(formData: FormData) {
    setError(null);
    setSuccessMessage(null);
    setScanResult(null);
    setProjectSearchQuery("");

    const scanPath = formData.get("scanPath") as string;
    if (!scanPath) {
      setError(t("scanPathRequired"));
      return;
    }

    void scanAction.run(scanPath, (formData.get("scanSshHost") as string) || undefined);
  }

  function handleDelete(projectId: string, projectName: string) {
    if (deleteAction.isPending) return;
    if (!confirm(t("deleteConfirm", { name: projectName }))) return;

    setDeleteTargetProjectId(projectId);
    void deleteAction.run(projectId, projectName);
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
              disabled={scanAction.isPending}
              aria-busy={scanAction.isPending || undefined}
              className="w-full rounded-md bg-brand-primary py-1.5 text-sm text-text-inverse transition-colors hover:bg-brand-hover disabled:opacity-50"
            >
              {scanAction.isPending ? t("scanning") : t("scanButton")}
            </button>
          </form>

          {scanAction.error ? (
            <InlineLoadError
              className="mt-3"
              message={t("scanFailed", { reason: scanAction.error })}
              onRetry={() => void scanAction.retry()}
            />
          ) : null}

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
                  <div>
                    {t("errors")}: {scanResult.errors.length}{t("errorsSuffix")}
                  </div>
                  <ul className="mt-1 list-disc space-y-0.5 pl-4">
                    {scanResult.errors.map((message) => (
                      <li key={message}>{message}</li>
                    ))}
                  </ul>
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
              {t("projectList")} (
              {projectSearchQuery.trim()
                ? `${filteredProjects.length} / ${projects.length}`
                : projects.length}
              )
            </h3>

            {projects.length > 0 && (
              <input
                type="search"
                data-testid="project-registry-search"
                value={projectSearchQuery}
                onChange={(event) => setProjectSearchQuery(event.target.value)}
                placeholder={t("projectSearchPlaceholder")}
                aria-label={t("projectSearchPlaceholder")}
                autoComplete="off"
                className="w-full rounded-md border border-border-default bg-bg-page px-3 py-1.5 text-sm text-text-primary transition-colors focus:border-brand-primary focus:outline-none"
              />
            )}

            {projects.length === 0 ? (
              <p className="text-sm text-text-muted">
                {t("noProjects")}
              </p>
            ) : filteredProjects.length === 0 ? (
              <p className="text-sm text-text-muted">
                {t("noMatchingProjects")}
              </p>
            ) : (
              <ul className="space-y-2">
                {filteredProjects.map((project) => {
                  const isDeletingProject = deleteAction.pendingArgs?.[0] === project.id;
                  const deleteFailureReason =
                    !deleteAction.isPending && deleteTargetProjectId === project.id ? deleteAction.error : null;
                  return (
                    <li
                      key={project.id}
                      aria-busy={isDeletingProject || undefined}
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
                          disabled={deleteAction.isPending}
                          className="shrink-0 text-xs text-status-error hover:opacity-80 disabled:opacity-50"
                        >
                          {isDeletingProject ? t("deletingProject") : t("deleteProject")}
                        </button>
                      </div>
                      {deleteFailureReason ? (
                        <InlineLoadError
                          className="mt-2"
                          message={t("deleteProjectFailed", { reason: deleteFailureReason })}
                          onRetry={() => void deleteAction.retry()}
                        />
                      ) : null}
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </div>
      </section>
    </div>
  );
}
