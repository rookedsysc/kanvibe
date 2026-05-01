"use client";

import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import { getTaskById } from "@/desktop/renderer/actions/kanban";
import { markAllNotificationsRead, markNotificationRead, listNotifications } from "@/desktop/renderer/actions/notifications";
import { redirect } from "@/desktop/renderer/navigation";
import type { AppNotification } from "@/desktop/shared/notifications";

interface NotificationCenterButtonProps {
  buttonClassName?: string;
  panelClassName?: string;
}

export interface NotificationCenterButtonHandle {
  close: () => void;
  open: () => void;
  toggle: () => void;
}

function shouldIgnoreKeyboardNavigation(eventTarget: EventTarget | null, container: HTMLDivElement | null) {
  if (!(eventTarget instanceof Element)) {
    return false;
  }

  if (eventTarget.closest('[data-shortcut-capture="true"]')) {
    return true;
  }

  if (container?.contains(eventTarget)) {
    return false;
  }

  if (
    eventTarget instanceof HTMLInputElement
    || eventTarget instanceof HTMLTextAreaElement
    || eventTarget instanceof HTMLSelectElement
  ) {
    return true;
  }

  return eventTarget.closest('[contenteditable="true"]') !== null;
}

const NotificationCenterButton = forwardRef<NotificationCenterButtonHandle, NotificationCenterButtonProps>(function NotificationCenterButton(
  { buttonClassName = "", panelClassName = "" },
  ref,
) {
  const t = useTranslations("common");
  const containerRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [isOpen, setIsOpen] = useState(false);
  const [notifications, setNotifications] = useState<AppNotification[]>([]);
  const [highlightedIndex, setHighlightedIndex] = useState(-1);
  const [missingTaskNotification, setMissingTaskNotification] = useState<AppNotification | null>(null);

  useImperativeHandle(ref, () => ({
    close() {
      setIsOpen(false);
    },
    open() {
      setIsOpen(true);
    },
    toggle() {
      setIsOpen((current) => !current);
    },
  }), []);

  useEffect(() => {
    async function load() {
      setNotifications(await listNotifications());
    }

    void load();
    return window.kanvibeDesktop?.onNotificationsChanged?.(() => {
      void load();
    });
  }, []);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (!containerRef.current?.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }

    document.addEventListener("mousedown", handleClickOutside);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, []);

  useEffect(() => {
    if (!isOpen) {
      setHighlightedIndex(-1);
      return;
    }

    setHighlightedIndex(notifications.length > 0 ? 0 : -1);
  }, [isOpen, notifications.length]);

  useEffect(() => {
    if (highlightedIndex < 0) {
      return;
    }

    itemRefs.current[highlightedIndex]?.scrollIntoView?.({
      block: "nearest",
    });
  }, [highlightedIndex]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    function handleWindowKeyDown(event: KeyboardEvent) {
      if (shouldIgnoreKeyboardNavigation(event.target, containerRef.current)) {
        return;
      }

      switch (event.key) {
        case "ArrowDown":
          if (notifications.length === 0) {
            return;
          }
          event.preventDefault();
          setHighlightedIndex((current) => (current < notifications.length - 1 ? current + 1 : 0));
          break;
        case "ArrowUp":
          if (notifications.length === 0) {
            return;
          }
          event.preventDefault();
          setHighlightedIndex((current) => (current > 0 ? current - 1 : notifications.length - 1));
          break;
        case "Enter":
          if (highlightedIndex < 0 || highlightedIndex >= notifications.length) {
            return;
          }
          event.preventDefault();
          void handleNotificationClick(notifications[highlightedIndex]);
          break;
        case "Escape":
          event.preventDefault();
          setIsOpen(false);
          break;
      }
    }

    window.addEventListener("keydown", handleWindowKeyDown);
    return () => {
      window.removeEventListener("keydown", handleWindowKeyDown);
    };
  }, [highlightedIndex, isOpen, notifications]);

  const unreadCount = useMemo(() => notifications.filter((notification) => !notification.isRead).length, [notifications]);

  async function handleNotificationClick(notification: AppNotification) {
    if (!notification.isRead) {
      await markNotificationRead(notification.id);
      setNotifications((current) => current.map((item) => (
        item.id === notification.id ? { ...item, isRead: true } : item
      )));
    }

    setIsOpen(false);

    if (notification.taskId) {
      const task = await getTaskById(notification.taskId);

      if (!task) {
        setMissingTaskNotification(notification);
        return;
      }

      redirect(`/${notification.locale}/task/${notification.taskId}`);
      return;
    }

    redirect(notification.relativePath);
  }

  async function handleMarkAllRead() {
    await markAllNotificationsRead();
    setNotifications((current) => current.map((notification) => ({ ...notification, isRead: true })));
  }

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        onClick={() => setIsOpen((current) => !current)}
        className={`relative rounded-md p-1.5 text-text-muted hover:bg-bg-page hover:text-text-primary transition-colors ${buttonClassName}`.trim()}
        title={t("notifications")}
        aria-label={t("notifications")}
      >
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M15 17h5l-1.4-1.4A2 2 0 0 1 18 14.2V11a6 6 0 1 0-12 0v3.2a2 2 0 0 1-.6 1.4L4 17h5" />
          <path d="M9 17a3 3 0 0 0 6 0" />
        </svg>
        {unreadCount > 0 ? (
          <span className="absolute -right-1 -top-1 min-w-4 h-4 rounded-full bg-brand-primary px-1 text-[10px] font-semibold text-white flex items-center justify-center">
            {unreadCount > 9 ? "9+" : unreadCount}
          </span>
        ) : null}
      </button>

      {isOpen ? (
        <div className={`absolute right-0 z-50 mt-2 w-96 max-w-[calc(100vw-2rem)] overflow-hidden rounded-xl border border-border-default bg-bg-surface shadow-xl ${panelClassName}`.trim()}>
          <div className="flex items-center justify-between border-b border-border-default px-4 py-3">
            <div>
              <h3 className="text-sm font-semibold text-text-primary">{t("notifications")}</h3>
              <p className="text-xs text-text-muted">
                {unreadCount > 0 ? t("unreadCount", { count: unreadCount }) : t("allCaughtUp")}
              </p>
            </div>
            <button type="button" onClick={handleMarkAllRead} className="text-xs text-text-secondary hover:text-text-primary transition-colors">
              {t("markAllRead")}
            </button>
          </div>

          <div className="max-h-[28rem] overflow-y-auto">
            {notifications.length === 0 ? (
              <div className="px-4 py-8 text-center text-sm text-text-muted">{t("noNotifications")}</div>
            ) : notifications.map((notification, index) => (
              <button
                key={notification.id}
                ref={(node) => {
                  itemRefs.current[index] = node;
                }}
                type="button"
                onClick={() => handleNotificationClick(notification)}
                onMouseEnter={() => setHighlightedIndex(index)}
                className={`w-full border-b border-border-subtle px-4 py-3 text-left transition-colors hover:bg-bg-page ${
                  index === highlightedIndex
                    ? "bg-brand-primary/10"
                    : notification.isRead ? "bg-bg-surface" : "bg-brand-primary/5"
                }`}
              >
                <div className="flex items-start gap-3">
                  <span className={`mt-1 h-2.5 w-2.5 rounded-full ${notification.isRead ? "bg-border-default" : "bg-brand-primary"}`} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-start justify-between gap-3">
                      <p className="text-sm font-medium text-text-primary">{notification.title}</p>
                      <span className="shrink-0 text-[11px] text-text-muted">{new Date(notification.createdAt).toLocaleString()}</span>
                    </div>
                    <p className="mt-1 whitespace-pre-wrap text-xs text-text-secondary">{notification.body}</p>
                  </div>
                </div>
              </button>
            ))}
          </div>
        </div>
      ) : null}

      {missingTaskNotification ? (
        <div className="fixed inset-0 z-[500] flex items-center justify-center bg-bg-overlay px-4">
          <div className="w-full max-w-sm rounded-xl border border-border-default bg-bg-surface p-6 shadow-lg">
            <h2 className="text-lg font-semibold text-text-primary">{t("notificationTaskMissingTitle")}</h2>
            <p className="mt-2 whitespace-pre-wrap text-sm text-text-secondary">{t("notificationTaskMissingBody")}</p>
            <div className="mt-5 flex justify-end">
              <button
                type="button"
                onClick={() => setMissingTaskNotification(null)}
                className="rounded-md bg-brand-primary px-4 py-1.5 text-sm text-text-inverse transition-colors hover:bg-brand-hover"
              >
                {t("confirm")}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
});

export default NotificationCenterButton;
