import { describe, expect, it } from "vitest";
import {
  buildBackgroundSyncReviewNotification,
  buildHookStatusTargetMissingNotification,
  buildTaskStatusNotification,
  getNotificationLocale,
} from "@/desktop/shared/taskNotifications";

describe("taskNotifications", () => {
  it("알 수 없는 locale은 한국어 메시지로 fallback한다", () => {
    // Given

    // When
    const locale = getNotificationLocale("fr-FR");

    // Then
    expect(locale).toBe("ko");
  });

  it("작업 상태 알림은 상세 경로와 dedupe key를 안정적으로 만든다", () => {
    // Given
    const payload = {
      projectName: "kanvibe",
      branchName: "fix/session-prompt",
      taskTitle: "세션 프롬프트 회귀",
      description: "원격 sshHost 전달 누락 수정",
      newStatus: "review",
      taskId: "task-1",
      locale: "fr-FR",
    };

    // When
    const notification = buildTaskStatusNotification(payload);

    // Then
    expect(notification.title).toBe("kanvibe — fix/session-prompt");
    expect(notification.body).toBe("세션 프롬프트 회귀: review로 변경\n원격 sshHost 전달 누락 수정");
    expect(notification.desktopPayload).toEqual({
      title: "kanvibe — fix/session-prompt",
      body: "세션 프롬프트 회귀: review로 변경\n원격 sshHost 전달 누락 수정",
      taskId: "task-1",
      locale: "fr-FR",
      relativePath: "/fr-FR/task/task-1",
      dedupeKey: "task-status:task-1:review",
    });
  });

  it("누락 알림은 locale별 실패 메시지와 루트 경로를 사용한다", () => {
    // Given
    const payload = {
      taskId: "task-404",
      requestedStatus: "pending",
      reason: "task-not-found" as const,
      locale: "zh-CN",
    };

    // When
    const notification = buildHookStatusTargetMissingNotification(payload);

    // Then
    expect(notification.title).toBe("Hook target missing — task-404");
    expect(notification.body).toBe("未能变更为 pending 状态。\n未找到匹配的任务。");
    expect(notification.desktopPayload).toEqual({
      title: "Hook target missing — task-404",
      body: "未能变更为 pending 状态。\n未找到匹配的任务。",
      locale: "zh-CN",
      relativePath: "/zh-CN",
      dedupeKey: "hook-missing:task-404:pending:task-not-found",
    });
  });

  it("background sync review 알림 body는 요약만 포함하고 상세는 action payload에 포함한다", () => {
    // Given
    const payload = {
      locale: "ko",
      mergedPullRequests: [],
      registeredWorktrees: [],
      pulledTasks: [
        {
          taskId: "task-pull-a",
          taskTitle: "Pull A",
          branchName: "feature/pull-a",
          worktreePath: "/workspace/repo__worktrees/pull-a",
          sshHost: null,
          status: "updated" as const,
          summary: "Fast-forward",
        },
        {
          taskId: "task-pull-b",
          taskTitle: "Pull B",
          branchName: "feature/pull-b",
          worktreePath: "/workspace/repo__worktrees/pull-b",
          sshHost: "remote-host",
          status: "failed" as const,
          summary: "Not possible to fast-forward",
        },
      ],
      failures: [
        {
          operation: "pull-request-sync" as const,
          target: "PR sync target (feature/pr-fail)",
          reason: "gh auth failed",
          taskId: "task-11",
          branchName: "feature/pr-fail",
        },
      ],
    };

    // When
    const notification = buildBackgroundSyncReviewNotification(payload);

    // Then
    expect(notification.body).toBe("pull 완료 1건 / pull 실패 1건 / sync 실패 1건");
    expect(notification.desktopPayload.dedupeKey).toBe(
      "background-sync-review:::::task-pull-a:feature/pull-a:updated:Fast-forward|task-pull-b:feature/pull-b:failed:Not possible to fast-forward::pull-request-sync:task-11:feature/pr-fail:gh auth failed",
    );
    expect(notification.desktopPayload.action).toEqual({
      type: "background-sync-review",
      payload: {
        mergedPullRequests: [],
        registeredWorktrees: [],
        pulledTasks: payload.pulledTasks,
        failures: payload.failures,
      },
    });
  });
});
