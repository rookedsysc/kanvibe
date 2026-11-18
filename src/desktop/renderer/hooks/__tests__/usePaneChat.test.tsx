import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

describe("보낸 메시지와 대화 기록 맞추기", () => {
  /** 메시지를 보내 "전송됨" 말풍선이 남은 상태에서, CLI가 그 메시지를 기록한 대화를 다시 읽는다 */
  async function sendThenRecord(sentText: string, recordedText: string) {
    api.sendPaneChatPrompt.mockResolvedValue({ success: true });
    const view = renderHook(() => usePaneChat("task", true, "%A"));
    await waitFor(() => expect(view.result.current.snapshot?.targetToken).toBe("X"));
    await act(async () => { await view.result.current.send(sentText, sentText ? [] : ["attached-file"], "one-operation"); });
    expect(view.result.current.pendingMessages).toHaveLength(1);
    expect(view.result.current.isAgentWorking).toBe(true);
    const recorded = { role: "user", timestamp: "2026-11-17T12:00:00Z", text: recordedText, fullText: recordedText, isTruncated: false };
    api.getPaneChatSnapshot.mockResolvedValue({ ...snapshot("X"), transcript: { status: "available", messages: [recorded] } });
    await act(async () => { await view.result.current.refresh(); });
    return view;
  }

  it.each([
    ["끝의 줄바꿈과 공백을 뗀", "Ends with a newline  \n", "Ends with a newline"],
    ["탭을 공백으로 바꾼", "tab\there\n\tindented", "tab    here\n    indented"],
    ["첨부한 파일의 언급을 뒤에 붙인", "See the\tattached file", "See the    attached file\n\n@/attachments/note.txt"],
    ["글 없이 보낸 파일의 언급만 남긴", "", "@/attachments/note.txt"],
  ])("CLI가 %s 기록도 보낸 메시지의 기록으로 알아보고, 말풍선과 작업 중 표시를 걷는다", async (_recordShape, sentText, recordedText) => {
    const view = await sendThenRecord(sentText, recordedText);

    await waitFor(() => expect(view.result.current.pendingMessages).toHaveLength(0));
    expect(view.result.current.isAgentWorking).toBe(false);
  });

  it("보낸 글로 시작하지만 다른 글이 이어지는 기록은 보낸 메시지의 기록으로 치지 않는다", async () => {
    const view = await sendThenRecord("Deploy now", "Deploy now please");

    expect(view.result.current.pendingMessages).toHaveLength(1);
    expect(view.result.current.isAgentWorking).toBe(true);
  });
});
