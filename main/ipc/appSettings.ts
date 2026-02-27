import { ipcMain } from "electron";
import { getAppSettingsRepository } from "../database";
import { SessionType } from "@/entities/KanbanTask";

const SIDEBAR_COLLAPSED_KEY = "sidebar_default_collapsed";
const SIDEBAR_HINT_DISMISSED_KEY = "sidebar_hint_dismissed";
const NOTIFICATION_ENABLED_KEY = "notification_enabled";
const NOTIFICATION_STATUSES_KEY = "notification_statuses";
const DONE_ALERT_DISMISSED_KEY = "done_alert_dismissed";
const DEFAULT_SESSION_TYPE_KEY = "default_session_type";

/** 기본 알림 대상 상태 (사용자가 직접 설정하는 todo/done은 제외) */
const DEFAULT_NOTIFICATION_STATUSES = ["progress", "pending", "review"];

/**
 * 키로 앱 설정값을 조회한다.
 * @param key 설정 키
 * @returns 설정값 문자열, 없으면 null
 */
async function getAppSetting(key: string): Promise<string | null> {
  const repo = getAppSettingsRepository();
  const setting = await repo.findOne({ where: { key } });
  return setting?.value ?? null;
}

/**
 * 앱 설정값을 저장한다. 기존 키가 있으면 업데이트, 없으면 생성한다.
 * @param key 설정 키
 * @param value 설정값
 */
async function setAppSetting(key: string, value: string): Promise<void> {
  const repo = getAppSettingsRepository();
  const existing = await repo.findOne({ where: { key } });

  if (existing) {
    existing.value = value;
    await repo.save(existing);
  } else {
    const setting = repo.create({ key, value });
    await repo.save(setting);
  }
}

/** 앱 설정 관련 IPC 핸들러를 등록한다 */
export function registerAppSettingsHandlers(): void {
  /** 키로 앱 설정값을 조회한다 */
  ipcMain.handle("settings:getAppSetting", async (_event, key: string) => {
    return getAppSetting(key);
  });

  /** 앱 설정값을 저장한다 */
  ipcMain.handle("settings:setAppSetting", async (_event, key: string, value: string) => {
    await setAppSetting(key, value);
  });

  /** 사이드바 기본 접힘 상태를 조회한다 */
  ipcMain.handle("settings:getSidebarDefaultCollapsed", async () => {
    const value = await getAppSetting(SIDEBAR_COLLAPSED_KEY);
    return value === "true";
  });

  /** 사이드바 기본 접힘 상태를 저장한다 */
  ipcMain.handle("settings:setSidebarDefaultCollapsed", async (_event, collapsed: boolean) => {
    await setAppSetting(SIDEBAR_COLLAPSED_KEY, String(collapsed));
  });

  /** 사이드바 힌트 숨김 여부를 조회한다 */
  ipcMain.handle("settings:getSidebarHintDismissed", async () => {
    const value = await getAppSetting(SIDEBAR_HINT_DISMISSED_KEY);
    return value === "true";
  });

  /** 사이드바 힌트를 다시 보지 않기로 설정한다 */
  ipcMain.handle("settings:dismissSidebarHint", async () => {
    await setAppSetting(SIDEBAR_HINT_DISMISSED_KEY, "true");
  });

  /** Done 이동 경고 다시 묻지 않기 여부를 조회한다 */
  ipcMain.handle("settings:getDoneAlertDismissed", async () => {
    const value = await getAppSetting(DONE_ALERT_DISMISSED_KEY);
    return value === "true";
  });

  /** Done 이동 경고를 다시 묻지 않기로 설정한다 */
  ipcMain.handle("settings:dismissDoneAlert", async () => {
    await setAppSetting(DONE_ALERT_DISMISSED_KEY, "true");
  });

  /** 알림 설정을 조회한다. 키가 없으면 기본값(전체 활성화)을 반환한다 */
  ipcMain.handle("settings:getNotificationSettings", async () => {
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
  });

  /** 알림 전역 활성화 상태를 저장한다 */
  ipcMain.handle("settings:setNotificationEnabled", async (_event, enabled: boolean) => {
    await setAppSetting(NOTIFICATION_ENABLED_KEY, String(enabled));
  });

  /** 알림 수신 대상 상태 목록을 저장한다 */
  ipcMain.handle("settings:setNotificationStatuses", async (_event, statuses: string[]) => {
    await setAppSetting(NOTIFICATION_STATUSES_KEY, JSON.stringify(statuses));
  });

  /** 기본 세션 타입을 조회한다. 미설정 시 "tmux"를 반환한다 */
  ipcMain.handle("settings:getDefaultSessionType", async () => {
    const value = await getAppSetting(DEFAULT_SESSION_TYPE_KEY);
    return value === SessionType.ZELLIJ ? SessionType.ZELLIJ : SessionType.TMUX;
  });

  /** 기본 세션 타입을 저장한다 */
  ipcMain.handle("settings:setDefaultSessionType", async (_event, sessionType: SessionType) => {
    await setAppSetting(DEFAULT_SESSION_TYPE_KEY, sessionType);
  });
}
