import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { usePolledValue } from "../usePolledValue";

const windowState = vi.hoisted(() => ({ isActive: true }));
vi.mock("../useIsWindowActive", () => ({ useIsWindowActive: () => windowState.isActive }));
afterEach(() => { cleanup(); vi.useRealTimers(); windowState.isActive = true; });

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

describe("usePolledValue query ownership", () => {
  it("hides the previous query while the next query is pending or rejected", async () => {
    const pending = deferred<string>();
    const first = vi.fn().mockResolvedValue("A");
    const second = vi.fn(() => pending.promise);
    const { result, rerender } = renderHook(({ read }) => usePolledValue(read, "empty", 1000, true), { initialProps: { read: first } });
    await waitFor(() => expect(result.current).toBe("A"));
    rerender({ read: second });
    expect(result.current).toBe("empty");
    await act(async () => pending.reject(new Error("unavailable")));
    expect(result.current).toBe("empty");
  });

  it("retains the same query's last success when a later poll fails", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const read = vi.fn().mockResolvedValueOnce("A").mockRejectedValue(new Error("unavailable"));
    const { result } = renderHook(() => usePolledValue(read, "empty", 1000, true));
    await waitFor(() => expect(result.current).toBe("A"));
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(read).toHaveBeenCalledTimes(2);
    expect(result.current).toBe("A");
  });

  it("does not publish a cancelled query response over the current query", async () => {
    const old = deferred<string>();
    const first = vi.fn(() => old.promise);
    const second = vi.fn().mockResolvedValue("B");
    const { result, rerender } = renderHook(({ read }) => usePolledValue(read, "empty", 1000, true), { initialProps: { read: first } });
    rerender({ read: second });
    await waitFor(() => expect(result.current).toBe("B"));
    await act(async () => old.resolve("A"));
    expect(result.current).toBe("B");
  });
});

describe("usePolledValue 창 활성 전환", () => {
  it("이미 읽은 창이 가려질 때는 다시 읽지 않고, 다시 앞으로 나오면 밀린 변화를 한 번 읽는다", async () => {
    const read = vi.fn().mockResolvedValueOnce("A").mockResolvedValueOnce("B");
    const { result, rerender } = renderHook(() => usePolledValue(read, "empty", 60_000, true));
    await waitFor(() => expect(result.current).toBe("A"));

    // 창을 떠나는 순간마다 읽으면 원격 조회가 창 전환 횟수만큼 더 나간다.
    windowState.isActive = false;
    rerender();
    await act(async () => { await Promise.resolve(); });
    expect(read).toHaveBeenCalledTimes(1);
    expect(result.current).toBe("A");

    windowState.isActive = true;
    rerender();
    await waitFor(() => expect(result.current).toBe("B"));
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("가려진 채로 열린 창도 첫 조회는 한 번 한다", async () => {
    windowState.isActive = false;
    const read = vi.fn().mockResolvedValue("A");
    const { result } = renderHook(() => usePolledValue(read, "empty", 1000, true));

    await waitFor(() => expect(result.current).toBe("A"));
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("느린 조회가 끝나기 전에 창 활성 여부가 바뀌어도 조회를 겹쳐 보내지 않고 그 결과를 그대로 쓴다", async () => {
    const slowRead = deferred<string>();
    const read = vi.fn(() => slowRead.promise);
    const { result, rerender } = renderHook(() => usePolledValue(read, "empty", 60_000, true));
    await waitFor(() => expect(read).toHaveBeenCalledTimes(1));

    windowState.isActive = false;
    rerender();
    windowState.isActive = true;
    rerender();
    await act(async () => { await Promise.resolve(); });
    expect(read).toHaveBeenCalledTimes(1);

    await act(async () => slowRead.resolve("A"));
    expect(result.current).toBe("A");
  });
});
