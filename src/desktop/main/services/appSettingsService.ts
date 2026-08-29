import { getAppSettingsRepository } from "@/lib/database";
import { SessionType } from "@/entities/KanbanTask";
import {
  collectShortcutOverrides,
  parseShortcutOverrides,
  resolveShortcutBindings,
  type ShortcutBindings,
} from "@/desktop/shared/shortcutBindings";
import {
  canonicalizeShortcutForPlatform,
  getShortcutPlatformFromProcessPlatform,
} from "@/desktop/shared/keyboardShortcut";
import {
  parseBoardSortPreference,
  serializeBoardSortPreference,
  type BoardSortPreference,
} from "@/desktop/shared/boardSort";

const SIDEBAR_COLLAPSED_KEY = "sidebar_default_collapsed";
const SIDEBAR_HINT_DISMISSED_KEY = "sidebar_hint_dismissed";
const NOTIFICATION_ENABLED_KEY = "notification_enabled";
const NOTIFICATION_STATUSES_KEY = "notification_statuses";
const NOTIFICATION_UNREAD_ONLY_KEY = "notification_unread_only";
const RELEASE_UPDATE_DISMISSED_VERSIONS_KEY = "release_update_dismissed_versions";

/** 기본 알림 대상 상태 (사용자가 직접 설정하는 todo/done은 제외) */
const DEFAULT_NOTIFICATION_STATUSES = ["progress", "pending", "review"];

/**
 * 키로 앱 설정값을 조회한다.
 * @param key 설정 키
 * @returns 설정값 문자열, 없으면 null
 */
export async function getAppSetting(key: string): Promise<string | null> {
  const repo = await getAppSettingsRepository();
  const setting = await repo.findOne({ where: { key } });
  return setting?.value ?? null;
}

/**
 * 앱 설정값을 저장한다. 기존 키가 있으면 업데이트, 없으면 생성한다.
 * @param key 설정 키
 * @param value 설정값
 */
export async function setAppSetting(key: string, value: string): Promise<void> {
  const repo = await getAppSettingsRepository();
  const existing = await repo.findOne({ where: { key } });

  if (existing) {
    existing.value = value;
    await repo.save(existing);
  } else {
    const setting = repo.create({ key, value });
    await repo.save(setting);
  }
}

/** 사이드바 기본 접힘 상태를 조회한다 */
export async function getSidebarDefaultCollapsed(): Promise<boolean> {
  const value = await getAppSetting(SIDEBAR_COLLAPSED_KEY);
  return value === "true";
}

/** 사이드바 기본 접힘 상태를 저장한다 */
export async function setSidebarDefaultCollapsed(collapsed: boolean): Promise<void> {
  await setAppSetting(SIDEBAR_COLLAPSED_KEY, String(collapsed));
}

/** 사이드바 힌트 숨김 여부를 조회한다 */
export async function getSidebarHintDismissed(): Promise<boolean> {
  const value = await getAppSetting(SIDEBAR_HINT_DISMISSED_KEY);
  return value === "true";
}

/** 사이드바 힌트를 다시 보지 않기로 설정한다 */
export async function dismissSidebarHint(): Promise<void> {
  await setAppSetting(SIDEBAR_HINT_DISMISSED_KEY, "true");
}

const DONE_ALERT_DISMISSED_KEY = "done_alert_dismissed";

/** Done 이동 경고 다시 묻지 않기 여부를 조회한다 */
export async function getDoneAlertDismissed(): Promise<boolean> {
  const value = await getAppSetting(DONE_ALERT_DISMISSED_KEY);
  return value === "true";
}

/** Done 이동 경고를 다시 묻지 않기로 설정한다 */
export async function dismissDoneAlert(): Promise<void> {
  await setAppSetting(DONE_ALERT_DISMISSED_KEY, "true");
}

function parseDismissedReleaseVersions(value: string | null): string[] {
  if (!value) {
    return [];
  }

  try {
    const parsedValue = JSON.parse(value);
    if (!Array.isArray(parsedValue)) {
      return [];
    }

    return Array.from(new Set(
      parsedValue
        .filter((version): version is string => typeof version === "string")
        .map((version) => version.trim())
        .filter(Boolean),
    ));
  } catch {
    return [];
  }
}

/** 다시 보지 않기로 설정한 release version 목록을 조회한다 */
export async function getReleaseUpdateDismissedVersions(): Promise<string[]> {
  const value = await getAppSetting(RELEASE_UPDATE_DISMISSED_VERSIONS_KEY);
  return parseDismissedReleaseVersions(value);
}

/** 특정 release version의 업데이트 dialog를 다시 보지 않기로 저장한다 */
export async function dismissReleaseUpdateVersion(version: string): Promise<void> {
  const normalizedVersion = version.trim();
  if (!normalizedVersion) {
    return;
  }

  const dismissedVersions = await getReleaseUpdateDismissedVersions();
  if (dismissedVersions.includes(normalizedVersion)) {
    return;
  }

  await setAppSetting(
    RELEASE_UPDATE_DISMISSED_VERSIONS_KEY,
    JSON.stringify([...dismissedVersions, normalizedVersion]),
  );
}

