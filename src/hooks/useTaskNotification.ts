"use client";

import { useEffect, useCallback, useRef } from "react";

type NotificationLocale = "ko" | "en" | "zh";

const NOTIFICATION_ICON_PATH = "/icons/icon-192x192.png";

const NOTIFICATION_MESSAGES = {
  ko: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: ${newStatus}로 변경`,
    formatMissingStatus: (requestedStatus: string) => `${requestedStatus} 상태로 변경하지 못했습니다.`,
    projectNotFound: "프로젝트를 찾지 못했습니다.",
    taskNotFound: "브랜치에 연결된 작업을 찾지 못했습니다.",
  },
  en: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: changed to ${newStatus}`,
    formatMissingStatus: (requestedStatus: string) => `Failed to change status to ${requestedStatus}.`,
    projectNotFound: "Project was not found.",
    taskNotFound: "No task linked to this branch was found.",
  },
  zh: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: 已变更为${newStatus}`,
    formatMissingStatus: (requestedStatus: string) => `未能变更为 ${requestedStatus} 状态。`,
    projectNotFound: "未找到项目。",
    taskNotFound: "未找到与该分支关联的任务。",
  },
} as const;

function getNotificationLocale(locale: string): NotificationLocale {
  if (locale.startsWith("en")) return "en";
  if (locale.startsWith("zh")) return "zh";
  return "ko";
}

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
  projectName: string;
  branchName: string;
  requestedStatus: string;
  reason: "project-not-found" | "task-not-found";
  locale: string;
}

interface BrowserNotificationData {
  taskId?: string;
  locale: string;
}

function isDesktopNotificationAvailable() {
  return typeof window !== "undefined" && window.kanvibeDesktop?.isDesktop === true;
}

async function showNotificationViaDesktopBridge(title: string, body: string, data: BrowserNotificationData) {
  if (!isDesktopNotificationAvailable()) {
    return false;
  }

  await window.kanvibeDesktop?.showNotification?.({
    title,
    body,
    taskId: data.taskId,
    locale: data.locale,
  });

  return true;
}

async function showNotificationViaServiceWorker(title: string, body: string, data: BrowserNotificationData) {
  if (!("serviceWorker" in navigator)) {
    return false;
  }

  const registration = await navigator.serviceWorker.getRegistration();
  if (!registration) {
    return false;
  }

  await registration.showNotification(title, {
    body,
    icon: NOTIFICATION_ICON_PATH,
    data,
  });

  return true;
}

async function showNotificationViaBrowser(title: string, body: string, data: BrowserNotificationData) {
  const isServiceWorkerNotificationShown = await showNotificationViaServiceWorker(title, body, data);
  if (isServiceWorkerNotificationShown) {
    return true;
  }

  if (typeof Notification === "undefined") {
    return false;
  }

  new Notification(title, {
    body,
    icon: NOTIFICATION_ICON_PATH,
    data,
  });
  return true;
}

/** hooks 경유 task 상태 변경 시 Browser Notification을 발송하는 훅 */
export function useTaskNotification() {
  const isPermissionGranted = useRef(false);

  useEffect(() => {
    if (typeof window === "undefined") return;

    if (isDesktopNotificationAvailable()) {
      isPermissionGranted.current = true;
      return;
    }

    if (!("Notification" in window)) return;

    if (Notification.permission === "granted") {
      isPermissionGranted.current = true;
    } else if (Notification.permission !== "denied") {
      Notification.requestPermission().then((permission) => {
        isPermissionGranted.current = permission === "granted";
      });
    }
  }, []);

  const notifyTaskStatusChanged = useCallback(
    async (payload: TaskStatusNotification) => {
      if (!isPermissionGranted.current) return;

      const messages = NOTIFICATION_MESSAGES[getNotificationLocale(payload.locale)];
      const title = `${payload.projectName} — ${payload.branchName}`;
      const bodyParts = [messages.formatStatusChanged(payload.taskTitle, payload.newStatus)];
      if (payload.description) {
        bodyParts.push(payload.description);
      }

      try {
        const body = bodyParts.join("\n");
        const data = {
          taskId: payload.taskId,
          locale: payload.locale,
        };

        const isDesktopNotificationShown = await showNotificationViaDesktopBridge(title, body, data);
        if (!isDesktopNotificationShown) {
          await showNotificationViaBrowser(title, body, data);
        }
      } catch (err) {
        console.error("[Notification] Failed to show notification:", err);
      }
    },
    []
  );

  const notifyHookStatusTargetMissing = useCallback(
    async (payload: HookStatusTargetMissingNotification) => {
      if (!isPermissionGranted.current) return;

      const messages = NOTIFICATION_MESSAGES[getNotificationLocale(payload.locale)];
      const title = `${payload.projectName} — ${payload.branchName}`;
      const reasonMessage =
        payload.reason === "project-not-found"
          ? messages.projectNotFound
          : messages.taskNotFound;

      try {
        const body = `${messages.formatMissingStatus(payload.requestedStatus)}\n${reasonMessage}`;
        const data = {
          locale: payload.locale,
        };

        const isDesktopNotificationShown = await showNotificationViaDesktopBridge(title, body, data);
        if (!isDesktopNotificationShown) {
          await showNotificationViaBrowser(title, body, data);
        }
      } catch (err) {
        console.error("[Notification] Failed to show missing target notification:", err);
      }
    },
    []
  );

  return { notifyTaskStatusChanged, notifyHookStatusTargetMissing };
}
