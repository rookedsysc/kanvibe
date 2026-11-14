"use client";

import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import {
  getNotificationUnreadOnlyEnabled,
  setNotificationUnreadOnlyEnabled,
} from "@/desktop/renderer/actions/appSettings";
import { getTaskById } from "@/desktop/renderer/actions/kanban";
import {
  listNotifications,
  markAllNotificationsRead,
  markNotificationRead,
} from "@/desktop/renderer/actions/notifications";
import { useHasBoardShortcutBlocker } from "@/desktop/renderer/components/BoardCommandProvider";
import { showActionStatus } from "@/desktop/renderer/components/ActionStatusToast";
import { useTrackedAction } from "@/desktop/renderer/hooks/useTrackedAction";
import InlineLoadError from "@/components/InlineLoadError";
import { redirect } from "@/desktop/renderer/navigation";
import { requestActiveTerminalFocusAfterUiSettles } from "@/desktop/renderer/utils/terminalFocus";
import { navigateToTaskDetail } from "@/desktop/renderer/utils/taskNavigation";
import type { AppNotification } from "@/desktop/shared/notifications";
import BackgroundSyncReviewPanel from "@/desktop/renderer/components/BackgroundSyncReviewPanel";
import BackgroundSyncStatus from "@/desktop/renderer/components/BackgroundSyncStatus";
import { useBackgroundTaskSync } from "@/desktop/renderer/hooks/useBackgroundTaskSync";

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
  const sync = useBackgroundTaskSync();
  const [reviewNotification, setReviewNotification] = useState<AppNotification | null>(null);
  const hasShortcutBlocker = useHasBoardShortcutBlocker();
  const containerRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const isOpenRef = useRef(false);
  const highlightedIndexRef = useRef(0);
  const [isOpen, setIsOpen] = useState(false);
  const [notifications, setNotifications] = useState<AppNotification[]>([]);
  const [hasLoadedNotifications, setHasLoadedNotifications] = useState(false);
  const [notificationLoadFailure, setNotificationLoadFailure] = useState<string | null>(null);
  /** 읽음 저장과 이동을 기다리는 알림. 같은 알림을 거듭 열지 않게 하고 그 행에 진행을 보인다 */
  const [openingNotificationId, setOpeningNotificationId] = useState<string | null>(null);
  const openingNotificationIdRef = useRef<string | null>(null);
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

  useEffect(() => window.kanvibeDesktop?.onNotificationActivated?.((notification: AppNotification) => {
    if (notification.action?.type !== "background-sync-review") return;
    setReviewNotification(notification);
    openPanel();
  }), [openPanel]);

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

  /** 목록을 읽지 못하면 알림이 없는 것처럼 보이지 않게 실패 사유를 따로 남긴다 */
  const loadNotifications = useCallback(async () => {
    try {
      setNotifications(sortNotificationsByNewestFirst(await listNotifications()));
      setNotificationLoadFailure(null);
    } catch (error) {
      setNotificationLoadFailure(error instanceof Error ? error.message : String(error));
    } finally {
      setHasLoadedNotifications(true);
    }
  }, []);

  useEffect(() => {
    void loadNotifications();
    return window.kanvibeDesktop?.onNotificationsChanged?.(() => {
      void loadNotifications();
    });
  }, [loadNotifications]);

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

  const openNotification = useCallback(async (
    notification: AppNotification,
    { openInNewWindow = false }: { openInNewWindow?: boolean } = {},
  ) => {
    await markNotificationAsRead(notification);

    if (notification.action?.type === "background-sync-review") {
      setReviewNotification(notification);
      return;
    }

    closePanel();

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

  /** 패널이 닫힌 뒤에도 실패할 수 있으므로 열기 실패는 토스트로 알린다 */
  const handleNotificationClick = useCallback(async (
    notification: AppNotification,
    options: { openInNewWindow?: boolean } = {},
  ) => {
    if (openingNotificationIdRef.current) {
      return;
    }

    openingNotificationIdRef.current = notification.id;
    setOpeningNotificationId(notification.id);
    try {
      await openNotification(notification, options);
    } catch (error) {
      console.error("Failed to open notification", error);
      showActionStatus({
        kind: "failure",
        message: t("notificationOpenFailed", { reason: error instanceof Error ? error.message : String(error) }),
      });
    } finally {
      openingNotificationIdRef.current = null;
      setOpeningNotificationId(null);
    }
  }, [openNotification, t]);

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
      if (reviewNotification) return;
      if (event.target instanceof Element && event.target.closest("section")) return;
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
            void markNotificationAsRead(highlightedNotification).catch((error) => {
              showActionStatus({
                kind: "failure",
                message: t("notificationMarkReadFailed", { reason: error instanceof Error ? error.message : String(error) }),
              });
            });
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
  }, [closePanel, handleNotificationClick, hasShortcutBlocker, highlightedNotificationIndex, isOpen, markNotificationAsRead, t, visibleNotifications, reviewNotification]);

  const markAllReadAction = useTrackedAction(async () => {
    await markAllNotificationsRead();
    setNotifications((current) => current.map((notification) => ({ ...notification, isRead: true })));
  });

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        onClick={togglePanel}
        className={`relative rounded-md p-1.5 text-text-muted hover:bg-bg-page hover:text-text-primary transition-colors ${buttonClassName}`.trim()}
        title={t("notifications")}
        aria-label={t("notifications")}
        aria-expanded={isOpen}
      >
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M15 17h5l-1.4-1.4A2 2 0 0 1 18 14.2V11a6 6 0 1 0-12 0v3.2a2 2 0 0 1-.6 1.4L4 17h5" />
          <path d="M9 17a3 3 0 0 0 6 0" />
        </svg>
        {sync.isStarting || sync.status?.isRunning ? (
          <span aria-label={t("backgroundSyncStatus.running")} role="status" className="absolute -right-1 -top-1 h-3 w-3 rounded-full border-2 border-brand-primary border-t-transparent motion-safe:animate-spin" />
        ) : sync.error || sync.status?.phase === "failed" ? (
          <span aria-label={t("backgroundSyncStatus.failed")} role="status" className="absolute -right-1 -top-1 flex h-4 w-4 items-center justify-center rounded-full bg-status-error text-[10px] font-semibold text-white">!</span>
        ) : unreadCount > 0 ? (
          <span className="absolute -right-1 -top-1 min-w-4 h-4 rounded-full bg-brand-primary px-1 text-[10px] font-semibold text-white flex items-center justify-center">
            {unreadCount > 9 ? "9+" : unreadCount}
          </span>
        ) : null}
      </button>

      {isOpen || reviewNotification ? (
        <div
          hidden={!isOpen}
          ref={panelRef}
          role="dialog"
          aria-label={t("notifications")}
          tabIndex={-1}
          data-terminal-focus-blocker={isOpen ? "true" : undefined}
          className={`absolute right-0 z-50 mt-2 w-96 max-w-[calc(100vw-5rem)] overflow-hidden rounded-xl border border-border-default bg-bg-surface shadow-xl ${panelClassName}`.trim()}
        >
          <div className="max-h-[calc(100dvh-6rem)] overflow-y-auto">
          <BackgroundSyncStatus sync={sync} />
          {reviewNotification?.action?.type === "background-sync-review" ? (
            <BackgroundSyncReviewPanel
              key={reviewNotification.id}
              review={reviewNotification.action.payload}
              onClose={() => setReviewNotification(null)}
              isActive={isOpen}
            />
          ) : <>
          <div className="border-b border-border-default px-4 py-3">
            <div className="flex items-center justify-between">
              <div>
                <h3 className="text-sm font-semibold text-text-primary">{t("notifications")}</h3>
                <p className="text-xs text-text-muted">
                  {unreadCount > 0 ? t("unreadCount", { count: unreadCount }) : t("allCaughtUp")}
                </p>
              </div>
              <button
                type="button"
                onClick={() => void markAllReadAction.run()}
                disabled={markAllReadAction.isPending}
                aria-busy={markAllReadAction.isPending || undefined}
                className="text-xs text-text-secondary hover:text-text-primary transition-colors disabled:cursor-progress disabled:opacity-60"
              >
                {markAllReadAction.isPending ? t("markingAllRead") : t("markAllRead")}
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

          {markAllReadAction.error ? (
            <InlineLoadError
              className="mx-4 mt-3"
              message={t("markAllReadFailed", { reason: markAllReadAction.error })}
              onRetry={() => void markAllReadAction.retry()}
            />
          ) : null}
          {notificationLoadFailure ? (
            <InlineLoadError
              className="mx-4 mt-3"
              message={t("loadFailed", { reason: notificationLoadFailure })}
              onRetry={() => void loadNotifications()}
            />
          ) : null}

          <div className="max-h-[28rem] overflow-y-auto">
            {visibleNotifications.length === 0 ? (
              notificationLoadFailure ? null : (
                <div role={hasLoadedNotifications ? undefined : "status"} className="px-4 py-8 text-center text-sm text-text-muted">
                  {!hasLoadedNotifications
                    ? t("loading")
                    : isUnreadOnly ? t("noUnreadNotifications") : t("noNotifications")}
                </div>
              )
            ) : visibleNotifications.map((notification, index) => (
              <button
                key={notification.id}
                ref={(node) => {
                  itemRefs.current[index] = node;
                }}
                type="button"
                aria-busy={openingNotificationId === notification.id || undefined}
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
                  {openingNotificationId === notification.id ? (
                    <span aria-hidden="true" className="mt-0.5 h-3 w-3 shrink-0 rounded-full border-2 border-current border-t-transparent text-text-muted motion-safe:animate-spin" />
                  ) : (
                    <span className={`mt-1 h-2.5 w-2.5 rounded-full ${notification.isRead ? "bg-border-default" : "bg-brand-primary"}`} />
                  )}
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
          </>}
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
