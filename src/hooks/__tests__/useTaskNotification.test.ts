import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, cleanup, act } from "@testing-library/react";

// --- Mocks ---

const mockShowNotification = vi.fn().mockResolvedValue(undefined);

const mockServiceWorkerRegistration = {
  showNotification: mockShowNotification,
  active: { state: "activated" },
  installing: null,
  waiting: null,
  controller: null,
  scope: "http://localhost:3000/",
};

const mockGetRegistration = vi.fn().mockResolvedValue(mockServiceWorkerRegistration);
const mockDesktopShowNotification = vi.fn().mockResolvedValue(true);

// Notification 글로벌 설정
const mockRequestPermission = vi.fn().mockResolvedValue("granted");
Object.defineProperty(global, "Notification", {
  value: {
    permission: "granted",
    requestPermission: mockRequestPermission,
  },
  configurable: true,
  writable: true,
});

// ServiceWorker 글로벌 설정
Object.defineProperty(global.navigator, "serviceWorker", {
  value: {
    getRegistration: mockGetRegistration,
  },
  configurable: true,
});

describe("useTaskNotification", () => {
  beforeEach(() => {
    mockShowNotification.mockClear();
    mockGetRegistration.mockClear();
    mockRequestPermission.mockClear();
    mockDesktopShowNotification.mockClear();

    delete window.kanvibeDesktop;

    // Notification permission 리셋
    Object.defineProperty(global, "Notification", {
      value: {
        permission: "granted",
        requestPermission: mockRequestPermission,
      },
      configurable: true,
      writable: true,
    });

    // getRegistration 리셋
    mockGetRegistration.mockResolvedValue(mockServiceWorkerRegistration);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("should call showNotification with correct title and body when registered", async () => {
    // Given
    const { useTaskNotification } = await import("@/hooks/useTaskNotification");
    const { result } = renderHook(() => useTaskNotification());

    // 충분한 시간을 줌 (useEffect 실행)
    await new Promise((resolve) => setTimeout(resolve, 10));

    // When
    await act(async () => {
      await result.current.notifyTaskStatusChanged({
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
    expect(mockShowNotification).toHaveBeenCalledWith(
      "kanvibe — feat/login",
      {
        body: "로그인 구현: review로 변경\nOAuth 연동",
        icon: "/icons/icon-192x192.png",
        data: { taskId: "task-123", locale: "ko" },
      }
    );
  });

  it("should omit description from body when description is null", async () => {
    // Given
    const { useTaskNotification } = await import("@/hooks/useTaskNotification");
    const { result } = renderHook(() => useTaskNotification());

    // 충분한 시간을 줌 (useEffect 실행)
    await new Promise((resolve) => setTimeout(resolve, 10));

    // When
    await act(async () => {
      await result.current.notifyTaskStatusChanged({
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
    expect(mockShowNotification).toHaveBeenCalledWith(
      "kanvibe — feat/test",
      {
        body: "테스트 작업: changed to progress",
        icon: "/icons/icon-192x192.png",
        data: { taskId: "task-456", locale: "en" },
      }
    );
  });

  it("should handle case when service worker registration is null", async () => {
    // Given
    mockGetRegistration.mockResolvedValue(null);
    const { useTaskNotification } = await import("@/hooks/useTaskNotification");
    const { result } = renderHook(() => useTaskNotification());

    await new Promise((resolve) => setTimeout(resolve, 10));

    // When
    await act(async () => {
      await result.current.notifyTaskStatusChanged({
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
    expect(mockShowNotification).not.toHaveBeenCalled();
  });

  it("should show missing target notification without taskId when hook target is missing", async () => {
    // Given
    const { useTaskNotification } = await import("@/hooks/useTaskNotification");
    const { result } = renderHook(() => useTaskNotification());

    await new Promise((resolve) => setTimeout(resolve, 10));

    // When
    await act(async () => {
      await result.current.notifyHookStatusTargetMissing({
        projectName: "case-study",
        branchName: "feat/test",
        requestedStatus: "review",
        reason: "task-not-found",
        locale: "ko",
      });
    });

    // Then
    expect(mockShowNotification).toHaveBeenCalledWith("case-study — feat/test", {
      body: "review 상태로 변경하지 못했습니다.\n브랜치에 연결된 작업을 찾지 못했습니다.",
      icon: "/icons/icon-192x192.png",
      data: { locale: "ko" },
    });
  });

  it("should localize missing target notification body with locale", async () => {
    // Given
    const { useTaskNotification } = await import("@/hooks/useTaskNotification");
    const { result } = renderHook(() => useTaskNotification());

    await new Promise((resolve) => setTimeout(resolve, 10));

    // When
    await act(async () => {
      await result.current.notifyHookStatusTargetMissing({
        projectName: "case-study",
        branchName: "feat/test",
        requestedStatus: "review",
        reason: "project-not-found",
        locale: "en",
      });
    });

    // Then
    expect(mockShowNotification).toHaveBeenCalledWith("case-study — feat/test", {
      body: "Failed to change status to review.\nProject was not found.",
      icon: "/icons/icon-192x192.png",
      data: { locale: "en" },
    });
  });

  it("should request permission when permission is default", async () => {
    // Given
    Object.defineProperty(global, "Notification", {
      value: {
        permission: "default",
        requestPermission: mockRequestPermission,
      },
      configurable: true,
      writable: true,
    });

    const { useTaskNotification } = await import("@/hooks/useTaskNotification");

    // When
    renderHook(() => useTaskNotification());

    await new Promise((resolve) => setTimeout(resolve, 10));

    // Then
    expect(mockRequestPermission).toHaveBeenCalled();
  });

  it("should use Electron native notification bridge on desktop", async () => {
    window.kanvibeDesktop = {
      isDesktop: true,
      showNotification: mockDesktopShowNotification,
    };

    const { useTaskNotification } = await import("@/hooks/useTaskNotification");
    const { result } = renderHook(() => useTaskNotification());

    await new Promise((resolve) => setTimeout(resolve, 10));

    await act(async () => {
      await result.current.notifyTaskStatusChanged({
        projectName: "kanvibe",
        branchName: "feat/electron",
        taskTitle: "네이티브 알림",
        description: "OS 알림 테스트",
        newStatus: "review",
        taskId: "task-desktop",
        locale: "ko",
      });
    });

    expect(mockDesktopShowNotification).toHaveBeenCalledWith({
      title: "kanvibe — feat/electron",
      body: "네이티브 알림: review로 변경\nOS 알림 테스트",
      taskId: "task-desktop",
      locale: "ko",
    });
    expect(mockShowNotification).not.toHaveBeenCalled();
  });
});
