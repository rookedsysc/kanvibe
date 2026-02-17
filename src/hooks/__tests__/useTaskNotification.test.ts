import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, cleanup, act } from "@testing-library/react";

// --- Mocks ---

const mockNotification = vi.fn();

vi.stubGlobal("Notification", Object.assign(mockNotification, {
  permission: "granted",
  requestPermission: vi.fn().mockResolvedValue("granted"),
}));

describe("useTaskNotification", () => {
  beforeEach(() => {
    vi.resetModules();
    mockNotification.mockClear();
    (Notification as unknown as { permission: string }).permission = "granted";
    (Notification.requestPermission as ReturnType<typeof vi.fn>).mockResolvedValue("granted");
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("should create notification with correct title and body when permission is granted", async () => {
    // Given
    const { useTaskNotification } = await import("@/hooks/useTaskNotification");
    const { result } = renderHook(() => useTaskNotification());

    // When
    act(() => {
      result.current.notifyTaskStatusChanged({
        projectName: "kanvibe",
        branchName: "feat/login",
        taskTitle: "로그인 구현",
        description: "OAuth 연동",
        newStatus: "review",
        taskId: "task-123",
        locale: "ko",
      });
    });

    // Then
    expect(mockNotification).toHaveBeenCalledWith(
      "kanvibe — feat/login",
      {
        body: "로그인 구현: review로 변경\nOAuth 연동",
        icon: "/kanvibe-logo.svg",
        data: { taskId: "task-123", locale: "ko" },
      }
    );
  });

  it("should omit description from body when description is null", async () => {
    // Given
    const { useTaskNotification } = await import("@/hooks/useTaskNotification");
    const { result } = renderHook(() => useTaskNotification());

    // When
    act(() => {
      result.current.notifyTaskStatusChanged({
        projectName: "kanvibe",
        branchName: "feat/test",
        taskTitle: "테스트 작업",
        description: null,
        newStatus: "progress",
        taskId: "task-456",
        locale: "en",
      });
    });

    // Then
    expect(mockNotification).toHaveBeenCalledWith(
      "kanvibe — feat/test",
      {
        body: "테스트 작업: progress로 변경",
        icon: "/kanvibe-logo.svg",
        data: { taskId: "task-456", locale: "en" },
      }
    );
  });

  it("should not create notification when permission is denied", async () => {
    // Given
    (Notification as unknown as { permission: string }).permission = "denied";
    const { useTaskNotification } = await import("@/hooks/useTaskNotification");
    const { result } = renderHook(() => useTaskNotification());

    // When
    act(() => {
      result.current.notifyTaskStatusChanged({
        projectName: "kanvibe",
        branchName: "feat/test",
        taskTitle: "테스트",
        description: null,
        newStatus: "done",
        taskId: "task-789",
        locale: "zh",
      });
    });

    // Then
    expect(mockNotification).not.toHaveBeenCalled();
  });

  it("should request permission when permission is default", async () => {
    // Given
    (Notification as unknown as { permission: string }).permission = "default";
    const { useTaskNotification } = await import("@/hooks/useTaskNotification");

    // When
    renderHook(() => useTaskNotification());

    // Then
    expect(Notification.requestPermission).toHaveBeenCalled();
  });
});
