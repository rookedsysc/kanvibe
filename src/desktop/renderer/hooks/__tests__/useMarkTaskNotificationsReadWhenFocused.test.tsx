import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMarkTaskNotificationsReadWhenFocused } from "../useMarkTaskNotificationsReadWhenFocused";

const mocks = vi.hoisted(() => ({
  markTaskNotificationsRead: vi.fn(),
}));

vi.mock("@/desktop/renderer/actions/notifications", () => ({
  markTaskNotificationsRead: (...args: unknown[]) => mocks.markTaskNotificationsRead(...args),
}));

function MarkReadHarness({ taskId }: { taskId: string | null }) {
  useMarkTaskNotificationsReadWhenFocused(taskId);

  return <div data-testid="harness" />;
}

function setWindowFocused(isFocused: boolean) {
  vi.mocked(document.hasFocus).mockReturnValue(isFocused);
}

describe("useMarkTaskNotificationsReadWhenFocused", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.markTaskNotificationsRead.mockResolvedValue(0);
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("창이 활성인 상태로 상세 화면에 들어오면 그 task의 알림을 읽음 처리한다", async () => {
    setWindowFocused(true);

    render(<MarkReadHarness taskId="task-1" />);

    await waitFor(() => {
      expect(mocks.markTaskNotificationsRead).toHaveBeenCalledWith("task-1");
    });
  });

  it("창이 비활성이면 상세 화면이 열려 있어도 읽음 처리하지 않는다", async () => {
    setWindowFocused(false);

    render(<MarkReadHarness taskId="task-1" />);

    await act(async () => {});

    expect(mocks.markTaskNotificationsRead).not.toHaveBeenCalled();
  });

  it("비활성으로 열려 있던 상세 화면이 포커스를 받으면 그때 읽음 처리한다", async () => {
    setWindowFocused(false);

    render(<MarkReadHarness taskId="task-1" />);

    await act(async () => {});
    expect(mocks.markTaskNotificationsRead).not.toHaveBeenCalled();

    setWindowFocused(true);
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });

    expect(mocks.markTaskNotificationsRead).toHaveBeenCalledWith("task-1");
  });

  it("비활성 상태에서 화면이 다시 그려져도 읽음 처리하지 않는다", async () => {
    setWindowFocused(false);

    const { rerender } = render(<MarkReadHarness taskId="task-1" />);
    rerender(<MarkReadHarness taskId="task-1" />);

    await act(async () => {});

    expect(mocks.markTaskNotificationsRead).not.toHaveBeenCalled();
  });

  it("아직 task를 확인하지 못했으면 읽음 처리하지 않는다", async () => {
    setWindowFocused(true);

    render(<MarkReadHarness taskId={null} />);

    await act(async () => {});

    expect(mocks.markTaskNotificationsRead).not.toHaveBeenCalled();
  });

  it("읽음 처리가 실패하면 로그만 남기고 화면은 그대로 둔다", async () => {
    const consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    setWindowFocused(true);
    mocks.markTaskNotificationsRead.mockRejectedValue(new Error("app settings write failed"));

    render(<MarkReadHarness taskId="task-1" />);

    await waitFor(() => {
      expect(consoleErrorSpy).toHaveBeenCalledWith("알림 읽음 처리 실패:", expect.any(Error));
    });
  });
});
