import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useTrackedAction } from "@/desktop/renderer/hooks/useTrackedAction";

describe("useTrackedAction", () => {
  it("진행 중에 다시 실행하면 작업을 한 번만 보내고 진행 대상 인자를 알려 준다", async () => {
    let resolveAction: () => void = () => {};
    const action = vi.fn((_target: string) => new Promise<void>((resolve) => {
      resolveAction = resolve;
    }));
    const { result } = renderHook(() => useTrackedAction(action));

    let firstRun: Promise<void> = Promise.resolve();
    act(() => {
      firstRun = result.current.run("review");
      void result.current.run("todo");
    });

    expect(action).toHaveBeenCalledTimes(1);
    expect(result.current.isPending).toBe(true);
    expect(result.current.pendingArgs).toEqual(["review"]);

    await act(async () => {
      resolveAction();
      await firstRun;
    });

    expect(result.current.isPending).toBe(false);
    expect(result.current.pendingArgs).toBeNull();
  });

  it("실패하면 사유를 보관하고 다시 던지지 않으며 retry는 마지막 인자로 다시 실행한다", async () => {
    const action = vi.fn()
      .mockRejectedValueOnce(new Error("ipc closed"))
      .mockResolvedValueOnce(undefined);
    const { result } = renderHook(() => useTrackedAction(action));

    await act(async () => {
      await result.current.run("review", 3);
    });

    expect(result.current.error).toBe("ipc closed");

    await act(async () => {
      await result.current.retry();
    });

    expect(action).toHaveBeenLastCalledWith("review", 3);
    expect(result.current.error).toBeNull();
  });

  it("clearError는 보관한 실패 사유를 지운다", async () => {
    const action = vi.fn().mockRejectedValueOnce(new Error("boom"));
    const { result } = renderHook(() => useTrackedAction(action));

    await act(async () => {
      await result.current.run();
    });
    act(() => {
      result.current.clearError();
    });

    expect(result.current.error).toBeNull();
  });
});
