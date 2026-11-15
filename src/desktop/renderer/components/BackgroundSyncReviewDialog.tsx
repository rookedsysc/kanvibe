"use client";

import { useEffect } from "react";
import { useTranslations } from "next-intl";
import { useEscapeKey } from "@/hooks/useEscapeKey";
import { useBackgroundSyncReview, openBackgroundSyncReview, closeBackgroundSyncReview, deleteBackgroundSyncReviewTasks, toggleBackgroundSyncReviewTask } from "@/desktop/renderer/hooks/useBackgroundSyncReview";
import type { AppNotification } from "@/desktop/shared/notifications";
import type { BackgroundSyncFailurePayload, BackgroundSyncPulledTaskPayload, BackgroundSyncRegisteredWorktreePayload } from "@/lib/boardNotifier";

function getBackgroundSyncFailureLabelKey(operation: BackgroundSyncFailurePayload["operation"]) {
  if (operation === "pull-request-sync") {
    return "pullRequestFailure";
  }

  if (operation === "task-pull-sync") {
    return "taskPullFailure";
  }

  return "worktreeFailure";
}

export default function BackgroundSyncReviewDialog() {
  const tc = useTranslations("common");
  const tr = useTranslations("common.backgroundSyncReview");
  const tm = useTranslations("common.prMergeAlert");
  const state = useBackgroundSyncReview();
  useEffect(() => window.kanvibeDesktop?.onNotificationActivated?.((notification: AppNotification) => openBackgroundSyncReview(notification)), []);
  useEscapeKey(closeBackgroundSyncReview, { enabled: state.isOpen });
  if (!state.isOpen || state.notification?.action?.type !== "background-sync-review") return null;
  const backgroundSyncReview = state.notification.action.payload;
  const isPrMergeDeletePending = state.isCleaning;
  const selectedPrMergeEventKeys = state.selectedTaskIds;
  const selectedPrMergeEventKeySet = new Set(state.selectedTaskIds);
  const prMergeDeleteResultByTaskId = state.results;
  const handlePrMergeCancel = closeBackgroundSyncReview;
  const handlePrMergeConfirm = () => { void deleteBackgroundSyncReviewTasks(); };
  const handleRetryFailedPrMergeDeletes = () => { void deleteBackgroundSyncReviewTasks(true); };

  const pulledTasks = backgroundSyncReview.pulledTasks ?? [];
  const backgroundSyncFailures = backgroundSyncReview.failures ?? [];
  const hasSelectedPrMergeTasks = selectedPrMergeEventKeys.length > 0;
  const prMergeDeleteResults = Object.values(prMergeDeleteResultByTaskId);
  const prMergeDeleteProcessedCount = prMergeDeleteResults.filter((result) => result.status !== "pending").length;
  const prMergeDeleteFailedCount = prMergeDeleteResults.filter((result) => result.status === "failed").length;
  const hasPrMergeDeleteStarted = prMergeDeleteResults.length > 0;
  const hasPrMergeDeleteFailures = !isPrMergeDeletePending && prMergeDeleteFailedCount > 0;

  return (
    <div data-terminal-focus-blocker="true" className="fixed inset-0 z-[520] flex items-center justify-center bg-bg-overlay p-4">
      <div role="dialog" aria-modal="true" aria-label={tr("title")} className="w-full max-w-2xl max-h-[calc(100dvh-2rem)] overflow-y-auto rounded-xl border border-border-default bg-bg-surface p-6 shadow-lg break-words [overflow-wrap:anywhere]">
        <h2 className="mb-2 text-lg font-semibold text-text-primary">
          {tr("title")}
        </h2>
        <p className="text-sm text-text-secondary">
          {tr("message")}
          <span className="mt-2 block">{tr("backgroundProcessing")}</span>
        </p>
        <div className="mt-4 max-h-[50dvh] space-y-5 overflow-y-auto pr-1">
          <section>
            <h3 className="text-sm font-semibold text-text-primary">{tr("mergedSection")}</h3>
            {backgroundSyncReview.mergedPullRequests.length === 0 ? (
              <p className="mt-2 text-sm text-text-muted">{tr("emptyMerged")}</p>
            ) : (
              <div className="mt-3 space-y-3">
                {backgroundSyncReview.mergedPullRequests.map((alert) => {
                  const eventKey = alert.taskId;
                  const deleteResult = prMergeDeleteResultByTaskId[alert.taskId];
                  return (
                    <label
                      key={eventKey}
                      className="flex cursor-pointer items-start gap-3 rounded-lg border border-border-default bg-bg-page px-4 py-3"
                    >
                      <input
                        type="checkbox"
                        checked={selectedPrMergeEventKeySet.has(eventKey)}
                        onChange={() => toggleBackgroundSyncReviewTask(eventKey)}
                        disabled={hasPrMergeDeleteStarted}
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
                        {deleteResult?.status === "failed" ? (
                          <p className="mt-2 text-xs text-status-error">{deleteResult.reason}</p>
                        ) : null}
                      </div>
                      {deleteResult ? (
                        <span
                          className={
                            deleteResult.status === "failed"
                              ? "inline-flex shrink-0 items-center rounded-full bg-red-50 px-2 py-0.5 text-[11px] text-status-error"
                              : deleteResult.status === "succeeded"
                                ? "inline-flex shrink-0 items-center rounded-full bg-green-50 px-2 py-0.5 text-[11px] text-status-done"
                                : "inline-flex shrink-0 items-center gap-1 rounded-full bg-bg-surface px-2 py-0.5 text-[11px] text-text-muted"
                          }
                        >
                          {deleteResult.status === "pending" ? (
                            <span aria-hidden="true" className="h-2.5 w-2.5 rounded-full border-2 border-current border-t-transparent motion-safe:animate-spin" />
                          ) : null}
                          {deleteResult.status === "pending"
                            ? tc("delete")
                            : tr(deleteResult.status === "succeeded" ? "deleteSucceeded" : "deleteFailed")}
                        </span>
                      ) : null}
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
                          {tr(getBackgroundSyncFailureLabelKey(failure.operation))}
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
        {hasPrMergeDeleteStarted ? (
          <p
            role={hasPrMergeDeleteFailures ? "alert" : "status"}
            className={`mt-4 text-sm ${hasPrMergeDeleteFailures ? "text-status-error" : "text-text-secondary"}`}
          >
            {isPrMergeDeletePending
              ? tr("deleteProgress", { processed: prMergeDeleteProcessedCount, total: prMergeDeleteResults.length })
              : tr("deleteSummary", {
                total: prMergeDeleteResults.length,
                succeeded: prMergeDeleteResults.length - prMergeDeleteFailedCount,
                failed: prMergeDeleteFailedCount,
              })}
          </p>
        ) : null}
        <div className="mt-5 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            onClick={handlePrMergeCancel}
            className="rounded-md border border-border-default bg-bg-page px-4 py-1.5 text-sm text-text-secondary transition-colors hover:border-brand-primary"
          >
            {tc(hasPrMergeDeleteStarted ? "close" : "cancel")}
          </button>
          {hasPrMergeDeleteFailures ? (
            <button
              type="button"
              onClick={handleRetryFailedPrMergeDeletes}
              className="rounded-md bg-brand-primary px-4 py-1.5 text-sm text-text-inverse transition-colors hover:bg-brand-hover"
            >
              {tr("retryFailedDeletes")}
            </button>
          ) : (
            <button
              type="button"
              onClick={handlePrMergeConfirm}
              disabled={isPrMergeDeletePending || hasPrMergeDeleteStarted}
              aria-busy={isPrMergeDeletePending || undefined}
              className="rounded-md bg-brand-primary px-4 py-1.5 text-sm text-text-inverse transition-colors hover:bg-brand-hover disabled:opacity-50"
            >
              {hasSelectedPrMergeTasks ? tr("deleteInBackground") : tc("confirm")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
