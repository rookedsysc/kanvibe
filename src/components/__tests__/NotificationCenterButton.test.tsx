import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import NotificationCenterButton from "@/components/NotificationCenterButton";

const { mockListNotifications, mockMarkNotificationRead, mockMarkAllNotificationsRead, mockActivateNotification, mockGetTaskById, mockRedirect } = vi.hoisted(() => ({
  mockListNotifications: vi.fn(),
  mockMarkNotificationRead: vi.fn(),
  mockMarkAllNotificationsRead: vi.fn(),
  mockActivateNotification: vi.fn(),
  mockGetTaskById: vi.fn(),
  mockRedirect: vi.fn(),
}));

vi.mock("next-intl", async () => {
  const actual = await vi.importActual("next-intl");
  return {
    ...actual,
    useTranslations: () => (key: string) => key,
  };
});

vi.mock("@/desktop/renderer/actions/notifications", () => ({
  listNotifications: mockListNotifications,
  markNotificationRead: mockMarkNotificationRead,
  markAllNotificationsRead: mockMarkAllNotificationsRead,
  activateNotification: mockActivateNotification,
}));

vi.mock("@/desktop/renderer/actions/kanban", () => ({
  getTaskById: mockGetTaskById,
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  redirect: mockRedirect,
}));

describe("NotificationCenterButton", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.kanvibeDesktop = {
      onNotificationsChanged: vi.fn(() => undefined),
    } as any;
  });

  it("shows a popup instead of redirecting when the task no longer exists", async () => {
    mockListNotifications.mockResolvedValue([
      {
        id: "n1",
        title: "Deleted task",
        body: "Body",
        taskId: "task-1",
        relativePath: "/task/task-1",
        locale: "en",
        isRead: false,
        createdAt: new Date().toISOString(),
        dedupeKey: "k1",
      },
    ]);
    mockMarkNotificationRead.mockResolvedValue(undefined);
    mockGetTaskById.mockResolvedValue(null);

    render(<NotificationCenterButton />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "notifications" }));
    fireEvent.click(screen.getByRole("button", { name: /Deleted task/i }));

    await waitFor(() => {
      expect(mockGetTaskById).toHaveBeenCalledWith("task-1");
    });

    expect(mockRedirect).not.toHaveBeenCalled();
    expect(screen.getByText("notificationTaskMissingTitle")).toBeTruthy();
  });

  it("navigates normally when the task still exists", async () => {
    mockListNotifications.mockResolvedValue([
      {
        id: "n1",
        title: "Existing task",
        body: "Body",
        taskId: "task-1",
        relativePath: "/task/task-1",
        locale: "en",
        isRead: false,
        createdAt: new Date().toISOString(),
        dedupeKey: "k1",
      },
    ]);
    mockMarkNotificationRead.mockResolvedValue(undefined);
    mockGetTaskById.mockResolvedValue({ id: "task-1" });

    render(<NotificationCenterButton />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "notifications" }));
    fireEvent.click(screen.getByRole("button", { name: /Existing task/i }));

    await waitFor(() => {
      expect(mockRedirect).toHaveBeenCalledWith("/en/task/task-1");
    });
  });

  it("uses activation bridge for background sync review notifications", async () => {
    mockListNotifications.mockResolvedValue([
      {
        id: "n-review",
        title: "Background sync review",
        body: "Review pending items",
        taskId: null,
        relativePath: "/en",
        locale: "en",
        isRead: false,
        createdAt: new Date().toISOString(),
        dedupeKey: "review-1",
        action: {
          type: "background-sync-review",
          payload: {
            mergedPullRequests: [],
            registeredWorktrees: [],
          },
        },
      },
    ]);
    mockMarkNotificationRead.mockResolvedValue(undefined);
    mockActivateNotification.mockResolvedValue(true);

    render(<NotificationCenterButton />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "notifications" }));
    fireEvent.click(screen.getByRole("button", { name: /Background sync review/i }));

    await waitFor(() => {
      expect(mockActivateNotification).toHaveBeenCalledWith("n-review");
    });

    expect(mockGetTaskById).not.toHaveBeenCalled();
    expect(mockRedirect).not.toHaveBeenCalled();
  });
});
