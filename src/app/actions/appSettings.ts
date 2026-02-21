"use server";

import { revalidatePath } from "next/cache";
import { getAppSettingsRepository } from "@/lib/database";

const SIDEBAR_COLLAPSED_KEY = "sidebar_default_collapsed";
const SIDEBAR_HINT_DISMISSED_KEY = "sidebar_hint_dismissed";
const NOTIFICATION_ENABLED_KEY = "notification_enabled";
const NOTIFICATION_STATUSES_KEY = "notification_statuses";

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
  revalidatePath("/");
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
  revalidatePath("/");
}

/** 알림 수신 대상 상태 목록을 저장한다 */
export async function setNotificationStatuses(statuses: string[]): Promise<void> {
  await setAppSetting(NOTIFICATION_STATUSES_KEY, JSON.stringify(statuses));
  revalidatePath("/");
}

const DEFAULT_SESSION_TYPE_KEY = "default_session_type";

/** 기본 세션 타입을 조회한다. 미설정 시 "tmux"를 반환한다 */
export async function getDefaultSessionType(): Promise<string> {
  const value = await getAppSetting(DEFAULT_SESSION_TYPE_KEY);
  return value || "tmux";
}

/** 기본 세션 타입을 저장한다 */
export async function setDefaultSessionType(sessionType: string): Promise<void> {
  await setAppSetting(DEFAULT_SESSION_TYPE_KEY, sessionType);
  revalidatePath("/");
}
