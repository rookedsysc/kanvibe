import { Notification } from "electron";
import type { AppNotification, DesktopNotificationPayload } from "@/desktop/shared/notifications";
import type { BoardEventPayload } from "@/lib/boardNotifier";
import { createNotification, markNotificationRead } from "@/desktop/main/notificationStore";
import { getNotificationSettings } from "@/desktop/main/services/appSettingsService";
import {
  buildBackgroundSyncReviewNotification,
  buildHookStatusTargetMissingNotification,
  buildHookTunnelNotification,
  buildTaskStatusNotification,
} from "@/desktop/shared/taskNotifications";

const activeDesktopNotifications = new Set<InstanceType<typeof Notification>>();

interface DesktopNotificationOptions {
  iconPath?: string;
  onNotificationsChanged?: () => void;
  onNotificationClick?: (notification: AppNotification) => Promise<void> | void;
}

export async function deliverDesktopNotification(
  payload: DesktopNotificationPayload,
  options: DesktopNotificationOptions,
): Promise<boolean> {
  const { created, notification: appNotification } = await createNotification(payload);

  if (created) {
    options.onNotificationsChanged?.();
  }

  if (!created) {
    return true;
  }

  if (!Notification.isSupported()) {
    return false;
  }

  const notification = new Notification({
    title: appNotification.title,
    body: appNotification.body,
    silent: false,
    ...(options.iconPath ? { icon: options.iconPath } : {}),
  });

  activeDesktopNotifications.add(notification);

  const releaseNotification = () => {
    activeDesktopNotifications.delete(notification);
  };

  notification.on("click", () => {
    void markNotificationRead(appNotification.id).then(() => {
      options.onNotificationsChanged?.();
    });
    void Promise.resolve(options.onNotificationClick?.(appNotification))
      .catch(() => {
        /* 클릭 후 이동 실패는 무시한다 */
      })
      .finally(releaseNotification);
  });

  notification.on("close", releaseNotification);
  notification.show();
  return true;
}

export async function deliverBoardEventNotification(
  payload: BoardEventPayload,
  locale: string,
  options: DesktopNotificationOptions,
): Promise<boolean> {
  if (payload.type === "board-updated" || payload.type === "task-session-changed" || payload.type === "background-sync-status-changed") {
    return false;
  }

  if (payload.type === "background-sync-review-needed") {
    const { created } = await createNotification(buildBackgroundSyncReviewNotification({ ...payload, locale }).desktopPayload);
    if (created) options.onNotificationsChanged?.();
    return true;
  }

  const settings = await getNotificationSettings();
  if (payload.type === "hook-tunnel-failed" || payload.type === "hook-tunnel-recovered") {
    const notification = buildHookTunnelNotification({ ...payload, locale });
    if (payload.type === "hook-tunnel-failed" && settings.isEnabled) {
      return deliverDesktopNotification(notification, options);
    }

    const { created } = await createNotification(notification);
    if (created) options.onNotificationsChanged?.();
    return true;
  }

  if (!settings.isEnabled) {
    return false;
  }

  if (payload.type === "task-status-changed") {
    if (!settings.enabledStatuses.includes(payload.newStatus)) {
      return false;
    }

    return deliverDesktopNotification(buildTaskStatusNotification({
      ...payload,
      locale,
    }).desktopPayload, options);
  }

  if (payload.type === "task-hook-install-failed" || payload.type === "task-peer-deletion-failed" || payload.type === "herdr-machine-registration-failed" || payload.type === "herdr-server-handoff-required") {
    return false;
  }

  if (
    payload.type === "task-pr-merged-detected"
    || payload.type === "task-pr-merged-detected-batch"
  ) {
    return false;
  }

  if (!settings.enabledStatuses.includes(payload.requestedStatus)) {
    return false;
  }

  return deliverDesktopNotification(buildHookStatusTargetMissingNotification({
    ...payload,
    locale,
  }).desktopPayload, options);
}
