import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useAiUsage } from "@/desktop/renderer/hooks/useAiUsage";
import type { AiUsageSnapshot } from "@/lib/aiUsage/types";

const { mockGetAiUsageSnapshot, mockGetCachedAiUsageSnapshot } = vi.hoisted(() => ({
  mockGetAiUsageSnapshot: vi.fn(),
  mockGetCachedAiUsageSnapshot: vi.fn(),
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

describe("useAiUsage", () => {
  afterEach(() => {
    vi.resetAllMocks();
  });

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
});
