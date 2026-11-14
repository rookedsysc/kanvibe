import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PaneChatPushEvent, PaneChatSnapshot } from "@/desktop/shared/paneChat";

const mocks = vi.hoisted(() => ({
  readPaneChat: vi.fn(),
  getPaneChatAgentProvider: vi.fn(),
  isActionInProgress: vi.fn(() => false),
  watch: vi.fn(),
  fileListeners: [] as (() => void)[],
  closedFileWatchers: 0,
}));

vi.mock("../paneChatService", () => ({ readPaneChat: mocks.readPaneChat, getPaneChatAgentProvider: mocks.getPaneChatAgentProvider, isPaneChatActionInProgress: mocks.isActionInProgress }));
vi.mock("node:fs", () => ({ watch: mocks.watch }));

const message = (text: string) => ({ role: "assistant" as const, timestamp: null, text, fullText: text, isTruncated: false });

/** texts는 오래된 것부터 적는다. 실제 스냅샷은 최신순이라 뒤집어 담는다 */
function snapshotOf(texts: string[], overrides: Partial<PaneChatSnapshot> = {}): PaneChatSnapshot {
  return {
    taskId: "task-1", paneId: "%1", sessionType: null, provider: "claude", targetToken: "token", canSend: true, inputState: "ready", screen: "화면 A", reason: null,
    transcript: { status: "available", reason: null, sessionId: "session-a", messages: texts.map(message).reverse(), nextCursor: null },
    modelPicker: { supported: true, mode: "native-cli" },
    ...overrides,
  };
}

function serveTranscript(texts: string[], overrides: Partial<PaneChatSnapshot> = {}, localTranscriptFile: string | null = null) {
  mocks.readPaneChat.mockResolvedValue({ snapshot: snapshotOf(texts, overrides), localTranscriptFile });
}

const chatTarget = { taskId: "task-1", paneId: "%1", detail: "chat" as const, wantsScreen: false };
const agentTarget = { taskId: "task-1", paneId: "%1", detail: "agent" as const, wantsScreen: false };
const rowTexts = (events: PaneChatPushEvent[]) => events.flatMap((event) => event.type === "pane-chat-rows" ? event.rows.map((row) => `${row.seq}:${row.message.fullText}`) : []);

async function loadService() {
  return import("../paneChatSubscriptionService");
}

