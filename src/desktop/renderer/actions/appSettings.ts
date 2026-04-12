import type { SessionType } from "@/entities/KanbanTask";
import { invokeDesktop } from "@/desktop/renderer/ipc";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";

async function invokeAndRefresh<T>(method: string, ...args: unknown[]): Promise<T> {
  const result = await invokeDesktop<T>("appSettings", method, ...args);
  triggerDesktopRefresh("settings");
  return result;
}

export function getAppSetting(key: string): Promise<string | null> {
  return invokeDesktop("appSettings", "getAppSetting", key);
}

export function setAppSetting(key: string, value: string): Promise<void> {
  return invokeAndRefresh("setAppSetting", key, value);
}

export function getSidebarDefaultCollapsed(): Promise<boolean> {
  return invokeDesktop("appSettings", "getSidebarDefaultCollapsed");
}

export function setSidebarDefaultCollapsed(collapsed: boolean): Promise<void> {
  return invokeAndRefresh("setSidebarDefaultCollapsed", collapsed);
}

export function getSidebarHintDismissed(): Promise<boolean> {
  return invokeDesktop("appSettings", "getSidebarHintDismissed");
}

export function dismissSidebarHint(): Promise<void> {
  return invokeAndRefresh("dismissSidebarHint");
}

export function getDoneAlertDismissed(): Promise<boolean> {
  return invokeDesktop("appSettings", "getDoneAlertDismissed");
}

export function dismissDoneAlert(): Promise<void> {
  return invokeAndRefresh("dismissDoneAlert");
}

export function getNotificationSettings(): Promise<{ isEnabled: boolean; enabledStatuses: string[] }> {
  return invokeDesktop("appSettings", "getNotificationSettings");
}

export function setNotificationEnabled(enabled: boolean): Promise<void> {
  return invokeAndRefresh("setNotificationEnabled", enabled);
}

export function setNotificationStatuses(statuses: string[]): Promise<void> {
  return invokeAndRefresh("setNotificationStatuses", statuses);
}

export function getDefaultSessionType(): Promise<SessionType> {
  return invokeDesktop("appSettings", "getDefaultSessionType");
}

export function setDefaultSessionType(sessionType: SessionType): Promise<void> {
  return invokeAndRefresh("setDefaultSessionType", sessionType);
}
