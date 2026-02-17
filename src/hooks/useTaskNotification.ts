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
    (payload: TaskStatusNotification) => {
      if (!isPermissionGranted.current) return;

      const title = `${payload.projectName} — ${payload.branchName}`;
      const bodyParts = [`${payload.taskTitle}: ${payload.newStatus}로 변경`];
      if (payload.description) {
        bodyParts.push(payload.description);
      }

      new Notification(title, {
        body: bodyParts.join("\n"),
        icon: "/kanvibe-logo.svg",
        data: {
          taskId: payload.taskId,
          locale: payload.locale,
        },
      });
    },
    []
  );

  return { notifyTaskStatusChanged };
}
