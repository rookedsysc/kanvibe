"use client";

import { useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import {
  deleteProject,
  scanAndRegisterProjects,
  type ScanResult,
} from "@/app/actions/project";
import type { Project } from "@/entities/Project";

interface ProjectSettingsProps {
  isOpen: boolean;
  onClose: () => void;
  projects: Project[];
  sshHosts: string[];
}

export default function ProjectSettings({
  isOpen,
  onClose,
  projects,
  sshHosts,
}: ProjectSettingsProps) {
  const t = useTranslations("settings");
  const tc = useTranslations("common");
  const [isPending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);

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
        (formData.get("scanSshHost") as string) || undefined
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
    <div className="fixed inset-0 z-[400] flex items-start justify-end pt-14 pr-4">
      <div className="fixed inset-0" onClick={onClose} />
      <div className="relative w-96 bg-bg-surface rounded-xl border border-border-default shadow-lg max-h-[80vh] overflow-y-auto">
        <div className="flex items-center justify-between p-4 border-b border-border-default">
          <h2 className="text-sm font-semibold text-text-primary">{t("title")}</h2>
          <button
            onClick={onClose}
            className="text-text-muted hover:text-text-primary text-lg"
          >
            &times;
          </button>
        </div>

        {/* 디렉토리 스캔 등록 */}
        <div className="p-4 border-b border-border-default">
          <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">
            {t("scanTitle")}
          </h3>

          <form action={handleScan} className="space-y-3">
            <input
              name="scanPath"
              required
              placeholder={t("scanPathPlaceholder")}
              className="w-full px-3 py-1.5 text-sm bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary font-mono transition-colors"
            />

            {sshHosts.length > 0 && (
              <select
                name="scanSshHost"
                className="w-full px-3 py-1.5 text-sm bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
              >
                <option value="">{tc("local")}</option>
                {sshHosts.map((host) => (
                  <option key={host} value={host}>
                    {host}
                  </option>
                ))}
              </select>
            )}

            <button
              type="submit"
              disabled={isPending}
              className="w-full py-1.5 text-sm bg-brand-primary hover:bg-brand-hover disabled:opacity-50 text-text-inverse rounded-md transition-colors"
            >
              {isPending ? t("scanning") : t("scanButton")}
            </button>
          </form>

          {scanResult && (
            <div className="mt-3 space-y-1">
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

          {error && <p className="text-xs text-status-error mt-2">{error}</p>}
          {successMessage && (
            <p className="text-xs text-status-success mt-2">{successMessage}</p>
          )}
        </div>

        {/* 등록된 프로젝트 목록 */}
        <div className="p-4 space-y-3">
          <h3 className="text-xs text-text-muted uppercase tracking-wide">
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
                  className="flex items-center justify-between p-2 bg-bg-page rounded-md"
                >
                  <div className="min-w-0">
                    <p className="text-sm font-medium text-text-primary truncate">
                      {project.name}
                    </p>
                    <p className="text-xs text-text-muted font-mono truncate">
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
                    onClick={() => handleDelete(project.id, project.name)}
                    disabled={isPending}
                    className="ml-2 text-xs text-status-error hover:opacity-80 shrink-0"
                  >
                    {t("deleteProject")}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