/** 알림 설정을 조회한다. 키가 없으면 기본값(전체 활성화)을 반환한다 */
export async function getNotificationSettings(): Promise<{
  isEnabled: boolean;
  enabledStatuses: string[];
}> {
  const [enabledValue, statusesValue] = await Promise.all([
    getAppSetting(NOTIFICATION_ENABLED_KEY),
    getAppSetting(NOTIFICATION_STATUSES_KEY),
  ]);

  const isEnabled = enabledValue !== "false";
  let enabledStatuses = DEFAULT_NOTIFICATION_STATUSES;
  if (statusesValue) {
    try {
      enabledStatuses = JSON.parse(statusesValue);
    } catch {
      /* 파싱 실패 시 기본값 사용 */
    }
  }

  return { isEnabled, enabledStatuses };
}

/** 알림 전역 활성화 상태를 저장한다 */
export async function setNotificationEnabled(enabled: boolean): Promise<void> {
  await setAppSetting(NOTIFICATION_ENABLED_KEY, String(enabled));
}

/** 알림 수신 대상 상태 목록을 저장한다 */
export async function setNotificationStatuses(statuses: string[]): Promise<void> {
  await setAppSetting(NOTIFICATION_STATUSES_KEY, JSON.stringify(statuses));
}

/** 알림 목록을 안읽음만 보기로 볼지 조회한다. 미설정 시 전체 보기로 시작한다 */
export async function getNotificationUnreadOnlyEnabled(): Promise<boolean> {
  const value = await getAppSetting(NOTIFICATION_UNREAD_ONLY_KEY);
  return value === "true";
}

/** 알림 목록의 안읽음만 보기 여부를 저장한다. 앱을 껐다 켜도 유지되도록 app_settings에 둔다 */
export async function setNotificationUnreadOnlyEnabled(enabled: boolean): Promise<void> {
  await setAppSetting(NOTIFICATION_UNREAD_ONLY_KEY, String(enabled));
}

const BACKGROUND_SYNC_ENABLED_KEY = "background_sync_enabled";
const BACKGROUND_SYNC_INTERVAL_MS_KEY = "background_sync_interval_ms";
const DEFAULT_BACKGROUND_SYNC_INTERVAL_MS = 10 * 60_000;

let backgroundSyncIntervalChangedCallback: ((intervalMs: number) => void) | null = null;
let backgroundSyncEnabledChangedCallback: ((enabled: boolean) => void) | null = null;

/** 백그라운드 sync 주기 변경 시 호출될 콜백을 등록한다. 순환 의존성 없이 서비스 간 협력을 위해 사용한다 */
export function registerBackgroundSyncIntervalChangedCallback(callback: (intervalMs: number) => void): void {
  backgroundSyncIntervalChangedCallback = callback;
}

/** 백그라운드 sync 활성화 상태 변경 시 호출될 콜백을 등록한다 */
export function registerBackgroundSyncEnabledChangedCallback(callback: (enabled: boolean) => void): void {
  backgroundSyncEnabledChangedCallback = callback;
}

/** 백그라운드 sync 활성화 여부를 조회한다. 미설정 시 기본값(활성화)을 반환한다 */
export async function getBackgroundSyncEnabled(): Promise<boolean> {
  const value = await getAppSetting(BACKGROUND_SYNC_ENABLED_KEY);
  return value !== "false";
}

/** 백그라운드 sync 활성화 여부를 저장하고, 실행 중인 루프에 즉시 반영한다 */
export async function setBackgroundSyncEnabled(enabled: boolean): Promise<void> {
  await setAppSetting(BACKGROUND_SYNC_ENABLED_KEY, String(enabled));
  backgroundSyncEnabledChangedCallback?.(enabled);
}

/** 백그라운드 sync 실행 주기(ms)를 조회한다. 미설정 시 기본값(10분)을 반환한다 */
export async function getBackgroundSyncIntervalMs(): Promise<number> {
  const value = await getAppSetting(BACKGROUND_SYNC_INTERVAL_MS_KEY);
  if (!value) return DEFAULT_BACKGROUND_SYNC_INTERVAL_MS;
  const parsed = parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_BACKGROUND_SYNC_INTERVAL_MS;
}

/** 백그라운드 sync 실행 주기(ms)를 저장하고, 실행 중인 루프에 즉시 반영한다 */
export async function setBackgroundSyncIntervalMs(intervalMs: number): Promise<void> {
  await setAppSetting(BACKGROUND_SYNC_INTERVAL_MS_KEY, String(intervalMs));
  backgroundSyncIntervalChangedCallback?.(intervalMs);
}

