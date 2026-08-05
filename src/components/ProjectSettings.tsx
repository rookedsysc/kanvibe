"use client";

import { useEffect, useRef, useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import {
  setSidebarDefaultCollapsed,
  setNotificationEnabled,
  setNotificationStatuses,
  setDefaultSessionType,
  setThemePreference,
  setVimModeEnabled,
  setBackgroundSyncEnabled,
  setBackgroundSyncIntervalMs,
  setTerminalOpacity,
  type ThemePreference,
} from "@/desktop/renderer/actions/appSettings";
import { SessionType } from "@/entities/KanbanTask";
import { Link } from "@/desktop/renderer/navigation";
import type { Project } from "@/entities/Project";
import { applyThemePreference, notifyThemePreferenceChanged } from "@/desktop/renderer/utils/theme";
import { applyTerminalTransparency } from "@/desktop/renderer/utils/terminalTransparency";
import { MIN_TERMINAL_OPACITY, OPAQUE_TERMINAL_OPACITY } from "@/lib/terminalOpacity";
import { useEscapeKey } from "@/hooks/useEscapeKey";

const MIN_SYNC_INTERVAL_MINUTES = 1;
const MAX_SYNC_INTERVAL_MINUTES = 1440;

/** 투명도를 백분율 슬라이더로 다루기 위한 배율과 눈금 간격 */
const TERMINAL_OPACITY_PERCENT_SCALE = 100;
const TERMINAL_OPACITY_PERCENT_STEP = 5;

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
  projects?: Project[];
  sshHosts?: string[];
  sidebarDefaultCollapsed: boolean;
  defaultSessionType: SessionType;
  vimModeEnabled?: boolean;
  themePreference?: ThemePreference;
  terminalOpacity?: number;
  onDefaultSessionTypeChange?: (sessionType: SessionType) => void;
  onVimModeEnabledChange?: (enabled: boolean) => void;
  onThemePreferenceChange?: (themePreference: ThemePreference) => void;
  onTerminalOpacityChange?: (terminalOpacity: number) => void;
  notificationSettings: { isEnabled: boolean; enabledStatuses: string[] };
  backgroundSyncSettings: { isEnabled: boolean; intervalMs: number };
}

function areNotificationSettingsEqual(
  left: { isEnabled: boolean; enabledStatuses: string[] },
  right: { isEnabled: boolean; enabledStatuses: string[] },
) {
  return left.isEnabled === right.isEnabled
    && left.enabledStatuses.length === right.enabledStatuses.length
    && left.enabledStatuses.every((status, index) => status === right.enabledStatuses[index]);
}

function areBackgroundSyncSettingsEqual(
  left: { isEnabled: boolean; intervalMs: number },
  right: { isEnabled: boolean; intervalMs: number },
) {
  return left.isEnabled === right.isEnabled && left.intervalMs === right.intervalMs;
}

function formatSyncIntervalMinutes(intervalMs: number) {
  return String(Math.round(intervalMs / 60_000));
}

function parseSyncIntervalMinutes(value: string): number | null {
  const normalizedValue = value.trim();
  if (!/^\d+$/.test(normalizedValue)) {
    return null;
  }

  const minutes = Number(normalizedValue);
  if (!Number.isSafeInteger(minutes) || minutes < MIN_SYNC_INTERVAL_MINUTES || minutes > MAX_SYNC_INTERVAL_MINUTES) {
    return null;
  }

  return minutes;
}

