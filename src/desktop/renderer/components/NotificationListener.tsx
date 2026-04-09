import { useEffect, useState } from "react";
import { useLocale } from "next-intl";
import { useTaskNotification } from "@/hooks/useTaskNotification";
import { getNotificationSettings } from "@/desktop/renderer/actions/appSettings";
import { useRefreshSignal } from "@/desktop/renderer/utils/refresh";

export default function NotificationListener() {
  const { notifyTaskStatusChanged, notifyHookStatusTargetMissing } = useTaskNotification();
  const locale = useLocale();
  const refreshSignal = useRefreshSignal();
  const [settings, setSettings] = useState({
    isEnabled: true,
    enabledStatuses: ["progress", "pending", "review"],
  });

  useEffect(() => {
    getNotificationSettings()
      .then((nextSettings) => setSettings(nextSettings))
      .catch(() => {
        /* 설정 로드 실패 시 기본값 유지 */
      });
  }, [refreshSignal]);

  useEffect(() => {
    return window.kanvibeDesktop!.onBoardEvent((event: any) => {
      if (!settings.isEnabled) {
        return;
      }

      if (event.type === "task-status-changed") {
        if (!settings.enabledStatuses.includes(event.newStatus)) {
          return;
        }

        notifyTaskStatusChanged({ ...event, locale });
      }

      if (event.type === "hook-status-target-missing") {
        if (!settings.enabledStatuses.includes(event.requestedStatus)) {
          return;
        }

        notifyHookStatusTargetMissing({ ...event, locale });
      }
    });
  }, [locale, notifyHookStatusTargetMissing, notifyTaskStatusChanged, settings]);

  return null;
}
