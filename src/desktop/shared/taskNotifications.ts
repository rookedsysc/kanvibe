import type { DesktopNotificationPayload } from "@/desktop/shared/notifications";
import type { BackgroundSyncReviewPayload, BoardEventPayload } from "@/lib/boardNotifier";

export type NotificationLocale = "ko" | "en" | "zh";

const NOTIFICATION_MESSAGES = {
  ko: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: ${newStatus}로 변경`,
    formatMissingStatus: (requestedStatus: string) => `${requestedStatus} 상태로 변경하지 못했습니다.`,
    taskNotFound: "연결된 작업을 찾지 못했습니다.",
    hookTunnelFailure: {
      ssh: { title: "SSH 연결 실패", hint: "네트워크, VPN, 호스트 전원 및 SSH 설정을 확인하세요." },
      handshake: { title: "KanVibe 핸드셰이크 실패", hint: "원격 daemon과 데스크톱의 KanVibe 버전 및 설정을 확인하세요." },
      "client-in-use": { title: "KanVibe 연결 거부", hint: "같은 클라이언트 ID의 기존 연결이 응답했습니다. 기존 데스크톱의 연결을 종료한 뒤 다시 시도하세요." },
      setup: { title: "Hook 터널 준비 실패", hint: "원격 호스트의 KanVibe 실행 환경을 확인하세요." },
    },
    hookTunnelRecovered: { title: "Hook 터널 복구", body: "같은 클라이언트 ID의 기존 연결이 남아 있었습니다. 응답이 없어 기존 연결을 교체했습니다." },
    backgroundSyncReviewTitle: "백그라운드 sync 검토 필요",
    formatMergedPullRequestCount: (count: number) => `merge된 PR ${count}건`,
    formatRegisteredWorktreeCount: (count: number) => `새 TODO worktree ${count}건`,
    formatUpdatedPullCount: (count: number) => `pull 완료 ${count}건`,
    formatFailedPullCount: (count: number) => `pull 실패 ${count}건`,
    formatSyncFailureCount: (count: number) => `sync 실패 ${count}건`,
  },
  en: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: changed to ${newStatus}`,
    formatMissingStatus: (requestedStatus: string) => `Failed to change status to ${requestedStatus}.`,
    taskNotFound: "No matching task was found.",
    hookTunnelFailure: {
      ssh: { title: "SSH connection failed", hint: "Check the network, VPN, host power, and SSH configuration." },
      handshake: { title: "KanVibe handshake failed", hint: "Check the KanVibe versions and configuration on the remote host and desktop." },
      "client-in-use": { title: "KanVibe connection rejected", hint: "An existing connection with this client ID responded. Disconnect the other desktop and try again." },
      setup: { title: "Hook tunnel setup failed", hint: "Check the KanVibe runtime on the remote host." },
    },
    hookTunnelRecovered: { title: "Hook tunnel recovered", body: "An existing connection with this client ID did not respond and was replaced." },
    backgroundSyncReviewTitle: "Background sync review needed",
    formatMergedPullRequestCount: (count: number) => `${count} merged PR${count === 1 ? "" : "s"}`,
    formatRegisteredWorktreeCount: (count: number) => `${count} new TODO worktree${count === 1 ? "" : "s"}`,
    formatUpdatedPullCount: (count: number) => `${count} pull update${count === 1 ? "" : "s"}`,
    formatFailedPullCount: (count: number) => `${count} failed pull${count === 1 ? "" : "s"}`,
    formatSyncFailureCount: (count: number) => `${count} sync failure${count === 1 ? "" : "s"}`,
  },
  zh: {
    formatStatusChanged: (taskTitle: string, newStatus: string) => `${taskTitle}: 已变更为${newStatus}`,
    formatMissingStatus: (requestedStatus: string) => `未能变更为 ${requestedStatus} 状态。`,
    taskNotFound: "未找到匹配的任务。",
    hookTunnelFailure: {
      ssh: { title: "SSH 连接失败", hint: "请检查网络、VPN、主机电源和 SSH 配置。" },
      handshake: { title: "KanVibe 握手失败", hint: "请检查远程主机和桌面端的 KanVibe 版本和配置。" },
      "client-in-use": { title: "KanVibe 连接被拒绝", hint: "使用相同客户端 ID 的现有连接已响应。请断开另一台桌面端的连接后重试。" },
      setup: { title: "Hook 隧道初始化失败", hint: "请检查远程主机上的 KanVibe 运行环境。" },
    },
    hookTunnelRecovered: { title: "Hook 隧道已恢复", body: "使用相同客户端 ID 的现有连接没有响应，已将其替换。" },
    backgroundSyncReviewTitle: "需要检查后台同步结果",
    formatMergedPullRequestCount: (count: number) => `已合并 PR ${count} 个`,
    formatRegisteredWorktreeCount: (count: number) => `新建 TODO worktree ${count} 个`,
    formatUpdatedPullCount: (count: number) => `pull 完成 ${count} 个`,
    formatFailedPullCount: (count: number) => `pull 失败 ${count} 个`,
    formatSyncFailureCount: (count: number) => `sync 失败 ${count} 个`,
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

export function buildHookTunnelNotification(
  payload: Extract<BoardEventPayload, { type: "hook-tunnel-failed" | "hook-tunnel-recovered" }> & { locale: string },
): DesktopNotificationPayload {
  const messages = NOTIFICATION_MESSAGES[getNotificationLocale(payload.locale)];
  const { title, body } = payload.type === "hook-tunnel-recovered" ? messages.hookTunnelRecovered : {
    title: messages.hookTunnelFailure[payload.kind].title,
    body: `${messages.hookTunnelFailure[payload.kind].hint}\n${payload.reason}`,
  };
  return {
    title: `${payload.sshHost} — ${title}`,
    body,
    locale: payload.locale,
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

  const body = summaryLines.join(" / ");
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