export default function ProjectSettings({
  isOpen,
  onClose,
  variant = "modal",
  sidebarDefaultCollapsed,
  defaultSessionType,
  vimModeEnabled = true,
  themePreference = "system",
  terminalOpacity = OPAQUE_TERMINAL_OPACITY,
  onDefaultSessionTypeChange,
  onVimModeEnabledChange,
  onThemePreferenceChange,
  onTerminalOpacityChange,
  notificationSettings,
  backgroundSyncSettings,
}: ProjectSettingsProps) {
  const t = useTranslations("settings");
  const tb = useTranslations("board.columns");
  const [isPending, startTransition] = useTransition();
  const [isNotificationPending, startNotificationTransition] = useTransition();
  const [selectedDefaultSessionType, setSelectedDefaultSessionType] = useState(defaultSessionType);
  const [localVimModeEnabled, setLocalVimModeEnabled] = useState(vimModeEnabled);
  const [localNotificationSettings, setLocalNotificationSettings] = useState(notificationSettings);
  const [pendingNotificationSettings, setPendingNotificationSettings] = useState<typeof notificationSettings | null>(null);
  const [localThemePreference, setLocalThemePreference] = useState<ThemePreference>(themePreference);
  const [localTerminalOpacity, setLocalTerminalOpacity] = useState(terminalOpacity);
  const [localSidebarDefaultCollapsed, setLocalSidebarDefaultCollapsed] = useState(sidebarDefaultCollapsed);
  const [localBackgroundSyncSettings, setLocalBackgroundSyncSettings] = useState(backgroundSyncSettings);
  const [pendingBackgroundSyncSettings, setPendingBackgroundSyncSettings] = useState<typeof backgroundSyncSettings | null>(null);
  const [backgroundSyncIntervalInputValue, setBackgroundSyncIntervalInputValue] = useState(
    () => formatSyncIntervalMinutes(backgroundSyncSettings.intervalMs),
  );
  const backgroundSyncIntervalSaveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const [shouldUseMacTitlebarLayout, setShouldUseMacTitlebarLayout] = useState(false);
  const isPage = variant === "page";

  function saveBackgroundSyncIntervalMs(nextIntervalMs: number) {
    const nextSave = backgroundSyncIntervalSaveQueueRef.current
      .catch(() => undefined)
      .then(() => setBackgroundSyncIntervalMs(nextIntervalMs));

    backgroundSyncIntervalSaveQueueRef.current = nextSave;
    return nextSave;
  }

  useEffect(() => {
    setSelectedDefaultSessionType(defaultSessionType);
  }, [defaultSessionType]);

  useEffect(() => {
    setLocalVimModeEnabled(vimModeEnabled);
  }, [vimModeEnabled]);

  useEffect(() => {
    setLocalSidebarDefaultCollapsed(sidebarDefaultCollapsed);
  }, [sidebarDefaultCollapsed]);

  useEffect(() => {
    if (pendingBackgroundSyncSettings && !areBackgroundSyncSettingsEqual(backgroundSyncSettings, pendingBackgroundSyncSettings)) {
      return;
    }

    setLocalBackgroundSyncSettings(backgroundSyncSettings);
    setPendingBackgroundSyncSettings(null);
    setBackgroundSyncIntervalInputValue(formatSyncIntervalMinutes(backgroundSyncSettings.intervalMs));
  }, [backgroundSyncSettings, pendingBackgroundSyncSettings]);

  useEffect(() => {
    setLocalThemePreference(themePreference);
  }, [themePreference]);

  useEffect(() => {
    setLocalTerminalOpacity(terminalOpacity);
  }, [terminalOpacity]);

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

  function handleThemePreferenceChange(nextThemePreference: ThemePreference) {
    setLocalThemePreference(nextThemePreference);
    applyThemePreference(nextThemePreference);
    notifyThemePreferenceChanged(nextThemePreference);
    onThemePreferenceChange?.(nextThemePreference);
    startTransition(async () => {
      await setThemePreference(nextThemePreference);
    });
  }

  function handleTerminalOpacityPercentChange(nextOpacityPercent: number) {
    const nextTerminalOpacity = nextOpacityPercent / TERMINAL_OPACITY_PERCENT_SCALE;
    setLocalTerminalOpacity(nextTerminalOpacity);
    applyTerminalTransparency(nextTerminalOpacity);
    onTerminalOpacityChange?.(nextTerminalOpacity);
    startTransition(async () => {
      await setTerminalOpacity(nextTerminalOpacity);
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
                ["background-sync", t("backgroundSyncSection")],
                ["keyboard", t("keyboardSection")],
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

          <div className="mt-4 flex items-center justify-between gap-4">
            <div>
              <label htmlFor="terminal-opacity" className="text-sm text-text-primary">{t("terminalOpacity")}</label>
              <p className="text-xs text-text-muted mt-0.5">{t("terminalOpacityDescription")}</p>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <input
                id="terminal-opacity"
                type="range"
                min={MIN_TERMINAL_OPACITY * TERMINAL_OPACITY_PERCENT_SCALE}
                max={OPAQUE_TERMINAL_OPACITY * TERMINAL_OPACITY_PERCENT_SCALE}
                step={TERMINAL_OPACITY_PERCENT_STEP}
                value={Math.round(localTerminalOpacity * TERMINAL_OPACITY_PERCENT_SCALE)}
                onChange={(event) => handleTerminalOpacityPercentChange(Number(event.target.value))}
                disabled={isPending}
                className="w-28 accent-brand-primary"
              />
              <span className="w-10 text-right text-xs text-text-muted tabular-nums">
                {Math.round(localTerminalOpacity * TERMINAL_OPACITY_PERCENT_SCALE)}%
              </span>
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

        {/* 백그라운드 Sync 설정 */}
        <div id="background-sync" className="p-4 border-b border-border-default">
          <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">
            {t("backgroundSyncSection")}
          </h3>

          {/* 활성화 토글 */}
          <label className="flex items-center justify-between cursor-pointer">
            <div>
              <span className="text-sm text-text-primary">{t("backgroundSyncEnabled")}</span>
              <p className="text-xs text-text-muted mt-0.5">{t("backgroundSyncEnabledDescription")}</p>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={localBackgroundSyncSettings.isEnabled}
              onClick={() => {
                const nextEnabled = !localBackgroundSyncSettings.isEnabled;
                const nextSettings = { ...localBackgroundSyncSettings, isEnabled: nextEnabled };
                setLocalBackgroundSyncSettings(nextSettings);
                setPendingBackgroundSyncSettings(nextSettings);
                startTransition(async () => {
                  await setBackgroundSyncEnabled(nextEnabled);
                });
              }}
              disabled={isPending}
              className={`relative inline-flex h-5 w-9 shrink-0 rounded-full border-2 border-transparent transition-colors ${
                localBackgroundSyncSettings.isEnabled ? "bg-brand-primary" : "bg-border-default"
              }`}
            >
              <span
                className={`pointer-events-none inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
                  localBackgroundSyncSettings.isEnabled ? "translate-x-4" : "translate-x-0"
                }`}
              />
            </button>
          </label>

          {/* 주기 입력 */}
          <div className={`mt-4 flex items-center justify-between ${!localBackgroundSyncSettings.isEnabled ? "opacity-40 pointer-events-none" : ""}`}>
            <div>
              <span className="text-sm text-text-primary">{t("backgroundSyncInterval")}</span>
              <p className="text-xs text-text-muted mt-0.5">{t("backgroundSyncIntervalDescription")}</p>
            </div>
            <div className="flex items-center gap-1.5">
              <input
                type="number"
                min={MIN_SYNC_INTERVAL_MINUTES}
                max={MAX_SYNC_INTERVAL_MINUTES}
                step={1}
                inputMode="numeric"
                value={backgroundSyncIntervalInputValue}
                disabled={!localBackgroundSyncSettings.isEnabled}
                onChange={(e) => {
                  const nextInputValue = e.target.value;
                  setBackgroundSyncIntervalInputValue(nextInputValue);

                  const minutes = parseSyncIntervalMinutes(nextInputValue);
                  if (minutes === null) return;

                  const nextIntervalMs = minutes * 60_000;
                  if (nextIntervalMs === localBackgroundSyncSettings.intervalMs) return;

                  const nextSettings = { ...localBackgroundSyncSettings, intervalMs: nextIntervalMs };
                  setLocalBackgroundSyncSettings(nextSettings);
                  setPendingBackgroundSyncSettings(nextSettings);
                  startTransition(async () => {
                    await saveBackgroundSyncIntervalMs(nextIntervalMs);
                  });
                }}
                onBlur={() => {
                  if (parseSyncIntervalMinutes(backgroundSyncIntervalInputValue) === null) {
                    setBackgroundSyncIntervalInputValue(formatSyncIntervalMinutes(localBackgroundSyncSettings.intervalMs));
                  }
                }}
                className="w-20 px-2 py-1 text-sm bg-bg-page border border-border-default rounded-md text-text-primary text-right focus:outline-none focus:border-brand-primary transition-colors disabled:opacity-50"
              />
              <span className="text-sm text-text-muted">{t("backgroundSyncIntervalUnit")}</span>
            </div>
          </div>
        </div>

        {/* 키보드 설정 */}
        <div id="keyboard" className="p-4 border-b border-border-default">
          <h3 className="text-xs text-text-muted uppercase tracking-wide mb-3">
            {t("keyboardSection")}
          </h3>
          <label className="flex items-center justify-between cursor-pointer">
            <div>
              <span className="text-sm text-text-primary">{t("vimModeEnabled")}</span>
              <p className="text-xs text-text-muted mt-0.5">{t("vimModeEnabledDescription")}</p>
            </div>
            <button
              type="button"
              role="switch"
              aria-label={t("vimModeEnabled")}
              aria-checked={localVimModeEnabled}
              onClick={() => {
                const nextEnabled = !localVimModeEnabled;
                setLocalVimModeEnabled(nextEnabled);
                onVimModeEnabledChange?.(nextEnabled);
                startTransition(async () => {
                  await setVimModeEnabled(nextEnabled);
                });
              }}
              disabled={isPending}
              className={`relative inline-flex h-5 w-9 shrink-0 rounded-full border-2 border-transparent transition-colors ${
                localVimModeEnabled ? "bg-brand-primary" : "bg-border-default"
              }`}
            >
              <span
                className={`pointer-events-none inline-block h-4 w-4 rounded-full bg-white shadow transition-transform ${
                  localVimModeEnabled ? "translate-x-4" : "translate-x-0"
                }`}
              />
            </button>
          </label>
        </div>

        </div>
      </div>
    </div>
  );
}
