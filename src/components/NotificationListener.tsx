"use client";

import { useEffect, useRef } from "react";
import { useLocale } from "next-intl";
import { useTaskNotification } from "@/hooks/useTaskNotification";
import { ipcSettings, ipcApp } from "@/lib/ipc";

const RECONNECT_DELAY_MS = 3000;

/** 모든 페이지에서 WebSocket을 통해 task 상태 변경 알림을 수신하는 컴포넌트 */
export default function NotificationListener() {
  const { notifyTaskStatusChanged } = useTaskNotification();
  const locale = useLocale();
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  /** useEffect 내 stale closure 방지용 ref */
  const settingsRef = useRef<{ isEnabled: boolean; enabledStatuses: string[] }>({
    isEnabled: false,
    enabledStatuses: [],
  });

  /** 마운트 시 IPC를 통해 알림 설정을 로드하여 ref에 저장 */
  useEffect(() => {
    ipcSettings.getNotificationSettings().then((settings) => {
      settingsRef.current = {
        isEnabled: settings.isEnabled,
        enabledStatuses: settings.enabledStatuses,
      };
    });
  }, []);

  useEffect(() => {
    let cancelled = false;

    async function connect() {
      const port = await ipcApp.getWsPort();
      if (cancelled) return;

      const wsUrl = `ws://127.0.0.1:${port}/api/board/events`;
      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          if (data.type === "task-status-changed") {
            const { isEnabled, enabledStatuses } = settingsRef.current;
            if (!isEnabled) return;
            if (!enabledStatuses.includes(data.newStatus)) return;

            const { projectName, branchName, taskTitle, description, newStatus, taskId } = data;
            notifyTaskStatusChanged({ projectName, branchName, taskTitle, description, newStatus, taskId, locale });
          }
        } catch {
          /* 파싱 실패 무시 */
        }
      };

      ws.onclose = () => {
        wsRef.current = null;
        if (!cancelled) {
          reconnectTimerRef.current = setTimeout(connect, RECONNECT_DELAY_MS);
        }
      };

      ws.onerror = () => {
        ws.close();
      };
    }

    connect();

    return () => {
      cancelled = true;
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
      }
      wsRef.current?.close();
    };
  }, [notifyTaskStatusChanged, locale]);

  return null;
}
