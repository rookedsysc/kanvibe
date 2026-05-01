"use client";

import { useEffect, useCallback, useRef } from "react";
import type { DesktopNotificationPayload } from "@/desktop/shared/notifications";
import {
  buildBackgroundSyncReviewNotification,
  buildHookStatusTargetMissingNotification,
  buildTaskStatusNotification,
  type BackgroundSyncReviewNotification,
  type HookStatusTargetMissingNotification,
  type TaskStatusNotification,
} from "@/desktop/shared/taskNotifications";

interface BrowserNotificationData {
  taskId?: string;
  locale: string;
}

interface DesktopBridgeNotificationData extends BrowserNotificationData {
  relativePath?: string;
  dedupeKey?: string;
  action?: DesktopNotificationPayload["action"];
}

function isDesktopNotificationAvailable() {
  return typeof window !== "undefined" && window.kanvibeDesktop?.isDesktop === true;
}

async function showNotificationViaDesktopBridge(title: string, body: string, data: DesktopBridgeNotificationData) {
  if (!isDesktopNotificationAvailable()) {
    return false;
  }

  const payload: DesktopNotificationPayload = {
    title,
    body,
    taskId: data.taskId,
    locale: data.locale,
    relativePath: data.relativePath,
    dedupeKey: data.dedupeKey,
    action: data.action,
  };

  return (await window.kanvibeDesktop?.showNotification?.(payload)) === true;
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
    data,
  });

  return true;
}

async function showNotificationViaBrowser(title: string, body: string, data: BrowserNotificationData) {
  const isServiceWorkerNotificationShown = await showNotificationViaServiceWorker(title, body, data);
  if (isServiceWorkerNotificationShown) {
    return true;
  }

  if (typeof Notification !== "function") {
    return false;
  }

  new Notification(title, {
    body,
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

      try {
        const notification = buildTaskStatusNotification(payload);
        const data = {
          taskId: payload.taskId,
          locale: payload.locale,
        };

        const isDesktopNotificationShown = await showNotificationViaDesktopBridge(
          notification.title,
          notification.body,
          notification.desktopPayload,
        );
        if (!isDesktopNotificationShown) {
          await showNotificationViaBrowser(notification.title, notification.body, data);
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

      try {
        const notification = buildHookStatusTargetMissingNotification(payload);
        const data = {
          locale: payload.locale,
        };

        const isDesktopNotificationShown = await showNotificationViaDesktopBridge(
          notification.title,
          notification.body,
          notification.desktopPayload,
        );
        if (!isDesktopNotificationShown) {
          await showNotificationViaBrowser(notification.title, notification.body, data);
        }
      } catch (err) {
        console.error("[Notification] Failed to show missing target notification:", err);
      }
    },
    []
  );

  const notifyBackgroundSyncReview = useCallback(
    async (payload: BackgroundSyncReviewNotification) => {
      if (!isPermissionGranted.current) return;

      try {
        const notification = buildBackgroundSyncReviewNotification(payload);
        const data = {
          locale: payload.locale,
        };

        const isDesktopNotificationShown = await showNotificationViaDesktopBridge(
          notification.title,
          notification.body,
          notification.desktopPayload,
        );
        if (!isDesktopNotificationShown) {
          await showNotificationViaBrowser(notification.title, notification.body, data);
        }
      } catch (err) {
        console.error("[Notification] Failed to show background sync review notification:", err);
      }
    },
    []
  );

  return { notifyTaskStatusChanged, notifyHookStatusTargetMissing, notifyBackgroundSyncReview };
}
