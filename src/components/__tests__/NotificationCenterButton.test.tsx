import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import NotificationCenterButton, { type NotificationCenterButtonHandle } from "@/components/NotificationCenterButton";

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
  localizeHref: (href: string, currentLocale = "ko") => (
    href.startsWith("/") ? `/${currentLocale}${href}` : href
  ),
  redirect: mockRedirect,
}));

function NotificationShortcutHarness() {
  const notificationCenterRef = useRef<NotificationCenterButtonHandle>(null);

  return (
    <>
      <button type="button" onClick={() => notificationCenterRef.current?.toggle()}>
        open notification shortcut
      </button>
      <NotificationCenterButton ref={notificationCenterRef} />
    </>
  );
}

describe("NotificationCenterButton", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.history.replaceState({}, "", "/#/en");
    window.kanvibeDesktop = {
      onNotificationsChanged: vi.fn(() => undefined),
    } as Partial<NonNullable<Window["kanvibeDesktop"]>> as NonNullable<Window["kanvibeDesktop"]>;
  });

  afterEach(() => {
    vi.restoreAllMocks();
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

  it("focuses an existing task window instead of redirecting when opening a task notification", async () => {
    const focusExistingInternalRoute = vi.fn().mockResolvedValue(true);
    window.kanvibeDesktop = {
      isDesktop: true,
      onNotificationsChanged: vi.fn(() => undefined),
      focusExistingInternalRoute,
    } as Partial<NonNullable<Window["kanvibeDesktop"]>> as NonNullable<Window["kanvibeDesktop"]>;
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
      expect(focusExistingInternalRoute).toHaveBeenCalledWith("/en/task/task-1");
    });
    expect(mockRedirect).not.toHaveBeenCalled();
  });

  it("focuses the notification panel when opened through the shortcut handle", async () => {
    mockListNotifications.mockResolvedValue([
      {
        id: "n1",
        title: "Focusable task",
        body: "Body",
        taskId: "task-1",
        relativePath: "/task/task-1",
        locale: "en",
        isRead: false,
        createdAt: new Date().toISOString(),
        dedupeKey: "k1",
      },
    ]);

    render(<NotificationShortcutHarness />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "open notification shortcut" }));

    const panel = await screen.findByRole("dialog", { name: "notifications" });
    await waitFor(() => {
      expect(document.activeElement).toBe(panel);
    });
  });

  it("starts keyboard selection from the newest notification when opened", async () => {
    mockListNotifications.mockResolvedValue([
      {
        id: "n-old",
        title: "Older task",
        body: "Body",
        taskId: "task-old",
        relativePath: "/task/task-old",
        locale: "en",
        isRead: false,
        createdAt: "2026-05-04T00:00:00.000Z",
        dedupeKey: "k-old",
      },
      {
        id: "n-new",
        title: "Newest task",
        body: "Body",
        taskId: "task-new",
        relativePath: "/task/task-new",
        locale: "en",
        isRead: false,
        createdAt: "2026-05-04T00:01:00.000Z",
        dedupeKey: "k-new",
      },
    ]);
    mockMarkNotificationRead.mockResolvedValue(undefined);
    mockGetTaskById.mockImplementation(async (taskId: string) => ({ id: taskId }));

    render(<NotificationShortcutHarness />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "open notification shortcut" }));
    fireEvent.keyDown(window, { key: "Enter" });

    await waitFor(() => {
      expect(mockGetTaskById).toHaveBeenCalledWith("task-new");
    });
    expect(mockRedirect).toHaveBeenCalledWith("/en/task/task-new");
  });

  it("opens task notifications in a new window with Shift+Click", async () => {
    const openWindow = vi.spyOn(window, "open").mockImplementation(() => null);
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
    fireEvent.click(screen.getByRole("button", { name: /Existing task/i }), { shiftKey: true });

    await waitFor(() => {
      expect(openWindow).toHaveBeenCalledWith(`${window.location.origin}/#/en/task/task-1`, "_blank", "noopener,noreferrer");
    });
    expect(mockRedirect).not.toHaveBeenCalled();
  });

  it("supports arrow navigation and Enter to open the highlighted notification", async () => {
    mockListNotifications.mockResolvedValue([
      {
        id: "n1",
        title: "First task",
        body: "Body",
        taskId: "task-1",
        relativePath: "/task/task-1",
        locale: "en",
        isRead: false,
        createdAt: "2026-05-04T00:01:00.000Z",
        dedupeKey: "k1",
      },
      {
        id: "n2",
        title: "Second task",
        body: "Body",
        taskId: "task-2",
        relativePath: "/task/task-2",
        locale: "en",
        isRead: false,
        createdAt: "2026-05-04T00:00:00.000Z",
        dedupeKey: "k2",
      },
    ]);
    mockMarkNotificationRead.mockResolvedValue(undefined);
    mockGetTaskById.mockImplementation(async (taskId: string) => ({ id: taskId }));

    render(<NotificationCenterButton />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "notifications" }));
    fireEvent.keyDown(window, { key: "ArrowDown" });
    fireEvent.keyDown(window, { key: "Enter" });

    await waitFor(() => {
      expect(mockGetTaskById).toHaveBeenCalledWith("task-2");
    });
    expect(mockRedirect).toHaveBeenCalledWith("/en/task/task-2");
  });

  it("opens the highlighted task notification in a new window with Shift+Enter", async () => {
    const openWindow = vi.spyOn(window, "open").mockImplementation(() => null);
    mockListNotifications.mockResolvedValue([
      {
        id: "n1",
        title: "First task",
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
    fireEvent.keyDown(window, { key: "Enter", shiftKey: true });

    await waitFor(() => {
      expect(openWindow).toHaveBeenCalledWith(`${window.location.origin}/#/en/task/task-1`, "_blank", "noopener,noreferrer");
    });
    expect(mockRedirect).not.toHaveBeenCalled();
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

  it("closes the dropdown when Escape is pressed", async () => {
    mockListNotifications.mockResolvedValue([
      {
        id: "n1",
        title: "Only task",
        body: "Body",
        taskId: "task-1",
        relativePath: "/task/task-1",
        locale: "en",
        isRead: false,
        createdAt: new Date().toISOString(),
        dedupeKey: "k1",
      },
    ]);

    render(<NotificationCenterButton />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "notifications" }));
    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(screen.queryByText("Only task")).toBeNull();
    });
  });
});
