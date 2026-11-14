"use client";

import { useEffect, useRef, useState } from "react";
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
  type ThemePreference,
} from "@/desktop/renderer/actions/appSettings";
import MobilePairingSettings from "@/components/MobilePairingSettings";
import InlineLoadError from "@/components/InlineLoadError";
import { SessionType } from "@/entities/KanbanTask";
import { Link } from "@/desktop/renderer/navigation";
import type { Project } from "@/entities/Project";
import { applyThemePreference, notifyThemePreferenceChanged } from "@/desktop/renderer/utils/theme";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import SessionTypeOptions from "./SessionTypeOptions";
import BulkSessionTypeChangeSettings from "./BulkSessionTypeChangeSettings";

const MIN_SYNC_INTERVAL_MINUTES = 1;
const MAX_SYNC_INTERVAL_MINUTES = 1440;

/** 알림 대상 상태 목록 (사용자가 직접 설정하는 todo/done은 제외) */
const STATUS_OPTIONS = [
  { value: "progress", labelKey: "progress" },
  { value: "pending", labelKey: "pending" },
  { value: "review", labelKey: "review" },
] as const;

/** 저장 상태를 따로 보이는 설정 항목. 한 항목의 저장이 다른 항목의 입력을 막지 않게 나눈다 */
type SettingKey = "theme" | "sidebar" | "sessionType" | "notifications" | "backgroundSync" | "vimMode";

/** 화면 값을 먼저 바꾸고 저장한다. 저장에 실패하면 restore로 마지막 저장값을 되돌린다 */
interface SettingChange {
  apply: () => void;
  restore: () => void;
  save: () => Promise<unknown>;
}

