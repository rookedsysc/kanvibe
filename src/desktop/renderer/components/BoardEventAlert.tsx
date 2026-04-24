"use client";

import { useEffect, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import type { BoardEventPayload } from "@/lib/boardNotifier";

const AUTO_DISMISS_MS = 5000;

interface HookInstallAlertState {
  taskId: string;
  taskTitle: string;
}

function isTaskHookInstallFailedEvent(
  event: BoardEventPayload,
): event is Extract<BoardEventPayload, { type: "task-hook-install-failed" }> {
  return event.type === "task-hook-install-failed";
}

export default function BoardEventAlert() {
  const t = useTranslations("task");
  const tc = useTranslations("common");
  const [alert, setAlert] = useState<HookInstallAlertState | null>(null);
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
      if (!isTaskHookInstallFailedEvent(event)) {
        return;
      }

      setAlert({
        taskId: event.taskId,
        taskTitle: event.taskTitle,
      });

      if (timeoutRef.current !== null) {
        window.clearTimeout(timeoutRef.current);
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

  return (
    <div className="pointer-events-none fixed inset-x-0 top-4 z-[700] flex justify-center px-4">
      <div
        role="alert"
        className="pointer-events-auto w-full max-w-xl rounded-xl border border-status-error/20 bg-bg-surface/95 px-4 py-3 shadow-lg backdrop-blur"
      >
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <p className="text-sm font-semibold text-status-error">
              {t("createHooksBackgroundFailedTitle")}
            </p>
            <p className="mt-1 text-sm text-text-secondary">
              {t("createHooksBackgroundFailedBody", { taskTitle: alert.taskTitle })}
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
