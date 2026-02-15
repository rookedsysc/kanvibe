"use server";

import { revalidatePath } from "next/cache";
import { getAppSettingsRepository } from "@/lib/database";

const SIDEBAR_COLLAPSED_KEY = "sidebar_default_collapsed";
const SIDEBAR_HINT_DISMISSED_KEY = "sidebar_hint_dismissed";

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
