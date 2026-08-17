import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect, useRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import NotificationCenterButton, { type NotificationCenterButtonHandle } from "@/components/NotificationCenterButton";
import { BoardCommandProvider, useBoardCommands } from "@/desktop/renderer/components/BoardCommandProvider";
import type { AppNotification } from "@/desktop/shared/notifications";

const {
  mockListNotifications,
  mockMarkNotificationRead,
  mockMarkAllNotificationsRead,
  mockActivateNotification,
  mockGetTaskById,
  mockRedirect,
  mockGetNotificationUnreadOnlyEnabled,
  mockSetNotificationUnreadOnlyEnabled,
} = vi.hoisted(() => ({
  mockListNotifications: vi.fn(),
  mockMarkNotificationRead: vi.fn(),
  mockMarkAllNotificationsRead: vi.fn(),
  mockActivateNotification: vi.fn(),
  mockGetTaskById: vi.fn(),
  mockRedirect: vi.fn(),
  mockGetNotificationUnreadOnlyEnabled: vi.fn(),
  mockSetNotificationUnreadOnlyEnabled: vi.fn(),
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

vi.mock("@/desktop/renderer/actions/appSettings", () => ({
  getNotificationUnreadOnlyEnabled: mockGetNotificationUnreadOnlyEnabled,
  setNotificationUnreadOnlyEnabled: mockSetNotificationUnreadOnlyEnabled,
}));

vi.mock("@/desktop/renderer/navigation", () => ({
  localizeHref: (href: string, currentLocale = "ko") => (
    href.startsWith("/") ? `/${currentLocale}${href}` : href
  ),
  redirect: mockRedirect,
  useRouter: () => ({
    back: vi.fn(),
    forward: vi.fn(),
    push: vi.fn(),
  }),
}));

function createNotification(
  id: string,
  { isRead, createdAt }: { isRead: boolean; createdAt: string },
): AppNotification {
  return {
    id,
    title: `${id} title`,
    body: "Body",
    taskId: `task-${id}`,
    relativePath: `/task/task-${id}`,
    locale: "en",
    isRead,
    createdAt,
    dedupeKey: `k-${id}`,
  };
}

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

function ShortcutBlocker() {
  const boardCommands = useBoardCommands();

  useEffect(() => boardCommands.registerShortcutBlocker(), [boardCommands]);

  return null;
}

function BlockedNotificationShortcutHarness() {
  return (
    <BoardCommandProvider>
      <ShortcutBlocker />
      <NotificationShortcutHarness />
    </BoardCommandProvider>
  );
}

describe("NotificationCenterButton", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetNotificationUnreadOnlyEnabled.mockResolvedValue(false);
    mockSetNotificationUnreadOnlyEnabled.mockResolvedValue(undefined);
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

  it("marks the highlighted notification read with Space without opening it", async () => {
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

    render(<NotificationCenterButton />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "notifications" }));
    fireEvent.keyDown(window, { key: "ArrowDown" });
    fireEvent.keyDown(window, { key: " " });

    await waitFor(() => {
      expect(mockMarkNotificationRead).toHaveBeenCalledWith("n2");
    });

    expect(mockGetTaskById).not.toHaveBeenCalled();
    expect(mockRedirect).not.toHaveBeenCalled();
    expect(screen.getByText("Second task")).toBeTruthy();
  });

  it("does not call the read bridge again when Space repeats on an already read notification", async () => {
    mockListNotifications.mockResolvedValue([
      {
        id: "n1",
        title: "Already read",
        body: "Body",
        taskId: "task-1",
        relativePath: "/task/task-1",
        locale: "en",
        isRead: true,
        createdAt: "2026-05-04T00:01:00.000Z",
        dedupeKey: "k1",
      },
    ]);

    render(<NotificationCenterButton />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "notifications" }));
    fireEvent.keyDown(window, { key: " " });

    expect(mockMarkNotificationRead).not.toHaveBeenCalled();
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

  it("ignores keyboard navigation while a shortcut blocker is active", async () => {
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
    ]);

    render(<BlockedNotificationShortcutHarness />);

    await waitFor(() => {
      expect(mockListNotifications).toHaveBeenCalled();
    });

    fireEvent.click(screen.getByRole("button", { name: "open notification shortcut" }));
    fireEvent.keyDown(window, { key: "Enter" });
    fireEvent.keyDown(window, { key: "Escape" });

    expect(mockGetTaskById).not.toHaveBeenCalled();
    expect(mockRedirect).not.toHaveBeenCalled();
    expect(screen.getByText("First task")).toBeTruthy();
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

  describe("안읽음만 보기 필터", () => {
    const unreadNewest = createNotification("unread-newest", { isRead: false, createdAt: "2026-05-04T00:02:00.000Z" });
    const readMiddle = createNotification("read-middle", { isRead: true, createdAt: "2026-05-04T00:01:00.000Z" });
    const unreadOldest = createNotification("unread-oldest", { isRead: false, createdAt: "2026-05-04T00:00:00.000Z" });

    async function renderOpenedPanel(notifications: AppNotification[]) {
      mockListNotifications.mockResolvedValue(notifications);
      mockMarkNotificationRead.mockResolvedValue(undefined);

      render(<NotificationCenterButton />);

      await waitFor(() => {
        expect(mockListNotifications).toHaveBeenCalled();
      });

      fireEvent.click(screen.getByRole("button", { name: "notifications" }));
    }

    it("안읽음 탭을 누르면 읽은 알림만 목록에서 사라진다", async () => {
      await renderOpenedPanel([unreadNewest, readMiddle, unreadOldest]);

      fireEvent.click(screen.getByRole("button", { name: /notificationFilterUnread/ }));

      await waitFor(() => {
        expect(screen.queryByText("read-middle title")).toBeNull();
      });
      expect(screen.getByText("unread-newest title")).toBeTruthy();
      expect(screen.getByText("unread-oldest title")).toBeTruthy();
    });

    it("고른 필터를 앱 설정에 저장한다", async () => {
      await renderOpenedPanel([unreadNewest, readMiddle]);

      fireEvent.click(screen.getByRole("button", { name: /notificationFilterUnread/ }));

      await waitFor(() => {
        expect(mockSetNotificationUnreadOnlyEnabled).toHaveBeenCalledWith(true);
      });

      fireEvent.click(screen.getByRole("button", { name: "notificationFilterAll" }));

      await waitFor(() => {
        expect(mockSetNotificationUnreadOnlyEnabled).toHaveBeenLastCalledWith(false);
      });
    });

    it("저장된 필터가 켜져 있으면 팝업을 열 때부터 안읽음만 보여준다", async () => {
      mockGetNotificationUnreadOnlyEnabled.mockResolvedValue(true);

      await renderOpenedPanel([unreadNewest, readMiddle]);

      await waitFor(() => {
        expect(screen.getByText("unread-newest title")).toBeTruthy();
      });
      expect(screen.queryByText("read-middle title")).toBeNull();
    });

    it("안읽음만 보기에서는 키보드 이동이 읽은 알림을 건너뛴다", async () => {
      mockGetNotificationUnreadOnlyEnabled.mockResolvedValue(true);
      mockGetTaskById.mockImplementation(async (taskId: string) => ({ id: taskId }));

      await renderOpenedPanel([unreadNewest, readMiddle, unreadOldest]);

      await waitFor(() => {
        expect(screen.queryByText("read-middle title")).toBeNull();
      });

      fireEvent.keyDown(window, { key: "ArrowDown" });
      fireEvent.keyDown(window, { key: "Enter" });

      await waitFor(() => {
        expect(mockGetTaskById).toHaveBeenCalledWith("task-unread-oldest");
      });
      expect(mockGetTaskById).not.toHaveBeenCalledWith("task-read-middle");
    });

    it("안읽음만 보기에서 Space로 읽음 처리하면 목록에서 바로 빠진다", async () => {
      mockGetNotificationUnreadOnlyEnabled.mockResolvedValue(true);

      await renderOpenedPanel([unreadNewest, unreadOldest]);

      await waitFor(() => {
        expect(screen.getByText("unread-newest title")).toBeTruthy();
      });

      fireEvent.keyDown(window, { key: " " });

      await waitFor(() => {
        expect(screen.queryByText("unread-newest title")).toBeNull();
      });
      expect(mockMarkNotificationRead).toHaveBeenCalledWith("unread-newest");
      expect(screen.getByText("unread-oldest title")).toBeTruthy();
    });

    it("안읽은 알림이 없으면 알림이 아예 없을 때와 다른 안내를 보여준다", async () => {
      mockGetNotificationUnreadOnlyEnabled.mockResolvedValue(true);

      await renderOpenedPanel([readMiddle]);

      await waitFor(() => {
        expect(screen.getByText("noUnreadNotifications")).toBeTruthy();
      });
      expect(screen.queryByText("noNotifications")).toBeNull();
    });
  });
});
