"use client";

import { useCallback, useEffect, useMemo, useState, useTransition } from "react";
import { useTranslations } from "next-intl";
import { updateTaskStatus } from "@/desktop/renderer/actions/kanban";
import type { AppNotification } from "@/desktop/shared/notifications";
import { TaskStatus } from "@/entities/KanbanTask";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import type {
  BackgroundSyncFailurePayload,
  BackgroundSyncPulledTaskPayload,
  BackgroundSyncRegisteredWorktreePayload,
  BackgroundSyncReviewPayload,
  TaskPrMergedDetectedPayload,
} from "@/lib/boardNotifier";

function buildMergedPrEventKey(taskId: string, prUrl: string, mergedAt: string) {
  return `${taskId}:${prUrl}:${mergedAt}`;
}

function getMergedPrEventKey(payload: TaskPrMergedDetectedPayload) {
  return buildMergedPrEventKey(payload.taskId, payload.prUrl, payload.mergedAt);
}

function getBackgroundSyncReviewPayload(notification: AppNotification | null | undefined): BackgroundSyncReviewPayload | null {
  if (notification?.action?.type !== "background-sync-review") {
    return null;
  }

  return notification.action.payload;
}

export default function BackgroundSyncReviewDialog() {
  const tc = useTranslations("common");
  const tr = useTranslations("common.backgroundSyncReview");
  const tm = useTranslations("common.prMergeAlert");
  const [isPrMergeActionPending, startPrMergeActionTransition] = useTransition();
  const [backgroundSyncReview, setBackgroundSyncReview] = useState<BackgroundSyncReviewPayload | null>(null);
  const [selectedPrMergeEventKeys, setSelectedPrMergeEventKeys] = useState<string[]>([]);

  useEffect(() => {
    let isActive = true;

    const applyNotification = (notification: AppNotification | null | undefined) => {
      if (!isActive) {
        return;
      }

      const payload = getBackgroundSyncReviewPayload(notification);
      if (!payload) {
        return;
      }

      setBackgroundSyncReview(payload);
      setSelectedPrMergeEventKeys(payload.mergedPullRequests.map(getMergedPrEventKey));
    };

    const dispose = window.kanvibeDesktop?.onNotificationActivated?.((notification: AppNotification) => {
      applyNotification(notification);
    });

    return () => {
      isActive = false;
      dispose?.();
    };
  }, []);

  const selectedPrMergeEventKeySet = useMemo(
    () => new Set(selectedPrMergeEventKeys),
    [selectedPrMergeEventKeys],
  );

  const togglePrMergeSelection = useCallback((eventKey: string) => {
    setSelectedPrMergeEventKeys((current) => (
      current.includes(eventKey)
        ? current.filter((key) => key !== eventKey)
        : [...current, eventKey]
    ));
  }, []);

  const handlePrMergeCancel = useCallback(() => {
    setBackgroundSyncReview(null);
    setSelectedPrMergeEventKeys([]);
  }, []);

  useEscapeKey(handlePrMergeCancel, {
    enabled: Boolean(backgroundSyncReview) && !isPrMergeActionPending,
  });

  const handlePrMergeConfirm = useCallback(() => {
    if (!backgroundSyncReview) {
      return;
    }

    const selectedEventKeys = new Set(selectedPrMergeEventKeys);
    const selectedTaskIds = backgroundSyncReview.mergedPullRequests
      .filter((alert) => selectedEventKeys.has(getMergedPrEventKey(alert)))
      .map((alert) => alert.taskId);

    setBackgroundSyncReview(null);
    setSelectedPrMergeEventKeys([]);

    if (selectedTaskIds.length === 0) {
      return;
    }

    startPrMergeActionTransition(async () => {
      for (const taskId of selectedTaskIds) {
        await updateTaskStatus(taskId, TaskStatus.DONE);
      }
    });
  }, [backgroundSyncReview, selectedPrMergeEventKeys, startPrMergeActionTransition]);

  if (!backgroundSyncReview) {
    return null;
  }

  const pulledTasks = backgroundSyncReview.pulledTasks ?? [];
  const backgroundSyncFailures = backgroundSyncReview.failures ?? [];

  return (
    <div className="fixed inset-0 z-[520] flex items-center justify-center bg-bg-overlay">
      <div className="w-full max-w-2xl rounded-xl border border-border-default bg-bg-surface p-6 shadow-lg">
        <h2 className="mb-2 text-lg font-semibold text-text-primary">
          {tr("title")}
        </h2>
        <p className="text-sm text-text-secondary">
          {tr("message")}
        </p>
        <div className="mt-4 max-h-[360px] space-y-5 overflow-y-auto pr-1">
          <section>
            <h3 className="text-sm font-semibold text-text-primary">{tr("mergedSection")}</h3>
            {backgroundSyncReview.mergedPullRequests.length === 0 ? (
              <p className="mt-2 text-sm text-text-muted">{tr("emptyMerged")}</p>
            ) : (
              <div className="mt-3 space-y-3">
                {backgroundSyncReview.mergedPullRequests.map((alert) => {
                  const eventKey = getMergedPrEventKey(alert);
                  return (
                    <label
                      key={eventKey}
                      className="flex cursor-pointer items-start gap-3 rounded-lg border border-border-default bg-bg-page px-4 py-3"
                    >
                      <input
                        type="checkbox"
                        checked={selectedPrMergeEventKeySet.has(eventKey)}
                        onChange={() => togglePrMergeSelection(eventKey)}
                        aria-label={alert.taskTitle}
                        className="mt-0.5 h-4 w-4 rounded border-border-default text-brand-primary focus:ring-brand-primary"
                      />
                      <div className="min-w-0 flex-1">
                        <p className="text-sm font-semibold text-text-primary">
                          {alert.taskTitle}
                        </p>
                        <p className="mt-1 text-xs text-text-muted">
                          {alert.branchName}
                        </p>
                        <a
                          href={alert.prUrl}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="mt-3 inline-flex max-w-full items-center gap-1 rounded bg-tag-pr-bg px-2 py-1 text-xs text-tag-pr-text transition-opacity hover:opacity-80"
                        >
                          <span className="shrink-0">{tm("prLinkLabel")}</span>
                          <span className="truncate">{alert.prUrl}</span>
                        </a>
                      </div>
                    </label>
                  );
                })}
              </div>
            )}
          </section>

          <section>
            <h3 className="text-sm font-semibold text-text-primary">{tr("registeredSection")}</h3>
            {backgroundSyncReview.registeredWorktrees.length === 0 ? (
              <p className="mt-2 text-sm text-text-muted">{tr("emptyRegistered")}</p>
            ) : (
              <div className="mt-3 space-y-3">
                {backgroundSyncReview.registeredWorktrees.map((worktree: BackgroundSyncRegisteredWorktreePayload) => (
                  <div
                    key={`${worktree.taskId}:${worktree.worktreePath}`}
                    className="rounded-lg border border-border-default bg-bg-page px-4 py-3"
                  >
                    <div className="flex items-center justify-between gap-3">
                      <p className="text-sm font-semibold text-text-primary">
                        {worktree.projectName}
                      </p>
                      <span className="rounded-full bg-bg-surface px-2 py-0.5 text-[11px] text-text-muted">
                        {tc(worktree.sshHost ? "remote" : "local")}
                      </span>
                    </div>
                    <p className="mt-1 text-xs text-text-muted">{worktree.branchName}</p>
                    <p className="mt-3 text-xs text-text-secondary">{worktree.worktreePath}</p>
                  </div>
                ))}
              </div>
            )}
          </section>

          {pulledTasks.length > 0 ? (
            <section>
              <h3 className="text-sm font-semibold text-text-primary">{tr("pulledSection")}</h3>
              <div className="mt-3 space-y-3">
                {pulledTasks.map((pulledTask: BackgroundSyncPulledTaskPayload) => (
                  <div
                    key={`${pulledTask.taskId}:${pulledTask.worktreePath}:${pulledTask.status}`}
                    className="rounded-lg border border-border-default bg-bg-page px-4 py-3"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="text-sm font-semibold text-text-primary">
                          {pulledTask.taskTitle}
                        </p>
                        <p className="mt-1 text-xs text-text-muted">
                          {pulledTask.branchName}
                        </p>
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        <span className="rounded-full bg-bg-surface px-2 py-0.5 text-[11px] text-text-muted">
                          {tc(pulledTask.sshHost ? "remote" : "local")}
                        </span>
                        <span
                          className={
                            pulledTask.status === "updated"
                              ? "rounded-full bg-green-50 px-2 py-0.5 text-[11px] text-status-done"
                              : "rounded-full bg-red-50 px-2 py-0.5 text-[11px] text-status-error"
                          }
                        >
                          {tr(pulledTask.status === "updated" ? "pullUpdated" : "pullFailed")}
                        </span>
                      </div>
                    </div>
                    <p className="mt-3 text-xs text-text-secondary">{pulledTask.worktreePath}</p>
                    <p className="mt-2 text-xs text-text-secondary">{pulledTask.summary}</p>
                  </div>
                ))}
              </div>
            </section>
          ) : null}

          {backgroundSyncFailures.length > 0 ? (
            <section>
              <h3 className="text-sm font-semibold text-text-primary">{tr("failureSection")}</h3>
              <div className="mt-3 space-y-3">
                {backgroundSyncFailures.map((failure: BackgroundSyncFailurePayload) => (
                  <div
                    key={`${failure.operation}:${failure.taskId ?? failure.target}:${failure.reason}`}
                    className="rounded-lg border border-status-error/20 bg-bg-page px-4 py-3"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="text-sm font-semibold text-text-primary">
                          {failure.target}
                        </p>
                        {failure.branchName ? (
                          <p className="mt-1 text-xs text-text-muted">
                            {failure.branchName}
                          </p>
                        ) : null}
                      </div>
                      <div className="flex shrink-0 items-center gap-2">
                        {failure.sshHost ? (
                          <span className="rounded-full bg-bg-surface px-2 py-0.5 text-[11px] text-text-muted">
                            {tc("remote")}
                          </span>
                        ) : null}
                        <span className="rounded-full bg-red-50 px-2 py-0.5 text-[11px] text-status-error">
                          {tr(failure.operation === "pull-request-sync" ? "pullRequestFailure" : "worktreeFailure")}
                        </span>
                      </div>
                    </div>
                    <p className="mt-3 text-xs text-status-error">{failure.reason}</p>
                  </div>
                ))}
              </div>
            </section>
          ) : null}
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={handlePrMergeCancel}
            disabled={isPrMergeActionPending}
            className="rounded-md border border-border-default bg-bg-page px-4 py-1.5 text-sm text-text-secondary transition-colors hover:border-brand-primary"
          >
            {tc("cancel")}
          </button>
          <button
            type="button"
            onClick={handlePrMergeConfirm}
            disabled={isPrMergeActionPending}
            className="rounded-md bg-brand-primary px-4 py-1.5 text-sm text-text-inverse transition-colors hover:bg-brand-hover disabled:opacity-50"
          >
            {tc("confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
