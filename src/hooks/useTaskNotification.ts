"use client";

import { useEffect, useCallback, useRef } from "react";

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

/** hooks 경유 task 상태 변경 시 Browser Notification을 발송하는 훅 */
export function useTaskNotification() {
  const isPermissionGranted = useRef(false);

  useEffect(() => {
    if (typeof window === "undefined" || !("Notification" in window)) return;

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

      const title = `${payload.projectName} — ${payload.branchName}`;
      const bodyParts = [`${payload.taskTitle}: ${payload.newStatus}로 변경`];
      if (payload.description) {
        bodyParts.push(payload.description);
      }

      // Service Worker의 notificationclick 이벤트를 수신하려면
      // ServiceWorkerRegistration.showNotification()을 사용해야 함
      try {
        const registration = await navigator.serviceWorker.getRegistration();
        if (registration) {
          registration.showNotification(title, {
            body: bodyParts.join("\n"),
            icon: "/kanvibe-logo.svg",
            data: {
              taskId: payload.taskId,
              locale: payload.locale,
            },
          });
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

      const title = `${payload.projectName} — ${payload.branchName}`;
      const reasonMessage =
        payload.reason === "project-not-found"
          ? "프로젝트를 찾지 못했습니다."
          : "브랜치에 연결된 작업을 찾지 못했습니다.";

      try {
        const registration = await navigator.serviceWorker.getRegistration();
        if (registration) {
          registration.showNotification(title, {
            body: `${payload.requestedStatus} 상태로 변경하지 못했습니다.\n${reasonMessage}`,
            icon: "/kanvibe-logo.svg",
            data: {
              locale: payload.locale,
            },
          });
        }
      } catch (err) {
        console.error("[Notification] Failed to show missing target notification:", err);
      }
    },
    []
  );

  return { notifyTaskStatusChanged, notifyHookStatusTargetMissing };
}
