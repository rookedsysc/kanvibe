import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useAiUsage } from "@/desktop/renderer/hooks/useAiUsage";
import { AI_USAGE_REFRESH_INTERVAL_MS } from "@/lib/aiUsage/shared";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

const { mockGetAiUsageSnapshot, mockGetCachedAiUsageSnapshot, windowState } = vi.hoisted(() => ({
  mockGetAiUsageSnapshot: vi.fn(),
  mockGetCachedAiUsageSnapshot: vi.fn(),
  windowState: { isActive: true },
}));

vi.mock("@/desktop/renderer/hooks/useIsWindowActive", () => ({
  useIsWindowActive: () => windowState.isActive,
}));

vi.mock("@/desktop/renderer/actions/aiUsage", () => ({
  getAiUsageSnapshot: mockGetAiUsageSnapshot,
  getCachedAiUsageSnapshot: mockGetCachedAiUsageSnapshot,
}));

function createSnapshot(usedPercent: number): AiUsageSnapshot {
  return {
    fetchedAt: "2026-08-10T06:00:00.000Z",
    accounts: [
      {
        provider: "claude",
        accountId: "account",
        label: "me@example.com",
        status: "ok",
        planName: null,
        windows: [{ kind: "session", modelName: null, usedPercent, resetsAt: null }],
        reason: null,
        fetchedAt: "2026-08-10T06:00:00.000Z",
      },
    ],
  };
}

function createPendingSnapshot(): { promise: Promise<AiUsageSnapshot>; resolve: () => void } {
  let resolveSnapshot: (snapshot: AiUsageSnapshot) => void = () => {};
  const promise = new Promise<AiUsageSnapshot>((resolve) => {
    resolveSnapshot = resolve;
  });

  return { promise, resolve: () => resolveSnapshot(createSnapshot(80)) };
}

/** `createSnapshot`이 조회 시각으로 싣는 값. 예약 시각은 이 시각에서 주기만큼 뒤다 */
const SNAPSHOT_FETCHED_AT = "2026-08-10T06:00:00.000Z";

async function advanceTime(milliseconds: number): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(milliseconds);
  });
}

