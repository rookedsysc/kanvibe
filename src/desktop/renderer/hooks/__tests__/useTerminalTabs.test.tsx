import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useTerminalTabs } from "../useTerminalTabs";
import { SessionType } from "@/entities/KanbanTask";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";

const { list, create } = vi.hoisted(() => ({ list: vi.fn(), create: vi.fn() }));
vi.mock("@/desktop/renderer/actions/terminalTabs", () => ({ listTerminalTabs: list, createTerminalTab: create }));
vi.mock("@/desktop/renderer/hooks/useIsWindowActive", () => ({ useIsWindowActive: () => true }));
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => { resolve = res; });
  return { promise, resolve };
}
const tab = (id: string): TerminalTab => ({ id, name: id, isActive: true, nativeIndex: 0 });
const options = { taskId: "task", sessionType: SessionType.TMUX, isRemote: false, isVisible: true };
afterEach(() => { cleanup(); vi.useRealTimers(); });
beforeEach(() => { vi.clearAllMocks(); create.mockResolvedValue({ ok: true }); });

describe("useTerminalTabs refresh ownership", () => {
  it("keeps slow poll requests single-flight", async () => {
    vi.useFakeTimers();
    const pending = deferred<{ ok: true; tabs: TerminalTab[] }>();
    list.mockReturnValue(pending.promise);
    renderHook(() => useTerminalTabs(options));
    await act(async () => { vi.advanceTimersByTime(3_000); });
    expect(list).toHaveBeenCalledTimes(1);
    await act(async () => { pending.resolve({ ok: true, tabs: [] }); });
    expect(list).toHaveBeenCalledTimes(1);
    await act(async () => { vi.advanceTimersByTime(1_000); });
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("coalesces mutations during a read into one final read", async () => {
    const pending = deferred<{ ok: true; tabs: TerminalTab[] }>();
    list.mockReturnValueOnce(pending.promise).mockResolvedValue({ ok: true, tabs: [tab("new")] });
    const { result } = renderHook(() => useTerminalTabs(options));
    let mutation!: Promise<void>;
    await act(async () => { mutation = result.current.createTab(); await Promise.resolve(); });
    expect(list).toHaveBeenCalledTimes(1);
    await act(async () => { pending.resolve({ ok: true, tabs: [] }); await mutation; });
    expect(list).toHaveBeenCalledTimes(2);
    expect(result.current.tabs.map((entry) => entry.id)).toEqual(["new"]);
  });

  it("ignores a previous task's late response", async () => {
    const pending = deferred<{ ok: true; tabs: TerminalTab[] }>();
    list.mockImplementation((taskId) => taskId === "task" ? pending.promise : Promise.resolve({ ok: true, tabs: [tab("second")] }));
    const { result, rerender } = renderHook(({ taskId }) => useTerminalTabs({ ...options, taskId }), { initialProps: { taskId: "task" } });
    rerender({ taskId: "second" });
    await waitFor(() => expect(result.current.tabs.map((entry) => entry.id)).toEqual(["second"]));
    await act(async () => { pending.resolve({ ok: true, tabs: [tab("old")] }); });
    expect(result.current.tabs.map((entry) => entry.id)).toEqual(["second"]);
  });
});
