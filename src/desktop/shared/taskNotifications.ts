import type { DesktopNotificationPayload } from "@/desktop/shared/notifications";

export type NotificationLocale = "ko" | "en" | "zh";

const NOTIFICATION_MESSAGES = {
  ko: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: ${newStatus}로 변경`,
    formatMissingStatus: (requestedStatus: string) => `${requestedStatus} 상태로 변경하지 못했습니다.`,
    taskNotFound: "연결된 작업을 찾지 못했습니다.",
  },
  en: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: changed to ${newStatus}`,
    formatMissingStatus: (requestedStatus: string) => `Failed to change status to ${requestedStatus}.`,
    taskNotFound: "No matching task was found.",
  },
  zh: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: 已变更为${newStatus}`,
    formatMissingStatus: (requestedStatus: string) => `未能变更为 ${requestedStatus} 状态。`,
    taskNotFound: "未找到匹配的任务。",
  },
} as const;

export interface TaskStatusNotification {
  projectName: string;
  branchName: string;
  taskTitle: string;
  description: string | null;
  newStatus: string;
  taskId: string;
  locale: string;
}

export interface HookStatusTargetMissingNotification {
  taskId: string;
  requestedStatus: string;
  reason: "task-not-found";
  locale: string;
}

interface PreparedNotification {
  title: string;
  body: string;
  desktopPayload: DesktopNotificationPayload;
}

export function getNotificationLocale(locale: string): NotificationLocale {
  if (locale.startsWith("en")) return "en";
  if (locale.startsWith("zh")) return "zh";
  return "ko";
}

export function buildTaskStatusNotification(payload: TaskStatusNotification): PreparedNotification {
  const messages = NOTIFICATION_MESSAGES[getNotificationLocale(payload.locale)];
  const title = `${payload.projectName} — ${payload.branchName}`;
  const bodyParts = [messages.formatStatusChanged(payload.taskTitle, payload.newStatus)];
  if (payload.description) {
    bodyParts.push(payload.description);
  }

  return {
    title,
    body: bodyParts.join("\n"),
    desktopPayload: {
      title,
      body: bodyParts.join("\n"),
      taskId: payload.taskId,
      locale: payload.locale,
      relativePath: `/${payload.locale}/task/${payload.taskId}`,
      dedupeKey: `task-status:${payload.taskId}:${payload.newStatus}`,
    },
  };
}

export function buildHookStatusTargetMissingNotification(payload: HookStatusTargetMissingNotification): PreparedNotification {
  const messages = NOTIFICATION_MESSAGES[getNotificationLocale(payload.locale)];
  const title = `Hook target missing — ${payload.taskId}`;
  const body = `${messages.formatMissingStatus(payload.requestedStatus)}\n${messages.taskNotFound}`;

  return {
    title,
    body,
    desktopPayload: {
      title,
      body,
      locale: payload.locale,
      relativePath: `/${payload.locale}`,
      dedupeKey: `hook-missing:${payload.taskId}:${payload.requestedStatus}:${payload.reason}`,
    },
  };
}
