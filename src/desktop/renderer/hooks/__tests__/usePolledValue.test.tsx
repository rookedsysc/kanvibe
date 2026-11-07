import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { usePolledValue } from "../usePolledValue";

vi.mock("../useIsWindowActive", () => ({ useIsWindowActive: () => true }));
afterEach(() => { cleanup(); vi.useRealTimers(); });

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