interface SettingSaveFailure {
  key: SettingKey;
  reason: string;
  change: SettingChange;
}

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
  onDefaultSessionTypeChange?: (sessionType: SessionType) => void;
  onVimModeEnabledChange?: (enabled: boolean) => void;
  onThemePreferenceChange?: (themePreference: ThemePreference) => void;
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
  onDefaultSessionTypeChange,
  onVimModeEnabledChange,
  onThemePreferenceChange,
  notificationSettings,
  backgroundSyncSettings,
}: ProjectSettingsProps) {
  const t = useTranslations("settings");
  const tb = useTranslations("board.columns");
  const [savingSettingKeys, setSavingSettingKeys] = useState<ReadonlySet<SettingKey>>(() => new Set());
  const [settingSaveFailure, setSettingSaveFailure] = useState<SettingSaveFailure | null>(null);
  const [selectedDefaultSessionType, setSelectedDefaultSessionType] = useState(defaultSessionType);
  const [localVimModeEnabled, setLocalVimModeEnabled] = useState(vimModeEnabled);
  const [localNotificationSettings, setLocalNotificationSettings] = useState(notificationSettings);
  const [pendingNotificationSettings, setPendingNotificationSettings] = useState<typeof notificationSettings | null>(null);
  const [localThemePreference, setLocalThemePreference] = useState<ThemePreference>(themePreference);
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

  function updateSavingSettingKey(key: SettingKey, isSaving: boolean) {
    setSavingSettingKeys((current) => {
      const next = new Set(current);
      if (isSaving) {
        next.add(key);
      } else {
        next.delete(key);
      }
      return next;
    });
  }

  /**
   * 설정을 화면에 먼저 반영하고 저장한다. 실패한 값을 유효한 설정처럼 남기지 않도록 저장값으로 되돌리고,
   * 같은 변경을 다시 시도할 수 있게 실패를 그 항목 옆에 남긴다.
   */
  function saveSetting(key: SettingKey, change: SettingChange) {
    change.apply();
    updateSavingSettingKey(key, true);
    setSettingSaveFailure((current) => (current?.key === key ? null : current));
    void change.save()
      .catch((error: unknown) => {
        change.restore();
        setSettingSaveFailure({
          key,
          reason: error instanceof Error ? error.message : String(error),
          change,
        });
      })
      .finally(() => updateSavingSettingKey(key, false));
  }

  function renderSettingSaveFailure(key: SettingKey) {
    if (settingSaveFailure?.key !== key) {
      return null;
    }

    const { change } = settingSaveFailure;
    return (
      <InlineLoadError
        className="mt-2"
        message={t("settingSaveFailed", { reason: settingSaveFailure.reason })}
        onRetry={() => saveSetting(key, change)}
      />
    );
  }

  function applyThemeSelection(nextThemePreference: ThemePreference) {
    setLocalThemePreference(nextThemePreference);
    applyThemePreference(nextThemePreference);
    notifyThemePreferenceChanged(nextThemePreference);
    onThemePreferenceChange?.(nextThemePreference);
  }

  function handleThemePreferenceChange(nextThemePreference: ThemePreference) {
    const previousThemePreference = localThemePreference;
    saveSetting("theme", {
      apply: () => applyThemeSelection(nextThemePreference),
      restore: () => applyThemeSelection(previousThemePreference),
      save: () => setThemePreference(nextThemePreference),
    });
  }

  /** 알림 설정은 저장되는 동안 이전 props가 화면 값을 덮지 않게 pending으로 붙잡아 둔다 */
  function saveNotificationSettings(nextSettings: typeof notificationSettings, save: () => Promise<unknown>) {
    const previousSettings = localNotificationSettings;
    saveSetting("notifications", {
      apply: () => {
        setLocalNotificationSettings(nextSettings);
        setPendingNotificationSettings(nextSettings);
      },
      restore: () => {
        setLocalNotificationSettings(previousSettings);
        setPendingNotificationSettings(null);
      },
      save,
    });
  }

  function saveBackgroundSyncSettings(nextSettings: typeof backgroundSyncSettings, save: () => Promise<unknown>) {
    const previousSettings = localBackgroundSyncSettings;
    saveSetting("backgroundSync", {
      apply: () => {
        setLocalBackgroundSyncSettings(nextSettings);
        setPendingBackgroundSyncSettings(nextSettings);
      },
      restore: () => {
        setLocalBackgroundSyncSettings(previousSettings);
        setPendingBackgroundSyncSettings(null);
        setBackgroundSyncIntervalInputValue(formatSyncIntervalMinutes(previousSettings.intervalMs));
      },
      save,
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
                ["mobile", t("mobilePairingSection")],
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
                  disabled={savingSettingKeys.has("theme")}
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
          {renderSettingSaveFailure("theme")}
        </div>

        {/* AI 계정 관리 링크 */}
        <div className="p-4 border-b border-border-default">
          <Link
            href="/ai-accounts"
            prefetch={false}
            className="flex items-center justify-between w-full px-3 py-2 text-sm bg-bg-page border border-border-default rounded-md text-text-primary hover:border-brand-primary transition-colors"
          >
            <span>{t("aiAccountsLink")}</span>
            <span className="text-text-muted">&rarr;</span>
          </Link>
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
                const previousCollapsed = localSidebarDefaultCollapsed;
                const nextCollapsed = !previousCollapsed;
                saveSetting("sidebar", {
                  apply: () => setLocalSidebarDefaultCollapsed(nextCollapsed),
                  restore: () => setLocalSidebarDefaultCollapsed(previousCollapsed),
                  save: () => setSidebarDefaultCollapsed(nextCollapsed),
                });
              }}
              disabled={savingSettingKeys.has("sidebar")}
              aria-busy={savingSettingKeys.has("sidebar") || undefined}
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
          {renderSettingSaveFailure("sidebar")}
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
                const previousSessionType = selectedDefaultSessionType;
                const nextSessionType = e.target.value as SessionType;
                saveSetting("sessionType", {
                  apply: () => setSelectedDefaultSessionType(nextSessionType),
                  restore: () => setSelectedDefaultSessionType(previousSessionType),
                  save: async () => {
                    await setDefaultSessionType(nextSessionType);
                    onDefaultSessionTypeChange?.(nextSessionType);
                  },
                });
              }}
              disabled={savingSettingKeys.has("sessionType")}
              aria-busy={savingSettingKeys.has("sessionType") || undefined}
              className="px-2 py-1 text-sm bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
            >
              <SessionTypeOptions />
            </select>
          </div>
          {renderSettingSaveFailure("sessionType")}
          <BulkSessionTypeChangeSettings defaultSessionType={selectedDefaultSessionType} />
        </div>

        <MobilePairingSettings />

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
                 saveNotificationSettings(
                   { ...localNotificationSettings, isEnabled: nextEnabled },
                   () => setNotificationEnabled(nextEnabled),
                 );
               }}
               disabled={savingSettingKeys.has("notifications")}
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
                    disabled={savingSettingKeys.has("notifications")}
                    onClick={() => {
                      const nextStatuses = isSelected
                        ? localNotificationSettings.enabledStatuses.filter((s) => s !== value)
                        : [...localNotificationSettings.enabledStatuses, value];
                      saveNotificationSettings(
                        { ...localNotificationSettings, enabledStatuses: nextStatuses },
                        () => setNotificationStatuses(nextStatuses),
                      );
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
          {renderSettingSaveFailure("notifications")}
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
                saveBackgroundSyncSettings(
                  { ...localBackgroundSyncSettings, isEnabled: nextEnabled },
                  () => setBackgroundSyncEnabled(nextEnabled),
                );
              }}
              disabled={savingSettingKeys.has("backgroundSync")}
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

                  saveBackgroundSyncSettings(
                    { ...localBackgroundSyncSettings, intervalMs: nextIntervalMs },
                    () => saveBackgroundSyncIntervalMs(nextIntervalMs),
                  );
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
          {renderSettingSaveFailure("backgroundSync")}
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
                const previousEnabled = localVimModeEnabled;
                const nextEnabled = !previousEnabled;
                saveSetting("vimMode", {
                  apply: () => {
                    setLocalVimModeEnabled(nextEnabled);
                    onVimModeEnabledChange?.(nextEnabled);
                  },
                  restore: () => {
                    setLocalVimModeEnabled(previousEnabled);
                    onVimModeEnabledChange?.(previousEnabled);
                  },
                  save: () => setVimModeEnabled(nextEnabled),
                });
              }}
              disabled={savingSettingKeys.has("vimMode")}
              aria-busy={savingSettingKeys.has("vimMode") || undefined}
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
          {renderSettingSaveFailure("vimMode")}

          <Link
            href="/settings/shortcuts"
            prefetch={false}
            className="mt-4 flex items-center justify-between w-full px-3 py-2 text-sm bg-bg-page border border-border-default rounded-md text-text-primary hover:border-brand-primary transition-colors"
          >
            <span>{t("shortcutsLink")}</span>
            <span className="text-text-muted">&rarr;</span>
          </Link>
        </div>

        </div>
      </div>
    </div>
  );
}
