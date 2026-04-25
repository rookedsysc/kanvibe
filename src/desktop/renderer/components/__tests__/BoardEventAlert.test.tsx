import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import BoardEventAlert from "@/desktop/renderer/components/BoardEventAlert";

let boardEventListener: ((event: any) => void) | null = null;

vi.mock("next-intl", () => ({
  useTranslations: (namespace: string) => (key: string, values?: Record<string, unknown>) => {
    if (values?.taskTitle) {
      return `${namespace}.${key}:${values.taskTitle}`;
    }
    return `${namespace}.${key}`;
  },
}));

describe("BoardEventAlert", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    boardEventListener = null;

    window.kanvibeDesktop = {
      onBoardEvent: vi.fn((listener) => {
        boardEventListener = listener;
        return () => {};
      }),
    } as any;
  });

  it("hooks 자동 설치 실패 이벤트가 오면 상단 알럿을 표시한다", async () => {
    // Given
    render(<BoardEventAlert />);

    // When
    await act(async () => {
      boardEventListener?.({
        type: "task-hook-install-failed",
        taskId: "task-1",
        taskTitle: "로그인 개선",
        error: "codex config failed",
      });
    });

    // Then
    expect(screen.getByRole("alert")).toBeTruthy();
    expect(screen.getByText("task.createHooksBackgroundFailedTitle")).toBeTruthy();
    expect(screen.getByText("task.createHooksBackgroundFailedBody:로그인 개선")).toBeTruthy();
  });

  it("알럿은 자동으로 사라진다", async () => {
    // Given
    render(<BoardEventAlert />);

    // When
    await act(async () => {
      boardEventListener?.({
        type: "task-hook-install-failed",
        taskId: "task-1",
        taskTitle: "로그인 개선",
        error: "codex config failed",
      });
    });
    await act(async () => {
      vi.advanceTimersByTime(5000);
    });

    // Then
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("닫기 버튼으로 즉시 dismiss 할 수 있다", async () => {
    // Given
    render(<BoardEventAlert />);
    await act(async () => {
      boardEventListener?.({
        type: "task-hook-install-failed",
        taskId: "task-1",
        taskTitle: "로그인 개선",
        error: "codex config failed",
      });
    });

    // When
    fireEvent.click(screen.getByRole("button", { name: "common.close" }));

    // Then
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
