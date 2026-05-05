"use client";

import { useEffect, useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import {
  deleteProject,
  scanAndRegisterProjects,
  type ScanResult,
} from "@/desktop/renderer/actions/project";
import {
  setSidebarDefaultCollapsed,
  setNotificationEnabled,
  setNotificationStatuses,
  setDefaultSessionType,
  setThemePreference,
  type ThemePreference,
} from "@/desktop/renderer/actions/appSettings";
import { SessionType } from "@/entities/KanbanTask";
import { Link } from "@/desktop/renderer/navigation";
import type { Project } from "@/entities/Project";
import FolderSearchInput from "@/components/FolderSearchInput";
import { applyThemePreference, notifyThemePreferenceChanged } from "@/desktop/renderer/utils/theme";
import { useEscapeKey } from "@/hooks/useEscapeKey";

/** 알림 대상 상태 목록 (사용자가 직접 설정하는 todo/done은 제외) */
const STATUS_OPTIONS = [
  { value: "progress", labelKey: "progress" },
  { value: "pending", labelKey: "pending" },
  { value: "review", labelKey: "review" },
] as const;

interface ProjectSettingsProps {
  isOpen?: boolean;
  onClose?: () => void;
  variant?: "modal" | "page";
  projects: Project[];
  sshHosts: string[];
  sidebarDefaultCollapsed: boolean;
  defaultSessionType: SessionType;
  themePreference?: ThemePreference;
  onDefaultSessionTypeChange?: (sessionType: SessionType) => void;
  onThemePreferenceChange?: (themePreference: ThemePreference) => void;
  notificationSettings: { isEnabled: boolean; enabledStatuses: string[] };
}

function areNotificationSettingsEqual(
  left: { isEnabled: boolean; enabledStatuses: string[] },
  right: { isEnabled: boolean; enabledStatuses: string[] },
) {
  return left.isEnabled === right.isEnabled
    && left.enabledStatuses.length === right.enabledStatuses.length
    && left.enabledStatuses.every((status, index) => status === right.enabledStatuses[index]);
}

export default function ProjectSettings({
  isOpen,
  onClose,
  variant = "modal",
  projects,
  sshHosts,
  sidebarDefaultCollapsed,
  defaultSessionType,
  themePreference = "system",
  onDefaultSessionTypeChange,
  onThemePreferenceChange,
  notificationSettings,
}: ProjectSettingsProps) {
  const t = useTranslations("settings");
  const tc = useTranslations("common");
  const tb = useTranslations("board.columns");
  const [isPending, startTransition] = useTransition();
  const [isNotificationPending, startNotificationTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);
  const [scanResult, setScanResult] = useState<ScanResult | null>(null);
  const [scanSshHost, setScanSshHost] = useState("");
  const [selectedDefaultSessionType, setSelectedDefaultSessionType] = useState(defaultSessionType);
  const [localNotificationSettings, setLocalNotificationSettings] = useState(notificationSettings);
  const [pendingNotificationSettings, setPendingNotificationSettings] = useState<typeof notificationSettings | null>(null);
  const [localThemePreference, setLocalThemePreference] = useState<ThemePreference>(themePreference);
  const [localSidebarDefaultCollapsed, setLocalSidebarDefaultCollapsed] = useState(sidebarDefaultCollapsed);
  const [shouldUseMacTitlebarLayout, setShouldUseMacTitlebarLayout] = useState(false);
  const isPage = variant === "page";

  useEffect(() => {
    setSelectedDefaultSessionType(defaultSessionType);
  }, [defaultSessionType]);

  useEffect(() => {
    setLocalSidebarDefaultCollapsed(sidebarDefaultCollapsed);
  }, [sidebarDefaultCollapsed]);

  useEffect(() => {
    setLocalThemePreference(themePreference);
  }, [themePreference]);

  useEffect(() => {
    const isDesktopApp = window.kanvibeDesktop?.isDesktop === true;
    const isMacDesktop = navigator.userAgent.includes("Mac") || navigator.platform.toLowerCase().includes("mac");
    setShouldUseMacTitlebarLayout(isDesktopApp && isMacDesktop);
  }, []);

  useEffect(() => {
    if (pendingNotificationSettings && !areNotificationSettingsEqual(notificationSettings, pendingNotificationSettings)) {
      return;
    }

    setLocalNotificationSettings(notificationSettings);
    setPendingNotificationSettings(null);
  }, [notificationSettings, pendingNotificationSettings]);

  useEscapeKey(() => onClose?.(), { enabled: !isPage && !!isOpen });

  if (!isPage && !isOpen) return null;

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

  function handleThemePreferenceChange(nextThemePreference: ThemePreference) {
    setLocalThemePreference(nextThemePreference);
    applyThemePreference(nextThemePreference);
    notifyThemePreferenceChanged(nextThemePreference);
    onThemePreferenceChange?.(nextThemePreference);
    startTransition(async () => {
      await setThemePreference(nextThemePreference);
    });
  }

  return (
    <div className={isPage ? "min-h-screen w-full bg-bg-page text-text-primary" : "fixed inset-0 z-[400] flex items-start justify-end pt-14 pr-4"}>
      {!isPage ? <div className="fixed inset-0 bg-bg-overlay" onClick={onClose} /> : null}
      <div className={isPage
        ? "grid min-h-screen w-full grid-cols-1 lg:grid-cols-[240px_minmax(0,1fr)]"
        : "relative w-96 max-h-[80vh] overflow-y-auto rounded-lg border border-border-default bg-bg-surface shadow-lg"
      }>
        {isPage ? (
          <aside className={`border-b border-border-default bg-bg-surface/80 px-4 pb-4 lg:sticky lg:top-0 lg:h-screen lg:border-b-0 lg:border-r lg:pb-6 ${
            shouldUseMacTitlebarLayout ? "pt-16 lg:pt-16" : "pt-4 lg:pt-6"
          }`}>
            <Link href="/" className="mb-8 inline-flex items-center gap-3 text-xs font-medium text-text-muted hover:text-text-primary">
              <span aria-hidden="true">←</span>
              Board
            </Link>
            <nav className="flex gap-1 overflow-x-auto text-sm lg:block lg:space-y-1 lg:overflow-visible">
              {[
                ["appearance", t("appearanceSection")],
                ["detail", t("detailPageSection")],
                ["creation", t("taskCreationSection")],
                ["notifications", t("notificationSection")],
                ["projects", t("projectList")],
              ].map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => document.getElementById(id)?.scrollIntoView({ block: "start", behavior: "smooth" })}
                  className="block shrink-0 rounded-md px-3 py-2 text-text-secondary transition-colors hover:bg-bg-page hover:text-text-primary"
                >
                  {label}
                </button>
              ))}
            </nav>
          </aside>
        ) : null}

        <div className={isPage ? "min-h-screen min-w-0 px-4 py-5 sm:px-8 sm:py-6" : ""}>
          <div className="flex items-center justify-between border-b border-border-default px-4 py-4">
            <div>
              <h2 className="text-sm font-semibold text-text-primary">{t("title")}</h2>
              {isPage ? <p className="mt-1 text-xs text-text-muted">{t("pageDescription")}</p> : null}
            </div>
            {!isPage ? (
              <button
                type="button"
                onClick={onClose}
                className="text-text-muted hover:text-text-primary text-lg"
              >
                &times;
              </button>
            ) : null}
          </div>

        {/* 외관 설정 */}
        <div id="appearance" className="p-4 border-b border-border-default">
          <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">
            {t("appearanceSection")}
          </h3>
          <div className="flex items-center justify-between gap-4">
            <div>
              <span className="text-sm text-text-primary">{t("themePreference")}</span>
              <p className="text-xs text-text-muted mt-0.5">{t("themePreferenceDescription")}</p>
            </div>
            <div className="inline-flex rounded-md border border-border-default bg-bg-page p-0.5">
              {(["system", "dark", "light"] as const).map((value) => (
                <button
                  key={value}
                  type="button"
                  onClick={() => handleThemePreferenceChange(value)}
                  disabled={isPending}
                  className={`rounded-[5px] px-2.5 py-1 text-xs font-medium transition-colors ${
                    localThemePreference === value
                      ? "bg-bg-surface text-text-primary shadow-xs"
                      : "text-text-muted hover:text-text-primary"
                  }`}
                >
                  {t(`theme.${value}`)}
                </button>
              ))}
            </div>
          </div>
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
        <div id="detail" className="p-4 border-b border-border-default">
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
              aria-checked={localSidebarDefaultCollapsed}
              onClick={() => {
                const nextCollapsed = !localSidebarDefaultCollapsed;
                setLocalSidebarDefaultCollapsed(nextCollapsed);
                startTransition(async () => {
                  await setSidebarDefaultCollapsed(nextCollapsed);
                });
              }}
              disabled={isPending}
              className={`relative inline-flex h-5 w-9 shrink-0 rounded-full border-2 border-transparent transition-colors ${
                localSidebarDefaultCollapsed ? "bg-brand-primary" : "bg-border-default"
              }`}
            >
              <span
                className={`pointer-events-none inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
                  localSidebarDefaultCollapsed ? "translate-x-4" : "translate-x-0"
                }`}
              />
            </button>
          </label>
        </div>

        {/* 작업 생성 설정 */}
        <div id="creation" className="p-4 border-b border-border-default">
          <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">
            {t("taskCreationSection")}
          </h3>
          <div className="flex items-center justify-between">
            <div>
              <span className="text-sm text-text-primary">{t("defaultSessionType")}</span>
              <p className="text-xs text-text-muted mt-0.5">{t("defaultSessionTypeDescription")}</p>
            </div>
            <select
              value={selectedDefaultSessionType}
              onChange={(e) => {
                const nextSessionType = e.target.value as SessionType;
                setSelectedDefaultSessionType(nextSessionType);
                startTransition(async () => {
                  await setDefaultSessionType(nextSessionType);
                  onDefaultSessionTypeChange?.(nextSessionType);
                });
              }}
              disabled={isPending}
              className="px-2 py-1 text-sm bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
            >
              <option value="tmux">tmux</option>
              <option value="zellij">zellij</option>
            </select>
          </div>
        </div>

        {/* 알림 설정 */}
        <div id="notifications" className="p-4 border-b border-border-default">
          <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">
            {t("notificationSection")}
          </h3>

          {/* 전역 토글 */}
          <label className="flex items-center justify-between cursor-pointer">
            <div>
              <span className="text-sm text-text-primary">{t("notificationEnabled")}</span>
              <p className="text-xs text-text-muted mt-0.5">{t("notificationEnabledDescription")}</p>
            </div>
            <button
              type="button"
              role="switch"
               aria-checked={localNotificationSettings.isEnabled}
               onClick={() => {
                 const nextEnabled = !localNotificationSettings.isEnabled;
                  const nextSettings = { ...localNotificationSettings, isEnabled: nextEnabled };
                  setLocalNotificationSettings(nextSettings);
                  setPendingNotificationSettings(nextSettings);
                  startNotificationTransition(async () => {
                    await setNotificationEnabled(nextEnabled);
                  });
               }}
               disabled={isNotificationPending}
               className={`relative inline-flex h-5 w-9 shrink-0 rounded-full border-2 border-transparent transition-colors ${
                 localNotificationSettings.isEnabled ? "bg-brand-primary" : "bg-border-default"
               }`}
            >
              <span
                className={`pointer-events-none inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
                  localNotificationSettings.isEnabled ? "translate-x-4" : "translate-x-0"
                }`}
              />
            </button>
          </label>

          {/* 상태별 필터 — 칩 토글 */}
          <div className={`mt-4 ${!localNotificationSettings.isEnabled ? "opacity-40 pointer-events-none" : ""}`}>
            <div className="mb-2">
              <span className="text-sm text-text-primary">{t("notificationStatusFilter")}</span>
              <p className="text-xs text-text-muted mt-0.5">{t("notificationStatusFilterDescription")}</p>
            </div>
            <div className="flex flex-wrap gap-1.5">
              {STATUS_OPTIONS.map(({ value, labelKey }) => {
                const isSelected = localNotificationSettings.enabledStatuses.includes(value);
                return (
                  <button
                    key={value}
                    type="button"
                    disabled={isNotificationPending}
                    onClick={() => {
                      const nextStatuses = isSelected
                        ? localNotificationSettings.enabledStatuses.filter((s) => s !== value)
                        : [...localNotificationSettings.enabledStatuses, value];
                      const nextSettings = { ...localNotificationSettings, enabledStatuses: nextStatuses };
                      setLocalNotificationSettings(nextSettings);
                      setPendingNotificationSettings(nextSettings);
                      startNotificationTransition(async () => {
                        await setNotificationStatuses(nextStatuses);
                      });
                    }}
                    className={`px-2.5 py-1 text-xs rounded-full border transition-colors ${
                      isSelected
                        ? "bg-brand-primary/15 border-brand-primary text-brand-primary"
                        : "bg-bg-page border-border-default text-text-muted hover:border-border-strong"
                    }`}
                  >
                    {tb(labelKey)}
                  </button>
                );
              })}
            </div>
          </div>
        </div>

        {/* 디렉토리 스캔 등록 */}
        <div id="projects" className="p-4 border-b border-border-default">
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
                  </li>
                );
              })}
            </ul>
          )}
        </div>
        </div>
      </div>
    </div>
  );
}
