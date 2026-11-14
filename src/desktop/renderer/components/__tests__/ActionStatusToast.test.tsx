import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ActionStatusToast, { dismissActionStatus, showActionStatus } from "@/desktop/renderer/components/ActionStatusToast";

vi.mock("next-intl", () => ({
  useTranslations: () => (key: string) => key,
}));

describe("ActionStatusToast", () => {
  const shownIds: string[] = [];

  afterEach(() => {
    act(() => {
      for (const id of shownIds.splice(0)) {
        dismissActionStatus(id);
      }
    });
    vi.useRealTimers();
  });

  it("보낸 결과를 진행·실패 종류에 맞는 역할로 보이고 닫기 버튼으로 지운다", () => {
    render(<ActionStatusToast />);

    act(() => {
      shownIds.push(showActionStatus({ kind: "progress", message: "syncing" }));
      shownIds.push(showActionStatus({ kind: "failure", message: "sync failed" }));
    });

    expect(screen.getByRole("status").textContent).toContain("syncing");
    expect(screen.getByRole("alert").textContent).toContain("sync failed");

    fireEvent.click(screen.getAllByRole("button", { name: "close" })[1]);

    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("status")).toBeTruthy();
  });

  it("액션 버튼을 누르면 콜백을 실행하고 알림을 닫는다", () => {
    const onAction = vi.fn();
    render(<ActionStatusToast />);

    act(() => {
      shownIds.push(showActionStatus({ kind: "success", message: "created", action: { label: "open", onAction } }));
    });
    fireEvent.click(screen.getByRole("button", { name: "open" }));

    expect(onAction).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("created")).toBeNull();
  });

  it("성공 알림은 잠시 뒤 사라지고 실패 알림은 남는다", () => {
    vi.useFakeTimers();
    render(<ActionStatusToast />);

    act(() => {
      shownIds.push(showActionStatus({ kind: "success", message: "done" }));
      shownIds.push(showActionStatus({ kind: "failure", message: "broken" }));
    });
    act(() => {
      vi.advanceTimersByTime(10_000);
    });

    expect(screen.queryByText("done")).toBeNull();
    expect(screen.getByText("broken")).toBeTruthy();
  });
});