describe("pane 채팅 구독", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.useFakeTimers();
    mocks.readPaneChat.mockReset();
    mocks.getPaneChatAgentProvider.mockReset();
    mocks.isActionInProgress.mockReset().mockReturnValue(false);
    mocks.fileListeners.length = 0;
    mocks.closedFileWatchers = 0;
    mocks.watch.mockReset().mockImplementation((_file: string, listener: () => void) => {
      mocks.fileListeners.push(listener);
      return { on: vi.fn(), close: () => { mocks.closedFileWatchers += 1; } };
    });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("구독하면 곧바로 읽어 상태를 보내고, 이후 새로 붙은 줄만 순번과 함께 보낸다", async () => {
    const service = await loadService();
    const events: PaneChatPushEvent[] = [];
    serveTranscript(["a", "b"]);
    service.setPaneChatSubscriptions({}, [chatTarget], (event) => events.push(event));
    await vi.advanceTimersByTimeAsync(0);
    expect(events.filter((event) => event.type === "pane-chat-state")).toHaveLength(1);
    expect(rowTexts(events)).toEqual(["1:a", "2:b"]);

    events.length = 0;
    serveTranscript(["a", "b", "c"]);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(rowTexts(events)).toEqual(["3:c"]);
    expect(events.filter((event) => event.type === "pane-chat-state")).toHaveLength(0);
  });

  it("마지막 구독자가 나가면 읽기와 파일 감시를 멈춘다", async () => {
    const service = await loadService();
    const first = {};
    const second = {};
    serveTranscript(["a"], {}, "/home/user/.claude/projects/p/session.jsonl");
    service.setPaneChatSubscriptions(first, [chatTarget], () => {});
    service.setPaneChatSubscriptions(second, [chatTarget], () => {});
    await vi.advanceTimersByTimeAsync(0);
    expect(mocks.watch).toHaveBeenCalledTimes(1);

    service.clearPaneChatSubscriptions(first);
    const readsWithOneLeft = mocks.readPaneChat.mock.calls.length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(mocks.readPaneChat.mock.calls.length).toBeGreaterThan(readsWithOneLeft);
    expect(mocks.closedFileWatchers).toBe(0);

    service.setPaneChatSubscriptions(second, [], () => {});
    const readsAfterAllLeft = mocks.readPaneChat.mock.calls.length;
    await vi.advanceTimersByTimeAsync(5_000);
    expect(mocks.readPaneChat.mock.calls.length).toBe(readsAfterAllLeft);
    expect(mocks.closedFileWatchers).toBe(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("기록 파일이 연달아 바뀌면 첫 변화 뒤의 변화들을 한 번의 읽기로 묶는다", async () => {
    const service = await loadService();
    serveTranscript(["a"], {}, "/home/user/.claude/projects/p/session.jsonl");
    service.setPaneChatSubscriptions({}, [chatTarget], () => {});
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(200);
    const readsBeforeBurst = mocks.readPaneChat.mock.calls.length;

    for (let change = 0; change < 5; change++) {
      mocks.fileListeners[0]();
      await vi.advanceTimersByTimeAsync(10);
    }
    expect(mocks.readPaneChat.mock.calls.length).toBe(readsBeforeBurst + 1);
    await vi.advanceTimersByTimeAsync(60);
    expect(mocks.readPaneChat.mock.calls.length).toBe(readsBeforeBurst + 2);
  });

  it("원격 태스크처럼 감시할 파일이 없으면 주기적인 읽기만으로 새 줄을 보낸다", async () => {
    const service = await loadService();
    const events: PaneChatPushEvent[] = [];
    serveTranscript(["a"]);
    service.setPaneChatSubscriptions({}, [chatTarget], (event) => events.push(event));
    await vi.advanceTimersByTimeAsync(0);
    expect(mocks.watch).not.toHaveBeenCalled();
    events.length = 0;
    serveTranscript(["a", "b"]);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(rowTexts(events)).toEqual(["2:b"]);
  });

  it("화면은 요청한 구독자에게만 보내고, 요청하지 않은 구독자는 화면만 바뀌어도 아무것도 받지 않는다", async () => {
    const service = await loadService();
    const withScreen: PaneChatPushEvent[] = [];
    const withoutScreen: PaneChatPushEvent[] = [];
    serveTranscript(["a"]);
    service.setPaneChatSubscriptions({}, [{ ...chatTarget, wantsScreen: true }], (event) => withScreen.push(event));
    service.setPaneChatSubscriptions({}, [chatTarget], (event) => withoutScreen.push(event));
    await vi.advanceTimersByTimeAsync(0);
    const firstState = withoutScreen.find((event) => event.type === "pane-chat-state");
    expect(firstState?.type === "pane-chat-state" && "screen" in firstState.state).toBe(false);

    withScreen.length = 0;
    withoutScreen.length = 0;
    serveTranscript(["a"], { screen: "화면 B" });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(withScreen).toEqual([expect.objectContaining({ type: "pane-chat-state", state: expect.objectContaining({ screen: "화면 B" }) })]);
    expect(withoutScreen).toEqual([]);
  });

  it("입력 상태가 바뀌면 화면을 요청하지 않은 구독자에게도 상태를 보낸다", async () => {
    const service = await loadService();
    const events: PaneChatPushEvent[] = [];
    serveTranscript(["a"]);
    service.setPaneChatSubscriptions({}, [chatTarget], (event) => events.push(event));
    await vi.advanceTimersByTimeAsync(0);
    events.length = 0;
    serveTranscript(["a"], { inputState: "occupied" });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(events).toEqual([expect.objectContaining({ type: "pane-chat-state", state: expect.objectContaining({ inputState: "occupied" }) })]);
  });

  it("KanVibe가 pane에 입력을 치는 동안에는 읽지 않고, 다 친 뒤의 상태만 보낸다", async () => {
    const service = await loadService();
    const events: PaneChatPushEvent[] = [];
    serveTranscript(["a"]);
    service.setPaneChatSubscriptions({}, [chatTarget], (event) => events.push(event));
    await vi.advanceTimersByTimeAsync(0);
    events.length = 0;

    mocks.isActionInProgress.mockReturnValue(true);
    serveTranscript(["a"], { inputState: "occupied" });
    const readsBeforeAction = mocks.readPaneChat.mock.calls.length;
    await vi.advanceTimersByTimeAsync(3_000);
    expect(mocks.readPaneChat.mock.calls.length).toBe(readsBeforeAction);
    expect(events).toEqual([]);

    mocks.isActionInProgress.mockReturnValue(false);
    serveTranscript(["a", "b"]);
    await vi.advanceTimersByTimeAsync(200);
    expect(rowTexts(events)).toEqual(["2:b"]);
    expect(events.filter((event) => event.type === "pane-chat-state")).toEqual([]);
  });

  it("에이전트 실행 여부만 보는 구독자에게는 대화를 읽지 않고 이름이 바뀔 때만 알린다", async () => {
    const service = await loadService();
    const events: PaneChatPushEvent[] = [];
    mocks.getPaneChatAgentProvider.mockResolvedValue(null);
    service.setPaneChatSubscriptions({}, [agentTarget], (event) => events.push(event));
    await vi.advanceTimersByTimeAsync(0);
    expect(events).toEqual([{ type: "pane-chat-state", taskId: "task-1", paneId: "%1", state: { provider: null } }]);
    expect(mocks.readPaneChat).not.toHaveBeenCalled();

    events.length = 0;
    await vi.advanceTimersByTimeAsync(3_000);
    expect(events).toEqual([]);
    mocks.getPaneChatAgentProvider.mockResolvedValue("codex");
    await vi.advanceTimersByTimeAsync(3_000);
    expect(events).toEqual([{ type: "pane-chat-state", taskId: "task-1", paneId: "%1", state: { provider: "codex" } }]);
  });

  it("기록이 다른 세션으로 바뀌면 줄 대신 교체 신호만 보낸다", async () => {
    const service = await loadService();
    const events: PaneChatPushEvent[] = [];
    serveTranscript(["a", "b"]);
    service.setPaneChatSubscriptions({}, [chatTarget], (event) => events.push(event));
    await vi.advanceTimersByTimeAsync(0);
    events.length = 0;
    serveTranscript(["x"], { transcript: { status: "available", reason: null, sessionId: "session-b", messages: [message("x")], nextCursor: null } });
    await vi.advanceTimersByTimeAsync(1_000);
    expect(events.filter((event) => event.type !== "pane-chat-state")).toEqual([{ type: "pane-chat-replaced", taskId: "task-1", paneId: "%1", epoch: expect.any(String) }]);
  });

  it("구독자의 위치가 스냅샷의 끝과 같을 때만 최신이라고 답한다", async () => {
    const service = await loadService();
    serveTranscript(["a", "b"]);
    const snapshot = await service.getSequencedPaneChatSnapshot("task-1", "%1");
    expect(snapshot.transcript.seq).toBe(2);
    const epoch = snapshot.transcript.epoch!;
    const freshnessOf = (cursor: { epoch: string; seq: number } | undefined) =>
      service.setPaneChatSubscriptions({}, [{ ...chatTarget, cursor }], () => {})[0].freshness;
    expect(freshnessOf({ epoch, seq: 2 })).toBe("current");
    expect(freshnessOf({ epoch, seq: 1 })).toBe("behind");
    expect(freshnessOf({ epoch: "other", seq: 2 })).toBe("behind");
    expect(freshnessOf(undefined)).toBe("behind");
  });

  it("HTTP 조회가 먼저 발견한 새 줄도 구독자에게 빠짐없이 보낸다", async () => {
    const service = await loadService();
    const events: PaneChatPushEvent[] = [];
    serveTranscript(["a"]);
    service.setPaneChatSubscriptions({}, [chatTarget], (event) => events.push(event));
    await vi.advanceTimersByTimeAsync(0);
    events.length = 0;
    serveTranscript(["a", "b"]);
    await service.getSequencedPaneChatSnapshot("task-1", "%1");
    expect(rowTexts(events)).toEqual(["2:b"]);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(rowTexts(events)).toEqual(["2:b"]);
  });
});
