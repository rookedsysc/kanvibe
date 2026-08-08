import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useUnreadNotificationCountByTask } from "../useUnreadNotificationCountByTask";
import type { AppNotification } from "@/desktop/shared/notifications";

const mocks = vi.hoisted(() => ({
  listNotifications: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/notifications", () => ({
  listNotifications: (...args: unknown[]) => mocks.listNotifications(...args),
}));

function createNotification(overrides: Partial<AppNotification> = {}): AppNotification {
  return {
    id: "n1",
    title: "title",
    body: "body",
    taskId: "task-1",
    relativePath: "/ko/task/task-1",
    locale: "ko",
    isRead: false,
    createdAt: "2026-08-08T00:00:00.000Z",
    dedupeKey: "k1",
    action: null,
    ...overrides,
  };
}

function UnreadCountHarness() {
  const countByTask = useUnreadNotificationCountByTask();

  return <div data-testid="counts">{JSON.stringify(countByTask)}</div>;
}

describe("useUnreadNotificationCountByTask", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    delete window.kanvibeDesktop;
  });

  it("task별 미읽음 알림만 집계한다", async () => {
    mocks.listNotifications.mockResolvedValue([
      createNotification({ id: "n1", taskId: "task-1", isRead: false }),
      createNotification({ id: "n2", taskId: "task-1", isRead: false }),
      createNotification({ id: "n3", taskId: "task-1", isRead: true }),
      createNotification({ id: "n4", taskId: "task-2", isRead: false }),
      createNotification({ id: "n5", taskId: null, isRead: false }),
    ]);

    render(<UnreadCountHarness />);

    await waitFor(() => {
      expect(JSON.parse(screen.getByTestId("counts").textContent ?? "{}")).toEqual({
        "task-1": 2,
        "task-2": 1,
      });
    });
  });

  it("알림 변경 이벤트가 오면 개수를 다시 집계한다", async () => {
    let notifyChange = () => {};
    window.kanvibeDesktop = {
      onNotificationsChanged: (listener: () => void) => {
        notifyChange = listener;
        return () => {};
      },
    } as Partial<NonNullable<Window["kanvibeDesktop"]>> as NonNullable<Window["kanvibeDesktop"]>;
    mocks.listNotifications.mockResolvedValueOnce([
      createNotification({ id: "n1", taskId: "task-1", isRead: false }),
    ]);

    render(<UnreadCountHarness />);

    await waitFor(() => {
      expect(JSON.parse(screen.getByTestId("counts").textContent ?? "{}")).toEqual({ "task-1": 1 });
    });

    mocks.listNotifications.mockResolvedValueOnce([
      createNotification({ id: "n1", taskId: "task-1", isRead: true }),
    ]);
    notifyChange();

    await waitFor(() => {
      expect(JSON.parse(screen.getByTestId("counts").textContent ?? "{}")).toEqual({});
    });
  });
});
