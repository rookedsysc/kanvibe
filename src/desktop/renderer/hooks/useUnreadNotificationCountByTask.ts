import { useEffect, useState } from "react";
import { listNotifications } from "@/desktop/renderer/actions/notifications";
import type { AppNotification } from "@/desktop/shared/notifications";

export type UnreadNotificationCountByTask = Record<string, number>;

function countUnreadByTask(notifications: AppNotification[]): UnreadNotificationCountByTask {
  const countByTask: UnreadNotificationCountByTask = {};

  for (const notification of notifications) {
    if (notification.isRead || !notification.taskId) {
      continue;
    }

    countByTask[notification.taskId] = (countByTask[notification.taskId] ?? 0) + 1;
  }

  return countByTask;
}

function isSameUnreadCount(
  previous: UnreadNotificationCountByTask,
  next: UnreadNotificationCountByTask,
): boolean {
  const previousTaskIds = Object.keys(previous);
  if (previousTaskIds.length !== Object.keys(next).length) {
    return false;
  }

  return previousTaskIds.every((taskId) => previous[taskId] === next[taskId]);
}

/** 보드에서 task별 미읽음 알림 개수를 구독한다. 카드마다 구독하지 않도록 보드 단위로 한 번만 사용한다 */
export function useUnreadNotificationCountByTask(): UnreadNotificationCountByTask {
  const [countByTask, setCountByTask] = useState<UnreadNotificationCountByTask>({});

  useEffect(() => {
    let cancelled = false;

    async function load() {
      const notifications = await listNotifications();
      if (cancelled) {
        return;
      }

      /** 알림이 와도 집계가 그대로면 같은 객체를 유지해 보드 리렌더를 막는다 */
      const nextCountByTask = countUnreadByTask(notifications);
      setCountByTask((previous) => isSameUnreadCount(previous, nextCountByTask) ? previous : nextCountByTask);
    }

    void load();
    const unsubscribe = window.kanvibeDesktop?.onNotificationsChanged?.(() => {
      void load();
    });

    return () => {
      cancelled = true;
      unsubscribe?.();
    };
  }, []);

  return countByTask;
}
