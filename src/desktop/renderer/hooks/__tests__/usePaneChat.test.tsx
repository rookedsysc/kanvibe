import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PaneChatActionResult, PaneChatSnapshot } from "@/desktop/shared/paneChat";
const api = vi.hoisted(() => ({ getPaneChatSnapshot: vi.fn(), sendPaneChatPrompt: vi.fn(), openPaneChatModelPicker: vi.fn() }));
vi.mock("@/desktop/renderer/actions/paneChat", () => api);
vi.mock("../useIsWindowActive", () => ({ useIsWindowActive: () => true }));
import { usePaneChat } from "../usePaneChat";

const snapshot = (token: string) => ({ taskId: "task", paneId: "%A", targetToken: token, canSend: true, transcript: { status: "available", messages: [] } }) as unknown as PaneChatSnapshot;
beforeEach(() => { vi.resetAllMocks(); api.getPaneChatSnapshot.mockResolvedValue(snapshot("X")); });
afterEach(cleanup);

it("returns a successful send to its captured draft after the displayed conversation changes", async () => {
  let accept!: (value: PaneChatActionResult) => void;
  api.sendPaneChatPrompt.mockReturnValue(new Promise<PaneChatActionResult>((resolve) => { accept = resolve; }));
  const view = renderHook(() => usePaneChat("task", true, "%A"));
  await waitFor(() => expect(view.result.current.snapshot?.targetToken).toBe("X"));
  let sending!: Promise<boolean>;
  act(() => { sending = view.result.current.send("For X", [], "one-operation"); });
  api.getPaneChatSnapshot.mockResolvedValue(snapshot("Y"));
  await act(async () => { await view.result.current.refresh(); });
  expect(view.result.current.snapshot?.targetToken).toBe("Y");
  let accepted = false;
  await act(async () => { accept({ success: true }); accepted = await sending; });
  expect(accepted).toBe(true);
  expect(view.result.current.snapshot?.targetToken).toBe("Y");
  expect(view.result.current.actionError).toBeNull();
  expect(api.sendPaneChatPrompt).toHaveBeenCalledOnce();
});

it("keeps discovering a CLI that starts after several unavailable snapshots", async () => {
  vi.useFakeTimers();
  const unavailable = { ...snapshot(""), canSend: false, transcript: { status: "unavailable", reason: "exact-session-unavailable", messages: [] } };
  api.getPaneChatSnapshot.mockResolvedValue(unavailable);
  const view = renderHook(() => usePaneChat("task", true, "%A"));
  try {
    await act(async () => {});
    await act(async () => { await vi.advanceTimersByTimeAsync(3_000); });
    expect(api.getPaneChatSnapshot).toHaveBeenCalledTimes(3);
    api.getPaneChatSnapshot.mockResolvedValue(snapshot("new-cli"));
    await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });
    expect(view.result.current.snapshot?.targetToken).toBe("new-cli");
  } finally { view.unmount(); vi.useRealTimers(); }
});