const DEFAULT_SESSION_TYPE_KEY = "default_session_type";
const SHORTCUT_BINDINGS_KEY = "shortcut_bindings";
/** 단축키 표가 생기기 전, 빠른 검색 하나만 따로 저장하던 키 */
const LEGACY_TASK_SEARCH_SHORTCUT_KEY = "task_search_shortcut";
const VIM_MODE_ENABLED_KEY = "vim_mode_enabled";

/** 기본 세션 타입을 조회한다. 미설정이거나 모르는 값이면 "tmux"를 반환한다 */
export async function getDefaultSessionType(): Promise<SessionType> {
  const value = await getAppSetting(DEFAULT_SESSION_TYPE_KEY);
  const knownSessionTypes: string[] = Object.values(SessionType);

  return knownSessionTypes.includes(value ?? "") ? value as SessionType : SessionType.TMUX;
}

/** 기본 세션 타입을 저장한다 */
export async function setDefaultSessionType(sessionType: SessionType): Promise<void> {
  await setAppSetting(DEFAULT_SESSION_TYPE_KEY, sessionType);
}

/**
 * 사용자가 재배정한 값까지 반영한 단축키 표를 조회한다.
 *
 * 단축키 표가 없으면 옛 `task_search_shortcut` 값을 `taskSearch` 재배정으로 승계한다. 승계하지 않으면
 * 그 시절에 빠른 검색을 바꿔 둔 사용자의 단축키가 업그레이드 순간 말없이 기본값으로 되돌아간다.
 * 승계 결과를 한 번 써 두면 이 분기는 다시 타지 않으므로 옛 키는 그대로 두고 읽지 않는다.
 */
export async function getShortcutBindings(): Promise<ShortcutBindings> {
  const shortcutPlatform = getShortcutPlatformFromProcessPlatform(process.platform);
  const storedOverrides = await getAppSetting(SHORTCUT_BINDINGS_KEY);
  if (storedOverrides !== null) {
    return resolveShortcutBindings(parseShortcutOverrides(storedOverrides, shortcutPlatform));
  }

  const legacyTaskSearchShortcut = await getAppSetting(LEGACY_TASK_SEARCH_SHORTCUT_KEY);
  if (!legacyTaskSearchShortcut) {
    return resolveShortcutBindings({});
  }

  /** 옛 값은 Mod 없이 `Meta+…`/`Ctrl+…`로 저장돼 있어, 접지 않으면 중복 판정이 같은 조합을 못 알아본다 */
  const migratedTaskSearchShortcut = canonicalizeShortcutForPlatform(
    legacyTaskSearchShortcut,
    shortcutPlatform,
  );
  /** 옛 값이 파싱 불가면 canonical 결과가 빈 문자열이고, 그대로 얹으면 기본값을 지워 빠른 검색이 사라진다 */
  const migratedBindings = resolveShortcutBindings(
    migratedTaskSearchShortcut ? { taskSearch: migratedTaskSearchShortcut } : {},
  );
  await setShortcutBindings(migratedBindings);

  return migratedBindings;
}

/** 단축키 표를 저장한다. 기본값과 같은 항목은 빼고 재배정만 남긴다 */
export async function setShortcutBindings(bindings: ShortcutBindings): Promise<void> {
  const shortcutPlatform = getShortcutPlatformFromProcessPlatform(process.platform);
  await setAppSetting(
    SHORTCUT_BINDINGS_KEY,
    JSON.stringify(collectShortcutOverrides(bindings, shortcutPlatform)),
  );
}

/** 태스크 빠른 검색 단축키를 조회한다. 미설정 시 기본값을 반환한다 */
export async function getTaskSearchShortcut(): Promise<string> {
  return (await getShortcutBindings()).taskSearch;
}

/** Vim-style board navigation 활성화 여부를 조회한다. 미설정 시 활성화 상태를 반환한다 */
export async function getVimModeEnabled(): Promise<boolean> {
  const value = await getAppSetting(VIM_MODE_ENABLED_KEY);
  return value !== "false";
}

/** Vim-style board navigation 활성화 여부를 저장한다 */
export async function setVimModeEnabled(enabled: boolean): Promise<void> {
  await setAppSetting(VIM_MODE_ENABLED_KEY, String(enabled));
}

const BOARD_SORT_PREFERENCE_KEY = "board_sort_preference";

/** 보드 정렬 기준을 조회한다. 앱을 껐다 켜도 유지되도록 app_settings에 둔다 */
export async function getBoardSortPreference(): Promise<BoardSortPreference> {
  return parseBoardSortPreference(await getAppSetting(BOARD_SORT_PREFERENCE_KEY));
}

/** 보드 정렬 기준을 저장한다 */
export async function setBoardSortPreference(preference: BoardSortPreference): Promise<void> {
  await setAppSetting(BOARD_SORT_PREFERENCE_KEY, serializeBoardSortPreference(preference));
}
