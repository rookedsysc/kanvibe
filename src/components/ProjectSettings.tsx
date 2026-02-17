"use client";

import { useState, useTransition, useEffect, useCallback } from "react";
import { useTranslations } from "next-intl";
import {
  deleteProject,
  scanAndRegisterProjects,
  getProjectHooksStatus,
  installProjectHooks,
  getProjectGeminiHooksStatus,
  installProjectGeminiHooks,
  type ScanResult,
} from "@/app/actions/project";
import { setSidebarDefaultCollapsed } from "@/app/actions/appSettings";
import { Link } from "@/i18n/navigation";
import type { Project } from "@/entities/Project";
import type { ClaudeHooksStatus } from "@/lib/claudeHooksSetup";
import type { GeminiHooksStatus } from "@/lib/geminiHooksSetup";
import FolderSearchInput from "@/components/FolderSearchInput";

interface ProjectSettingsProps {
  isOpen: boolean;
  onClose: () => void;
  projects: Project[];
  sshHosts: string[];
  sidebarDefaultCollapsed: boolean;
}

export default function ProjectSettings({
  isOpen,
  onClose,
  projects,
  sshHosts,
  sidebarDefaultCollapsed,
}: ProjectSettingsProps) {
  const t = useTranslations("settings");
  const tc = useTranslations("common");
  const [isPending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [hooksStatusMap, setHooksStatusMap] = useState<Record<string, ClaudeHooksStatus | null>>({});
  const [geminiHooksStatusMap, setGeminiHooksStatusMap] = useState<Record<string, GeminiHooksStatus | null>>({});
  const [scanSshHost, setScanSshHost] = useState("");

  const loadHooksStatus = useCallback(async () => {
    const [claudeEntries, geminiEntries] = await Promise.all([
      Promise.all(
        projects.map(async (p) => {
          const status = await getProjectHooksStatus(p.id);
          return [p.id, status] as const;
        })
      ),
      Promise.all(
        projects.map(async (p) => {
          const status = await getProjectGeminiHooksStatus(p.id);
          return [p.id, status] as const;
        })
      ),
    ]);
    setHooksStatusMap(Object.fromEntries(claudeEntries));
    setGeminiHooksStatusMap(Object.fromEntries(geminiEntries));
  }, [projects]);

  useEffect(() => {
    if (isOpen && projects.length > 0) {
      loadHooksStatus();
    }
  }, [isOpen, projects, loadHooksStatus]);

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

  function handleInstallHooks(projectId: string) {
    startTransition(async () => {
      const result = await installProjectHooks(projectId);
      if (result.success) {
        setSuccessMessage(t("hooksInstallSuccess"));
        await loadHooksStatus();
      } else {
        setError(t("hooksInstallFailed", { error: result.error || "unknown" }));
      }
    });
  }

  function handleInstallGeminiHooks(projectId: string) {
    startTransition(async () => {
      const result = await installProjectGeminiHooks(projectId);
      if (result.success) {
        setSuccessMessage(t("geminiHooksInstallSuccess"));
        await loadHooksStatus();
      } else {
        setError(t("hooksInstallFailed", { error: result.error || "unknown" }));
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

        {/* Pane 레이아웃 설정 링크 */}
        <div className="p-4 border-b border-border-default">
          <Link
            href="/pane-layout"
            prefetch={false}
            className="flex items-center justify-between w-full px-3 py-2 text-sm bg-bg-page border border-border-default rounded-md text-text-primary hover:border-brand-primary transition-colors"
          >
            <span>{t("paneLayoutLink")}</span>
            <span className="text-text-muted">&rarr;</span>
          </Link>
        </div>

        {/* 상세 페이지 설정 */}
        <div className="p-4 border-b border-border-default">
          <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">
            {t("detailPageSection")}
          </h3>
          <label className="flex items-center justify-between cursor-pointer">
            <div>
              <span className="text-sm text-text-primary">{t("sidebarDefaultCollapsed")}</span>
              <p className="text-xs text-text-muted mt-0.5">{t("sidebarDefaultCollapsedDescription")}</p>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={sidebarDefaultCollapsed}
              onClick={() => {
                startTransition(async () => {
                  await setSidebarDefaultCollapsed(!sidebarDefaultCollapsed);
                });
              }}
              disabled={isPending}
              className={`relative inline-flex h-5 w-9 shrink-0 rounded-full border-2 border-transparent transition-colors ${
                sidebarDefaultCollapsed ? "bg-brand-primary" : "bg-border-default"
              }`}
            >
              <span
                className={`pointer-events-none inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
                  sidebarDefaultCollapsed ? "translate-x-4" : "translate-x-0"
                }`}
              />
            </button>
          </label>
        </div>

        {/* 디렉토리 스캔 등록 */}
        <div className="p-4 border-b border-border-default">
          <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">
            {t("scanTitle")}
          </h3>

          <form action={handleScan} className="space-y-3">
            {sshHosts.length > 0 && (
              <select
                name="scanSshHost"
                value={scanSshHost}
                onChange={(e) => setScanSshHost(e.target.value)}
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

            <FolderSearchInput
              name="scanPath"
              sshHost={scanSshHost || undefined}
              onSelect={() => {}}
            />

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
              {projects.map((project) => {
                const claudeHooksStatus = hooksStatusMap[project.id];
                const geminiHooksStatus = geminiHooksStatusMap[project.id];

                return (
                  <li
                    key={project.id}
                    className="p-2 bg-bg-page rounded-md"
                  >
                    <div className="flex items-center justify-between">
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
                    </div>
                    <div className="flex flex-wrap items-center gap-2 mt-1.5">
                      {project.sshHost ? (
                        <span className="text-[10px] px-1.5 py-0.5 bg-bg-surface border border-border-default rounded text-text-muted">
                          {t("hooksRemoteNotSupported")}
                        </span>
                      ) : (
                        <>
                          {/* Claude Hooks */}
                          {claudeHooksStatus?.installed ? (
                            <>
                              <span className="text-[10px] px-1.5 py-0.5 bg-status-done/15 text-status-done rounded">
                                Claude {t("hooksInstalled")}
                              </span>
                              <button
                                onClick={() => handleInstallHooks(project.id)}
                                disabled={isPending}
                                className="text-[10px] text-text-muted hover:text-text-primary transition-colors"
                              >
                                {t("reinstallHooks")}
                              </button>
                            </>
                          ) : (
                            <button
                              onClick={() => handleInstallHooks(project.id)}
                              disabled={isPending}
                              className="text-[10px] px-1.5 py-0.5 bg-brand-primary/10 text-brand-primary hover:bg-brand-primary/20 rounded transition-colors"
                            >
                              {isPending ? t("installingHooks") : `Claude ${t("installHooks")}`}
                            </button>
                          )}

                          {/* Gemini Hooks */}
                          {geminiHooksStatus?.installed ? (
                            <>
                              <span className="text-[10px] px-1.5 py-0.5 bg-status-done/15 text-status-done rounded">
                                Gemini {t("hooksInstalled")}
                              </span>
                              <button
                                onClick={() => handleInstallGeminiHooks(project.id)}
                                disabled={isPending}
                                className="text-[10px] text-text-muted hover:text-text-primary transition-colors"
                              >
                                {t("reinstallHooks")}
                              </button>
                            </>
                          ) : (
                            <button
                              onClick={() => handleInstallGeminiHooks(project.id)}
                              disabled={isPending}
                              className="text-[10px] px-1.5 py-0.5 bg-brand-primary/10 text-brand-primary hover:bg-brand-primary/20 rounded transition-colors"
                            >
                              {isPending ? t("installingHooks") : `Gemini ${t("installHooks")}`}
                            </button>
                          )}
                        </>
                      )}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
