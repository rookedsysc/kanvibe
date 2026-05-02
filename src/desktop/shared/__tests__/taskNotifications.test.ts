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

  it("백그라운드 sync 알림은 실패 개수와 대표 실패 이유를 포함한다", () => {
    // Given
    const payload = {
      locale: "ko",
      mergedPullRequests: [],
      registeredWorktrees: [],
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
    expect(notification.body).toBe(
      "sync 실패 1건\n실패: PR sync target (feature/pr-fail): gh auth failed\n알림을 열어 정리 대상을 검토하세요.",
    );
    expect(notification.desktopPayload.dedupeKey).toContain("task-11:feature/pr-fail:gh auth failed");
    expect(notification.desktopPayload.action?.payload.failures).toEqual(payload.failures);
  });
});