describe("useAiUsage", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.resetAllMocks();
    windowState.isActive = true;
  });

  function startAtSnapshotTime(): void {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(SNAPSHOT_FETCHED_AT));
    mockGetCachedAiUsageSnapshot.mockResolvedValue(null);
    mockGetAiUsageSnapshot.mockResolvedValue(createSnapshot(22));
  }

  it("새 조회를 기다리는 동안 저장된 결과를 먼저 보여준다", async () => {
    mockGetCachedAiUsageSnapshot.mockResolvedValue(createSnapshot(22));
    const pendingFetch = createPendingSnapshot();
    mockGetAiUsageSnapshot.mockReturnValue(pendingFetch.promise);

    const { result } = renderHook(() => useAiUsage(true));

    await waitFor(() => {
      expect(result.current.snapshot?.accounts[0].windows[0].usedPercent).toBe(22);
    });
    expect(result.current.isRefreshing).toBe(true);
    expect(result.current.isLoading).toBe(false);

    await act(async () => {
      pendingFetch.resolve();
    });

    expect(result.current.snapshot?.accounts[0].windows[0].usedPercent).toBe(80);
    expect(result.current.isRefreshing).toBe(false);
  });

  it("보여줄 캐시가 없을 때만 불러오는 중으로 표시한다", async () => {
    mockGetCachedAiUsageSnapshot.mockResolvedValue(null);
    const pendingFetch = createPendingSnapshot();
    mockGetAiUsageSnapshot.mockReturnValue(pendingFetch.promise);

    const { result } = renderHook(() => useAiUsage(true));

    await waitFor(() => {
      expect(result.current.isLoading).toBe(true);
    });

    await act(async () => {
      pendingFetch.resolve();
    });

    expect(result.current.isLoading).toBe(false);
  });

  it("캐시가 늦게 도착해도 이미 받은 새 결과를 덮어쓰지 않는다", async () => {
    let resolveCache: (snapshot: AiUsageSnapshot) => void = () => {};
    mockGetCachedAiUsageSnapshot.mockReturnValue(new Promise<AiUsageSnapshot>((resolve) => {
      resolveCache = resolve;
    }));
    mockGetAiUsageSnapshot.mockResolvedValue(createSnapshot(80));

    const { result } = renderHook(() => useAiUsage(true));

    await waitFor(() => {
      expect(result.current.snapshot?.accounts[0].windows[0].usedPercent).toBe(80);
    });

    await act(async () => {
      resolveCache(createSnapshot(22));
    });

    expect(result.current.snapshot?.accounts[0].windows[0].usedPercent).toBe(80);
  });

  it("조회가 실패해도 보여주던 캐시는 지우지 않는다", async () => {
    mockGetCachedAiUsageSnapshot.mockResolvedValue(createSnapshot(22));
    mockGetAiUsageSnapshot.mockRejectedValue(new Error("ipc down"));

    const { result } = renderHook(() => useAiUsage(true));

    await waitFor(() => {
      expect(result.current.hasFailed).toBe(true);
    });
    expect(result.current.snapshot?.accounts[0].windows[0].usedPercent).toBe(22);
  });

  it("패널을 열지 않으면 아무것도 부르지 않는다", () => {
    renderHook(() => useAiUsage(false));

    expect(mockGetCachedAiUsageSnapshot).not.toHaveBeenCalled();
    expect(mockGetAiUsageSnapshot).not.toHaveBeenCalled();
  });

  it("조회 중 들어온 계정 변경 갱신은 현재 조회 뒤 한 번 더 읽는다", async () => {
    mockGetCachedAiUsageSnapshot.mockResolvedValue(null);
    const pending = createPendingSnapshot();
    mockGetAiUsageSnapshot.mockReturnValueOnce(pending.promise).mockResolvedValue(createSnapshot(10));
    const { result } = renderHook(() => useAiUsage(true));
    await waitFor(() => expect(result.current.isRefreshing).toBe(true));
    act(() => { result.current.refresh(); result.current.refresh(); });
    await act(async () => { pending.resolve(); });
    await waitFor(() => expect(result.current.snapshot?.accounts[0].windows[0].usedPercent).toBe(10));
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(2);
    expect(mockGetAiUsageSnapshot.mock.calls[1][0]).toEqual({ canReuseRecentSnapshot: false });
    expect(result.current.isRefreshing).toBe(false);
  });

  it("새로고침은 캐시 주기와 무관하게 다시 조회한다", async () => {
    mockGetCachedAiUsageSnapshot.mockResolvedValue(null);
    mockGetAiUsageSnapshot.mockResolvedValue(createSnapshot(22));

    const { result } = renderHook(() => useAiUsage(true));
    await waitFor(() => {
      expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(1);
    });

    await act(async () => {
      result.current.refresh();
    });

    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(2);
  });

  it("스스로 시작한 조회는 최근 결과를 나눠 쓰고 새로고침은 새로 조회한다", async () => {
    startAtSnapshotTime();
    const { result } = renderHook(() => useAiUsage(true));
    await advanceTime(0);

    await act(async () => {
      result.current.refresh();
    });

    expect(mockGetAiUsageSnapshot.mock.calls[0][0]).toEqual({ canReuseRecentSnapshot: true });
    expect(mockGetAiUsageSnapshot.mock.calls[1][0]).toEqual({ canReuseRecentSnapshot: false });
  });

  it("화면이 열려 있고 창이 활성인 동안 5분마다 다시 조회한다", async () => {
    startAtSnapshotTime();
    renderHook(() => useAiUsage(true));
    await advanceTime(0);
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(1);

    await advanceTime(AI_USAGE_REFRESH_INTERVAL_MS - 1);
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(1);

    await advanceTime(1);
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(2);
    expect(mockGetAiUsageSnapshot.mock.calls[1][0]).toEqual({ canReuseRecentSnapshot: true });
  });

  it("창이 뒤에 있는 동안은 조회하지 않다가 돌아오는 순간 밀린 조회를 한다", async () => {
    startAtSnapshotTime();
    const { rerender } = renderHook(() => useAiUsage(true));
    await advanceTime(0);

    windowState.isActive = false;
    rerender();
    await advanceTime(AI_USAGE_REFRESH_INTERVAL_MS + 60_000);
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(1);

    windowState.isActive = true;
    rerender();
    await advanceTime(0);
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(2);
  });

  it("화면을 닫으면 더 조회하지 않는다", async () => {
    startAtSnapshotTime();
    const { rerender } = renderHook(({ isOpen }) => useAiUsage(isOpen), { initialProps: { isOpen: true } });
    await advanceTime(0);

    rerender({ isOpen: false });
    await advanceTime(AI_USAGE_REFRESH_INTERVAL_MS * 2);

    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(1);
  });

  it("이미 주기를 넘긴 결과를 받아도 곧바로 되풀이하지 않고 다음 주기에 다시 조회한다", async () => {
    startAtSnapshotTime();
    vi.setSystemTime(new Date(Date.parse(SNAPSHOT_FETCHED_AT) + AI_USAGE_REFRESH_INTERVAL_MS * 3));
    renderHook(() => useAiUsage(true));
    await advanceTime(0);

    await advanceTime(AI_USAGE_REFRESH_INTERVAL_MS - 1);
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(1);

    await advanceTime(1);
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(2);
  });

  it("조회가 실패해도 곧바로 되풀이하지 않고 다음 주기에 다시 시도한다", async () => {
    startAtSnapshotTime();
    mockGetAiUsageSnapshot.mockRejectedValue(new Error("ipc down"));
    renderHook(() => useAiUsage(true));
    await advanceTime(0);

    await advanceTime(AI_USAGE_REFRESH_INTERVAL_MS - 1);
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(1);

    await advanceTime(1);
    expect(mockGetAiUsageSnapshot).toHaveBeenCalledTimes(2);
  });
});
