import type { DesktopNotificationPayload } from "@/desktop/shared/notifications";
import type { BackgroundSyncReviewPayload } from "@/lib/boardNotifier";

export type NotificationLocale = "ko" | "en" | "zh";

const NOTIFICATION_MESSAGES = {
  ko: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: ${newStatus}로 변경`,
    formatMissingStatus: (requestedStatus: string) => `${requestedStatus} 상태로 변경하지 못했습니다.`,
    taskNotFound: "연결된 작업을 찾지 못했습니다.",
    backgroundSyncReviewTitle: "백그라운드 sync 검토 필요",
    formatMergedPullRequestCount: (count: number) => `merge된 PR ${count}건`,
    formatRegisteredWorktreeCount: (count: number) => `새 TODO worktree ${count}건`,
    formatUpdatedPullCount: (count: number) => `pull 완료 ${count}건`,
    formatFailedPullCount: (count: number) => `pull 실패 ${count}건`,
    formatSyncFailureCount: (count: number) => `sync 실패 ${count}건`,
    formatPullFailureDetail: (target: string, reason: string) => `pull 실패: ${target}: ${reason}`,
    formatSyncFailureDetail: (target: string, reason: string) => `실패: ${target}: ${reason}`,
    formatAdditionalFailureCount: (count: number) => `추가 실패 ${count}건`,
    backgroundSyncReviewPrompt: "알림을 열어 정리 대상을 검토하세요.",
  },
  en: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: changed to ${newStatus}`,
    formatMissingStatus: (requestedStatus: string) => `Failed to change status to ${requestedStatus}.`,
    taskNotFound: "No matching task was found.",
    backgroundSyncReviewTitle: "Background sync review needed",
    formatMergedPullRequestCount: (count: number) => `${count} merged PR${count === 1 ? "" : "s"}`,
    formatRegisteredWorktreeCount: (count: number) => `${count} new TODO worktree${count === 1 ? "" : "s"}`,
    formatUpdatedPullCount: (count: number) => `${count} pull update${count === 1 ? "" : "s"}`,
    formatFailedPullCount: (count: number) => `${count} failed pull${count === 1 ? "" : "s"}`,
    formatSyncFailureCount: (count: number) => `${count} sync failure${count === 1 ? "" : "s"}`,
    formatPullFailureDetail: (target: string, reason: string) => `Pull failed: ${target}: ${reason}`,
    formatSyncFailureDetail: (target: string, reason: string) => `Failed: ${target}: ${reason}`,
    formatAdditionalFailureCount: (count: number) => `${count} more failure${count === 1 ? "" : "s"}`,
    backgroundSyncReviewPrompt: "Open the notification to review the pending cleanup items.",
  },
  zh: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: 已变更为${newStatus}`,
    formatMissingStatus: (requestedStatus: string) => `未能变更为 ${requestedStatus} 状态。`,
    taskNotFound: "未找到匹配的任务。",
    backgroundSyncReviewTitle: "需要检查后台同步结果",
    formatMergedPullRequestCount: (count: number) => `已合并 PR ${count} 个`,
    formatRegisteredWorktreeCount: (count: number) => `新建 TODO worktree ${count} 个`,
    formatUpdatedPullCount: (count: number) => `pull 完成 ${count} 个`,
    formatFailedPullCount: (count: number) => `pull 失败 ${count} 个`,
    formatSyncFailureCount: (count: number) => `sync 失败 ${count} 个`,
    formatPullFailureDetail: (target: string, reason: string) => `pull 失败：${target}：${reason}`,
    formatSyncFailureDetail: (target: string, reason: string) => `失败：${target}：${reason}`,
    formatAdditionalFailureCount: (count: number) => `另有失败 ${count} 个`,
    backgroundSyncReviewPrompt: "打开通知以检查待整理项目。",
  },
} as const;

export interface TaskStatusNotification {
  projectName: string;
  branchName: string;
  taskTitle: string;
  description: string | null;
  newStatus: string;
  taskId: string;
  locale: string;
}

export interface HookStatusTargetMissingNotification {
  taskId: string;
  requestedStatus: string;
  reason: "task-not-found";
  locale: string;
}

export interface BackgroundSyncReviewNotification extends BackgroundSyncReviewPayload {
  locale: string;
}

interface PreparedNotification {
  title: string;
  body: string;
  desktopPayload: DesktopNotificationPayload;
}

export function getNotificationLocale(locale: string): NotificationLocale {
  if (locale.startsWith("en")) return "en";
  if (locale.startsWith("zh")) return "zh";
  return "ko";
}

export function buildTaskStatusNotification(payload: TaskStatusNotification): PreparedNotification {
  const messages = NOTIFICATION_MESSAGES[getNotificationLocale(payload.locale)];
  const title = `${payload.projectName} — ${payload.branchName}`;
  const bodyParts = [messages.formatStatusChanged(payload.taskTitle, payload.newStatus)];
  if (payload.description) {
    bodyParts.push(payload.description);
  }

  return {
    title,
    body: bodyParts.join("\n"),
    desktopPayload: {
      title,
      body: bodyParts.join("\n"),
      taskId: payload.taskId,
      locale: payload.locale,
      relativePath: `/${payload.locale}/task/${payload.taskId}`,
      dedupeKey: `task-status:${payload.taskId}:${payload.newStatus}`,
    },
  };
}

export function buildHookStatusTargetMissingNotification(payload: HookStatusTargetMissingNotification): PreparedNotification {
  const messages = NOTIFICATION_MESSAGES[getNotificationLocale(payload.locale)];
  const title = `Hook target missing — ${payload.taskId}`;
  const body = `${messages.formatMissingStatus(payload.requestedStatus)}\n${messages.taskNotFound}`;

  return {
    title,
    body,
    desktopPayload: {
      title,
      body,
      locale: payload.locale,
      relativePath: `/${payload.locale}`,
      dedupeKey: `hook-missing:${payload.taskId}:${payload.requestedStatus}:${payload.reason}`,
    },
  };
}

export function buildBackgroundSyncReviewNotification(payload: BackgroundSyncReviewNotification): PreparedNotification {
  const messages = NOTIFICATION_MESSAGES[getNotificationLocale(payload.locale)];
  const title = messages.backgroundSyncReviewTitle;
  const summaryLines: string[] = [];
  const pulledTasks = payload.pulledTasks ?? [];
  const failures = payload.failures ?? [];

  if (payload.mergedPullRequests.length > 0) {
    summaryLines.push(messages.formatMergedPullRequestCount(payload.mergedPullRequests.length));
  }

  if (payload.registeredWorktrees.length > 0) {
    summaryLines.push(messages.formatRegisteredWorktreeCount(payload.registeredWorktrees.length));
  }

  const updatedPullCount = pulledTasks.filter((item) => item.status === "updated").length;
  const failedPullCount = pulledTasks.filter((item) => item.status === "failed").length;
  if (updatedPullCount > 0) {
    summaryLines.push(messages.formatUpdatedPullCount(updatedPullCount));
  }
  if (failedPullCount > 0) {
    summaryLines.push(messages.formatFailedPullCount(failedPullCount));
  }
  if (failures.length > 0) {
    summaryLines.push(messages.formatSyncFailureCount(failures.length));
  }

  const failedPullTasks = pulledTasks.filter((item) => item.status === "failed");
  const detailLines = [
    ...failedPullTasks.slice(0, 3).map((item) => (
      messages.formatPullFailureDetail(`${item.taskTitle} (${item.branchName})`, item.summary)
    )),
    ...(failedPullTasks.length > 3 ? [messages.formatAdditionalFailureCount(failedPullTasks.length - 3)] : []),
    ...failures.slice(0, 3).map((item) => messages.formatSyncFailureDetail(item.target, item.reason)),
    ...(failures.length > 3 ? [messages.formatAdditionalFailureCount(failures.length - 3)] : []),
  ];

  const body = [summaryLines.join(" / "), ...detailLines, messages.backgroundSyncReviewPrompt]
    .filter(Boolean)
    .join("\n");
  const mergedKeys = payload.mergedPullRequests
    .map((item) => `${item.taskId}:${item.prUrl}:${item.mergedAt}`)
    .sort()
    .join("|");
  const worktreeKeys = payload.registeredWorktrees
    .map((item) => `${item.taskId}:${item.branchName}:${item.worktreePath}`)
    .sort()
    .join("|");
  const pullKeys = pulledTasks
    .map((item) => `${item.taskId}:${item.branchName}:${item.status}:${item.summary}`)
    .sort()
    .join("|");
  const failureKeys = failures
    .map((item) => `${item.operation}:${item.taskId ?? item.target}:${item.branchName ?? ""}:${item.reason}`)
    .sort()
    .join("|");
  const dedupeKey = `background-sync-review:${mergedKeys}::${worktreeKeys}::${pullKeys}`;

  return {
    title,
    body,
    desktopPayload: {
      title,
      body,
      locale: payload.locale,
      relativePath: `/${payload.locale}`,
      dedupeKey: failureKeys ? `${dedupeKey}::${failureKeys}` : dedupeKey,
      action: {
        type: "background-sync-review",
        payload: {
          mergedPullRequests: payload.mergedPullRequests,
          registeredWorktrees: payload.registeredWorktrees,
          pulledTasks,
          ...(failures.length > 0 ? { failures } : {}),
        },
      },
    },
  };
}
