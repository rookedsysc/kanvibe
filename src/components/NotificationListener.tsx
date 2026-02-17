"use client";

import { useEffect, useRef } from "react";
import { usePathname } from "@/i18n/navigation";
import { useTaskNotification } from "@/hooks/useTaskNotification";

const RECONNECT_DELAY_MS = 3000;

interface NotificationListenerProps {
  isNotificationEnabled: boolean;
  enabledStatuses: string[];
}

/** 모든 페이지에서 WebSocket을 통해 task 상태 변경 알림을 수신하는 컴포넌트 */
export default function NotificationListener({
  isNotificationEnabled,
  enabledStatuses,
}: NotificationListenerProps) {
  const { notifyTaskStatusChanged } = useTaskNotification();
  const pathname = usePathname();
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  /** useEffect 내 stale closure 방지용 ref */
  const settingsRef = useRef({ isNotificationEnabled, enabledStatuses });
  settingsRef.current = { isNotificationEnabled, enabledStatuses };

  // pathname에서 locale 추출: /[locale]/...
  const locale = pathname.split("/")[1] || "ko";

  // Service Worker 등록
  useEffect(() => {
    if ("serviceWorker" in navigator && typeof window !== "undefined") {
      navigator.serviceWorker
        .register("/sw.js", { scope: "/" })
        .then((registration) => {
          console.log("[Notification] Service Worker registered:", registration);

          // Service Worker 상태 모니터링
          if (registration.installing) {
            console.log("[Notification] Service Worker installing...");
            registration.installing.addEventListener("statechange", () => {
              console.log("[Notification] Service Worker state changed:", registration.installing?.state);
            });
          }

          if (registration.active) {
            console.log("[Notification] Service Worker is active");
          } else {
            console.log("[Notification] Waiting for Service Worker to activate...");
          }
        })
        .catch((err) => {
          console.error("[Notification] Service Worker 등록 실패:", err);
        });
    }
  }, []);

  useEffect(() => {
    function connect() {
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const port = parseInt(window.location.port || "4885", 10) + 10000;
      const wsUrl = `${protocol}//${window.location.hostname}:${port}/api/board/events`;

      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          if (data.type === "task-status-changed") {
            const { isNotificationEnabled: isEnabled, enabledStatuses: statuses } = settingsRef.current;
            if (!isEnabled) return;
            if (!statuses.includes(data.newStatus)) return;

            const { projectName, branchName, taskTitle, description, newStatus, taskId } = data;
            notifyTaskStatusChanged({ projectName, branchName, taskTitle, description, newStatus, taskId, locale });
          }
        } catch {
          /* 파싱 실패 무시 */
        }
      };

      ws.onclose = () => {
        wsRef.current = null;
        reconnectTimerRef.current = setTimeout(connect, RECONNECT_DELAY_MS);
      };

      ws.onerror = () => {
        ws.close();
      };
    }

    connect();

    return () => {
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
      }
      wsRef.current?.close();
    };
  }, [notifyTaskStatusChanged]);

  return null;
}
