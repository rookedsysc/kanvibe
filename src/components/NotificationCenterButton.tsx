"use client";

import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import {
  getNotificationUnreadOnlyEnabled,
  setNotificationUnreadOnlyEnabled,
} from "@/desktop/renderer/actions/appSettings";
import { getTaskById } from "@/desktop/renderer/actions/kanban";
import {
  activateNotification,
  listNotifications,
  markAllNotificationsRead,
  markNotificationRead,
} from "@/desktop/renderer/actions/notifications";
import { useHasBoardShortcutBlocker } from "@/desktop/renderer/components/BoardCommandProvider";
import { redirect } from "@/desktop/renderer/navigation";
import { requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import { navigateToTaskDetail } from "@/desktop/renderer/utils/taskNavigation";
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

function sortNotificationsByNewestFirst(notifications: AppNotification[]) {
  return [...notifications].sort((a, b) => b.createdAt.localeCompare(a.createdAt));
}

interface NotificationFilterTabProps {
  isSelected: boolean;
  label: string;
  count?: number;
  onSelect: () => void;
}

function NotificationFilterTab({ isSelected, label, count, onSelect }: NotificationFilterTabProps) {
  return (
    <button
      type="button"
      aria-pressed={isSelected}
      onClick={onSelect}
      className={`rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${
        isSelected
          ? "bg-brand-primary text-text-inverse"
          : "text-text-secondary hover:text-text-primary"
      }`}
    >
      {label}
      {count ? (
        <span className={`ml-1.5 ${isSelected ? "text-text-inverse/80" : "text-text-muted"}`}>{count}</span>
      ) : null}
    </button>
  );
}

const NotificationCenterButton = forwardRef<NotificationCenterButtonHandle, NotificationCenterButtonProps>(function NotificationCenterButton(
  { buttonClassName = "", panelClassName = "" },
  ref,
) {
  const t = useTranslations("common");
  const hasShortcutBlocker = useHasBoardShortcutBlocker();
  const containerRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const isOpenRef = useRef(false);
  const highlightedIndexRef = useRef(0);
  const [isOpen, setIsOpen] = useState(false);
  const [notifications, setNotifications] = useState<AppNotification[]>([]);
  const [isUnreadOnly, setIsUnreadOnly] = useState(false);
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const [missingTaskNotification, setMissingTaskNotification] = useState<AppNotification | null>(null);

  const setPanelOpen = useCallback((nextIsOpen: boolean) => {
    isOpenRef.current = nextIsOpen;
    setIsOpen(nextIsOpen);
  }, []);

  const closePanel = useCallback(() => {
    if (!isOpenRef.current) {
      return;
    }

    setPanelOpen(false);
    requestActiveTerminalFocusAfterUiSettles();
  }, [setPanelOpen]);

  const openPanel = useCallback(() => {
    highlightedIndexRef.current = 0;
    setHighlightedIndex(0);
    setPanelOpen(true);
  }, [setPanelOpen]);

  const togglePanel = useCallback(() => {
    if (isOpenRef.current) {
      closePanel();
      return;
    }

    openPanel();
  }, [closePanel, openPanel]);

  useImperativeHandle(ref, () => ({
    close() {
      closePanel();
    },
    open() {
      openPanel();
    },
    toggle() {
      togglePanel();
    },
  }), [closePanel, openPanel, togglePanel]);

  useEffect(() => {
    async function load() {
      setNotifications(sortNotificationsByNewestFirst(await listNotifications()));
    }

    void load();
    return window.kanvibeDesktop?.onNotificationsChanged?.(() => {
      void load();
    });
  }, []);

  useEffect(() => {
    async function restoreUnreadOnlyFilter() {
      setIsUnreadOnly(await getNotificationUnreadOnlyEnabled());
    }

    void restoreUnreadOnlyFilter();
  }, []);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (!containerRef.current?.contains(event.target as Node)) {
        closePanel();
      }
    }

    document.addEventListener("mousedown", handleClickOutside);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, [closePanel]);

  const visibleNotifications = useMemo(() => (
    isUnreadOnly ? notifications.filter((notification) => !notification.isRead) : notifications
  ), [isUnreadOnly, notifications]);

  const highlightedNotificationIndex = isOpen && visibleNotifications.length > 0
    ? Math.min(Math.max(highlightedIndex, 0), visibleNotifications.length - 1)
    : -1;

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    panelRef.current?.focus();
  }, [isOpen]);

  useEffect(() => {
    if (highlightedNotificationIndex < 0) {
      return;
    }

    itemRefs.current[highlightedNotificationIndex]?.scrollIntoView?.({
      block: "nearest",
    });
  }, [highlightedNotificationIndex]);

  const unreadCount = useMemo(() => notifications.filter((notification) => !notification.isRead).length, [notifications]);

  const changeUnreadOnlyFilter = useCallback(async (nextIsUnreadOnly: boolean) => {
    highlightedIndexRef.current = 0;
    setHighlightedIndex(0);
    setIsUnreadOnly(nextIsUnreadOnly);
    await setNotificationUnreadOnlyEnabled(nextIsUnreadOnly);
  }, []);

  const markNotificationAsRead = useCallback(async (notification: AppNotification) => {
    if (notification.isRead) {
      return;
    }

    await markNotificationRead(notification.id);
    setNotifications((current) => current.map((item) => (
      item.id === notification.id ? { ...item, isRead: true } : item
    )));
  }, []);

  const handleNotificationClick = useCallback(async (
    notification: AppNotification,
    { openInNewWindow = false }: { openInNewWindow?: boolean } = {},
  ) => {
    await markNotificationAsRead(notification);

    closePanel();

    if (notification.action?.type === "background-sync-review") {
      await activateNotification(notification.id);
      return;
    }

    if (notification.taskId) {
      const task = await getTaskById(notification.taskId);

      if (!task) {
        setMissingTaskNotification(notification);
        return;
      }

      if (openInNewWindow) {
        await navigateToTaskDetail(notification.taskId, {
          currentLocale: notification.locale,
          openInNewWindow: true,
        });
        return;
      }

      await navigateToTaskDetail(notification.taskId, {
        currentLocale: notification.locale,
        navigate: redirect,
      });
      return;
    }

    redirect(notification.relativePath);
  }, [closePanel, markNotificationAsRead]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    function getHighlightedNotification() {
      if (visibleNotifications.length === 0) {
        return null;
      }

      const boundedIndex = Math.min(Math.max(highlightedIndexRef.current, 0), visibleNotifications.length - 1);
      return visibleNotifications[boundedIndex] ?? null;
    }

    function handleWindowKeyDown(event: KeyboardEvent) {
      if (hasShortcutBlocker) {
        return;
      }

      if (shouldIgnoreKeyboardNavigation(event.target, containerRef.current)) {
        return;
      }

      switch (event.key) {
        case "ArrowDown":
          if (visibleNotifications.length === 0) {
            return;
          }
          event.preventDefault();
          highlightedIndexRef.current = highlightedIndexRef.current < visibleNotifications.length - 1
            ? highlightedIndexRef.current + 1
            : 0;
          setHighlightedIndex(highlightedIndexRef.current);
          break;
        case "ArrowUp":
          if (visibleNotifications.length === 0) {
            return;
          }
          event.preventDefault();
          highlightedIndexRef.current = highlightedIndexRef.current > 0
            ? highlightedIndexRef.current - 1
            : visibleNotifications.length - 1;
          setHighlightedIndex(highlightedIndexRef.current);
          break;
        case "Enter":
          {
            const highlightedNotification = getHighlightedNotification();
            if (!highlightedNotification) {
              return;
            }
            event.preventDefault();
            void handleNotificationClick(highlightedNotification, { openInNewWindow: event.shiftKey });
            return;
          }
        case " ":
          {
            const highlightedNotification = getHighlightedNotification();
            if (!highlightedNotification) {
              return;
            }
            event.preventDefault();
            void markNotificationAsRead(highlightedNotification);
            return;
          }
        case "Escape":
          event.preventDefault();
          closePanel();
          break;
      }
    }

    window.addEventListener("keydown", handleWindowKeyDown);
    return () => {
      window.removeEventListener("keydown", handleWindowKeyDown);
    };
  }, [closePanel, handleNotificationClick, hasShortcutBlocker, highlightedNotificationIndex, isOpen, markNotificationAsRead, visibleNotifications]);

  async function handleMarkAllRead() {
    await markAllNotificationsRead();
    setNotifications((current) => current.map((notification) => ({ ...notification, isRead: true })));
  }

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        onClick={togglePanel}
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
        <div
          ref={panelRef}
          role="dialog"
          aria-label={t("notifications")}
          tabIndex={-1}
          data-terminal-focus-blocker="true"
          className={`absolute right-0 z-50 mt-2 w-96 max-w-[calc(100vw-2rem)] overflow-hidden rounded-xl border border-border-default bg-bg-surface shadow-xl ${panelClassName}`.trim()}
        >
          <div className="border-b border-border-default px-4 py-3">
            <div className="flex items-center justify-between">
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

            <div role="group" className="mt-3 flex w-fit gap-0.5 rounded-lg bg-bg-page p-0.5">
              <NotificationFilterTab
                isSelected={!isUnreadOnly}
                label={t("notificationFilterAll")}
                onSelect={() => void changeUnreadOnlyFilter(false)}
              />
              <NotificationFilterTab
                isSelected={isUnreadOnly}
                label={t("notificationFilterUnread")}
                count={unreadCount}
                onSelect={() => void changeUnreadOnlyFilter(true)}
              />
            </div>
          </div>

          <div className="max-h-[28rem] overflow-y-auto">
            {visibleNotifications.length === 0 ? (
              <div className="px-4 py-8 text-center text-sm text-text-muted">
                {isUnreadOnly ? t("noUnreadNotifications") : t("noNotifications")}
              </div>
            ) : visibleNotifications.map((notification, index) => (
              <button
                key={notification.id}
                ref={(node) => {
                  itemRefs.current[index] = node;
                }}
                type="button"
                onClick={(event) => handleNotificationClick(notification, { openInNewWindow: event.shiftKey })}
                onMouseEnter={() => {
                  highlightedIndexRef.current = index;
                  setHighlightedIndex(index);
                }}
                className={`w-full border-b border-border-subtle px-4 py-3 text-left transition-colors hover:bg-bg-page ${
                  index === highlightedNotificationIndex
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
        <div data-terminal-focus-blocker="true" className="fixed inset-0 z-[500] flex items-center justify-center bg-bg-overlay px-4">
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
