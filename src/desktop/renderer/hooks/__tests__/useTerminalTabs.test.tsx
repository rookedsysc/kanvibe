import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useTerminalTabs } from "../useTerminalTabs";
import { SessionType } from "@/entities/KanbanTask";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";

const { list, create, select, rename } = vi.hoisted(() => ({ list: vi.fn(), create: vi.fn(), select: vi.fn(), rename: vi.fn() }));
vi.mock("@/desktop/renderer/actions/terminalTabs", () => ({
  listTerminalTabs: list,
  createTerminalTab: create,
  selectTerminalTab: select,
  renameTerminalTab: rename,
}));
const windowState = vi.hoisted(() => ({ isActive: true }));
vi.mock("@/desktop/renderer/hooks/useIsWindowActive", () => ({ useIsWindowActive: () => windowState.isActive }));
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => { resolve = res; });
  return { promise, resolve };
}
const tab = (id: string): TerminalTab => ({ id, name: id, isActive: true, nativeIndex: 0 });
const options = { taskId: "task", sessionType: SessionType.TMUX, isRemote: false, isVisible: true };
afterEach(() => { cleanup(); vi.useRealTimers(); windowState.isActive = true; });
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

describe("useTerminalTabs mutation feedback", () => {
  const tabs = (activeId: string): TerminalTab[] => [
    { id: "a", name: "a", isActive: activeId === "a", nativeIndex: 0 },
    { id: "b", name: "b", isActive: activeId === "b", nativeIndex: 1 },
  ];

  it("탭을 추가하는 동안 진행 중인 명령을 알리고 같은 순간의 두 번째 추가는 보내지 않는다", async () => {
    const pending = deferred<{ ok: true }>();
    create.mockReturnValueOnce(pending.promise);
    list.mockResolvedValue({ ok: true, tabs: tabs("a") });
    const { result } = renderHook(() => useTerminalTabs(options));
    await waitFor(() => expect(result.current.isTabListLoaded).toBe(true));

    let firstCreate!: Promise<void>;
    act(() => {
      firstCreate = result.current.createTab();
      void result.current.createTab();
    });

    expect(create).toHaveBeenCalledTimes(1);
    expect(result.current.pendingAction).toEqual({ kind: "create", tabId: null });

    await act(async () => { pending.resolve({ ok: true }); await firstCreate; });
    expect(result.current.pendingAction).toBeNull();
  });

  it("이름 변경이 실패하면 사유를 남기고 다시 시도로 같은 이름 변경을 보낸다", async () => {
    list.mockResolvedValue({ ok: true, tabs: tabs("a") });
    rename.mockRejectedValueOnce(new Error("tmux gone")).mockResolvedValueOnce({ ok: true });
    const { result } = renderHook(() => useTerminalTabs(options));
    await waitFor(() => expect(result.current.isTabListLoaded).toBe(true));

    await act(async () => { await result.current.renameTab("a", "server"); });
    expect(result.current.actionError).toBe("tmux gone");

    await act(async () => { await result.current.retryAction(); });
    expect(rename).toHaveBeenLastCalledWith("task", "a", "server");
    expect(result.current.actionError).toBeNull();
  });

  it("선택이 실패하면 실제 탭 상태로 되돌린다", async () => {
    list.mockResolvedValue({ ok: true, tabs: tabs("a") });
    select.mockRejectedValueOnce(new Error("select failed"));
    const { result } = renderHook(() => useTerminalTabs(options));
    await waitFor(() => expect(result.current.activeTab?.id).toBe("a"));

    await act(async () => { await result.current.selectTab("b"); });

    expect(result.current.activeTab?.id).toBe("a");
    expect(result.current.actionError).toBe("select failed");
  });

  it("목록을 처음 받기 전과 받은 뒤를 구분한다", async () => {
    const pending = deferred<{ ok: true; tabs: TerminalTab[] }>();
    list.mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useTerminalTabs(options));

    expect(result.current.isTabListLoaded).toBe(false);
    await act(async () => { pending.resolve({ ok: true, tabs: [] }); });
    expect(result.current.isTabListLoaded).toBe(true);
  });
});

describe("useTerminalTabs 창 활성 전환", () => {
  const remoteOptions = { ...options, sessionType: SessionType.HERDR, isRemote: true };

  it("원격 task의 창이 가려질 때는 탭 목록을 다시 읽지 않고, 다시 앞으로 나오면 한 번 따라잡는다", async () => {
    list.mockResolvedValue({ ok: true, tabs: [tab("w1:t1")] });
    const { result, rerender } = renderHook(() => useTerminalTabs(remoteOptions));
    await waitFor(() => expect(result.current.isTabListLoaded).toBe(true));
    expect(list).toHaveBeenCalledTimes(1);

    // 창을 떠날 때마다 읽으면 원격 task는 창 전환마다 SSH 왕복이 하나씩 더 나간다.
    windowState.isActive = false;
    rerender();
    await act(async () => { await Promise.resolve(); });
    expect(list).toHaveBeenCalledTimes(1);

    windowState.isActive = true;
    rerender();
    await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
  });

  it("가려진 채로 열린 창도 탭 목록을 한 번은 읽는다", async () => {
    windowState.isActive = false;
    list.mockResolvedValue({ ok: true, tabs: [tab("w1:t1")] });
    const { result } = renderHook(() => useTerminalTabs(remoteOptions));

    await waitFor(() => expect(result.current.isTabListLoaded).toBe(true));
    expect(list).toHaveBeenCalledTimes(1);
  });
});
