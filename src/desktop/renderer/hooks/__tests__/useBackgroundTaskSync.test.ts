import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useBackgroundTaskSync } from "../useBackgroundTaskSync";
import type { BackgroundTaskSyncStatus } from "@/desktop/shared/backgroundTaskSync";
import type { BoardEventPayload } from "@/lib/boardNotifier";

const mocks = vi.hoisted(() => ({ get: vi.fn(), start: vi.fn(), dispose: vi.fn() }));
vi.mock("@/desktop/renderer/actions/backgroundTaskSync", () => ({
  getBackgroundTaskSyncStatus: mocks.get,
  startBackgroundTaskSyncNow: mocks.start,
}));
const idle: BackgroundTaskSyncStatus = {
  phase: "idle", isRunning: false, completedStages: 0,
  reviewNeeded: false, boardUpdated: false, error: null,
};
const running: BackgroundTaskSyncStatus = { ...idle, phase: "worktrees", isRunning: true };
let receive: (event: BoardEventPayload) => void;

describe("useBackgroundTaskSync", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.get.mockResolvedValue(idle);
    mocks.start.mockResolvedValue(running);
    window.kanvibeDesktop = {
      onBoardEvent: (listener: typeof receive) => { receive = listener; return mocks.dispose; },
    } as unknown as NonNullable<typeof window.kanvibeDesktop>;
  });

  it("ignores an old snapshot after another client starts syncing", async () => {
    let resolve!: (snapshot: BackgroundTaskSyncStatus) => void;
    mocks.get.mockReturnValue(new Promise((done) => { resolve = done; }));
    const { result, unmount } = renderHook(useBackgroundTaskSync);
    act(() => receive({ type: "background-sync-status-changed", status: running }));
    await act(async () => resolve(idle));
    expect(result.current.status).toEqual(running);
    await act(async () => result.current.start());
    expect(mocks.start).not.toHaveBeenCalled();
    unmount();
    expect(mocks.dispose).toHaveBeenCalledOnce();
  });

  it("blocks rapid starts and keeps completion newer than the start response", async () => {
    let resolve!: (snapshot: BackgroundTaskSyncStatus) => void;
    mocks.start.mockReturnValue(new Promise((done) => { resolve = done; }));
    const { result } = renderHook(useBackgroundTaskSync);
    await waitFor(() => expect(result.current.status).toEqual(idle));
    let start!: Promise<void>;
    act(() => { start = result.current.start(); void result.current.start(); });
    expect(mocks.start).toHaveBeenCalledOnce();
    const completed = { ...idle, phase: "completed" as const, completedStages: 3 };
    act(() => receive({ type: "background-sync-status-changed", status: completed }));
    await act(async () => { resolve(running); await start; });
    expect(result.current.status).toEqual(completed);
    expect(result.current.isStarting).toBe(false);
  });

  it("keeps failures visible and permits retry", async () => {
    mocks.start.mockRejectedValueOnce(new Error("IPC unavailable"));
    const { result } = renderHook(useBackgroundTaskSync);
    await waitFor(() => expect(result.current.status).toEqual(idle));
    await act(async () => result.current.start());
    expect(result.current.error).toBe("IPC unavailable");
    await act(async () => result.current.start());
    expect(result.current.error).toBeNull();
    expect(result.current.status).toEqual(running);
  });

  it("shares a command start failure with the notification consumer", async () => {
    mocks.start.mockRejectedValueOnce(new Error("IPC unavailable"));
    const command = renderHook(useBackgroundTaskSync);
    const notifications = renderHook(useBackgroundTaskSync);
    await waitFor(() => expect(command.result.current.status).toEqual(idle));
    await act(async () => command.result.current.start());
    expect(notifications.result.current.error).toBe("IPC unavailable");
  });
});
