"use client";

import { useEffect, useRef } from "react";
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
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  /** useEffect 내 stale closure 방지용 ref */
  const settingsRef = useRef({ isNotificationEnabled, enabledStatuses });
  settingsRef.current = { isNotificationEnabled, enabledStatuses };

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
            console.debug("[WS] task-status-changed 수신:", data);

            const { isNotificationEnabled: isEnabled, enabledStatuses: statuses } = settingsRef.current;
            if (!isEnabled) return;
            if (!statuses.includes(data.newStatus)) return;

            const { projectName, branchName, taskTitle, description, newStatus } = data;
            notifyTaskStatusChanged({ projectName, branchName, taskTitle, description, newStatus });
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
