"use client";

import { useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import type { BoardEventPayload } from "@/desktop/shared/contracts/lib-boardNotifier";

const AUTO_DISMISS_MS = 5000;

type AlertableBoardEvent = Extract<BoardEventPayload, { type: "task-hook-install-failed" | "task-peer-deletion-failed" }>;

function isAlertableBoardEvent(event: BoardEventPayload): event is AlertableBoardEvent {
  return event.type === "task-hook-install-failed" || event.type === "task-peer-deletion-failed";
}

export default function BoardEventAlert() {
  const t = useTranslations("task");
  const tc = useTranslations("common");
  const [alert, setAlert] = useState<AlertableBoardEvent | null>(null);
  const timeoutRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (timeoutRef.current !== null) {
        window.clearTimeout(timeoutRef.current);
      }
    };
  }, []);

  useEffect(() => {
    return window.kanvibeDesktop!.onBoardEvent((event: BoardEventPayload) => {
      if (!isAlertableBoardEvent(event)) {
        return;
      }

      setAlert(event);

      if (timeoutRef.current !== null) {
        window.clearTimeout(timeoutRef.current);
        timeoutRef.current = null;
      }

      // 다른 데스크탑에 남은 task는 사용자가 직접 지워야 하므로, 읽고 닫을 때까지 남긴다
      if (event.type === "task-peer-deletion-failed") {
        return;
      }

      timeoutRef.current = window.setTimeout(() => {
        timeoutRef.current = null;
        setAlert(null);
      }, AUTO_DISMISS_MS);
    });
  }, []);

  function dismissAlert() {
    if (timeoutRef.current !== null) {
      window.clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }

    setAlert(null);
  }

  if (!alert) {
    return null;
  }

  const alertText = alert.type === "task-hook-install-failed"
    ? {
        title: t("createHooksBackgroundFailedTitle"),
        body: t("createHooksBackgroundFailedBody", { taskTitle: alert.taskTitle }),
      }
    : {
        title: t("peerDeletionFailedTitle"),
        body: t("peerDeletionFailedBody", { taskTitle: alert.taskTitle, peerUrls: alert.failedPeerUrls.join(", ") }),
      };

  return (
    <div className="pointer-events-none fixed inset-x-0 top-4 z-[700] flex justify-center px-4">
      <div
        role="alert"
        className="pointer-events-auto w-full max-w-xl rounded-xl border border-status-error/20 bg-bg-surface/95 px-4 py-3 shadow-lg backdrop-blur"
      >
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <p className="text-sm font-semibold text-status-error">
              {alertText.title}
            </p>
            <p className="mt-1 text-sm text-text-secondary">
              {alertText.body}
            </p>
          </div>

          <button
            type="button"
            onClick={dismissAlert}
            aria-label={tc("close")}
            className="text-base leading-none text-text-muted transition-colors hover:text-text-primary"
          >
            ×
          </button>
        </div>
      </div>
    </div>
  );
}
