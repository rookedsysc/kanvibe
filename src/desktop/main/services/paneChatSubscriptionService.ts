import { watch, type FSWatcher } from "node:fs";
import { advancePaneChatTimeline, isPaneChatCursorCurrent, readPaneChatTimelineSeq, type PaneChatTimeline, type PaneChatTimelineRow } from "@/lib/aiSessions/paneChatTimeline";
import type { PaneChatCursor, PaneChatPushEvent, PaneChatSnapshot, PaneChatSubscriptionFreshness, PaneChatSubscriptionTarget } from "@/desktop/shared/paneChat";
import { getPaneChatAgentProvider, isPaneChatActionInProgress, readPaneChat } from "./paneChatService";

/** 대화를 보는 구독자가 있는 pane을 다시 읽는 주기. 입력 상태와 CLI 화면은 파일이 아니라 터미널에서만 알 수 있다 */
const CHAT_READ_INTERVAL_MS = 1_000;
/** 에이전트가 돌고 있는지만 궁금한 구독자뿐인 pane을 다시 확인하는 주기 */
const AGENT_PROBE_INTERVAL_MS = 3_000;
/** 기록 파일이 연달아 바뀔 때 다시 읽는 횟수를 이 간격에 한 번으로 묶는다. 조용하다가 온 첫 변화는 바로 읽는다 */
const READ_COALESCE_WINDOW_MS = 60;
/** KanVibe가 pane에 입력을 치는 동안 읽기를 미뤘다가 다시 확인하는 간격 */
const ACTION_SETTLE_RETRY_MS = 200;
const MAX_REMEMBERED_TIMELINES = 256;

interface PaneSubscriber {
  target: PaneChatSubscriptionTarget;
  send: (event: PaneChatPushEvent) => void;
  /** 마지막으로 보낸 상태. 같은 상태를 되풀이해 보내지 않으려고 직렬화한 값으로 견준다 */
  lastSentState: string | null;
}

interface PaneWatcher {
  taskId: string;
  paneId: string;
  subscribers: Set<PaneSubscriber>;
  readTimer: NodeJS.Timeout | null;
  lastReadAt: number;
  isReading: boolean;
  /** 읽는 도중에 다시 읽으라는 요청이 왔는지 */
  isReadRequested: boolean;
  transcriptFile: string | null;
  transcriptFileWatcher: FSWatcher | null;
}

const watchers = new Map<string, PaneWatcher>();
const subscribersByOwner = new Map<object, Map<string, PaneSubscriber>>();
/** 구독자가 떠난 뒤에도 순번을 기억해야 돌아온 구독자가 자기 위치가 최신인지 물을 수 있다 */
const timelines = new Map<string, PaneChatTimeline>();

function paneKey(taskId: string, paneId: string): string {
  return JSON.stringify([taskId, paneId]);
}

function rememberTimeline(key: string, timeline: PaneChatTimeline): void {
  timelines.delete(key);
  timelines.set(key, timeline);
  if (timelines.size > MAX_REMEMBERED_TIMELINES) {
    const oldestIdleKey = [...timelines.keys()].find((candidate) => !watchers.has(candidate));
    if (oldestIdleKey) timelines.delete(oldestIdleKey);
  }
}

/**
 * 읽어 온 스냅샷의 대화에 순번을 매기고, 그 pane을 보는 구독자에게 달라진 줄을 내보낸다.
 *
 * HTTP 조회와 감시가 같은 순번을 쓰도록 둘 다 이 함수를 지난다. 한쪽만 순번을 올리면 다른 쪽 구독자가 그 줄을 놓친다.
 */
function sequenceSnapshot(snapshot: PaneChatSnapshot): PaneChatSnapshot {
  if (!snapshot.paneId || snapshot.transcript.status !== "available") return snapshot;
  const key = paneKey(snapshot.taskId, snapshot.paneId);
  const oldestFirstMessages = [...snapshot.transcript.messages].reverse();
  const advance = advancePaneChatTimeline(timelines.get(key) ?? null, snapshot.transcript.sessionId, oldestFirstMessages);
  rememberTimeline(key, advance.timeline);

  const watcher = watchers.get(key);
  if (watcher) {
    if (advance.isReplaced) {
      emitToChatSubscribers(watcher, { type: "pane-chat-replaced", taskId: watcher.taskId, paneId: watcher.paneId, epoch: advance.timeline.epoch });
    } else if (advance.changedRows.length > 0) {
      emitRows(watcher, advance.timeline.epoch, advance.changedRows);
    }
  }
  return { ...snapshot, transcript: { ...snapshot.transcript, epoch: advance.timeline.epoch, seq: readPaneChatTimelineSeq(advance.timeline) } };
}

function emitToChatSubscribers(watcher: PaneWatcher, event: PaneChatPushEvent): void {
  for (const subscriber of watcher.subscribers) {
    if (subscriber.target.detail === "chat") subscriber.send(event);
  }
}

function emitRows(watcher: PaneWatcher, epoch: string, rows: PaneChatTimelineRow[]): void {
  emitToChatSubscribers(watcher, { type: "pane-chat-rows", taskId: watcher.taskId, paneId: watcher.paneId, epoch, rows });
}

/** 구독자가 요청한 만큼만 상태에 담는다. 화면은 실제로 보일 때만 보내야 작업 중인 CLI의 화면 변화가 계속 전송되지 않는다 */
function buildSubscriberState(subscriber: PaneSubscriber, snapshot: Pick<PaneChatSnapshot, "provider"> | PaneChatSnapshot): PaneChatPushEvent & { type: "pane-chat-state" } {
  const { taskId, paneId, detail, wantsScreen } = subscriber.target;
  // 대화를 보는 구독자가 없는 pane은 에이전트 이름만 확인하므로 스냅샷 전체가 없다
  if (detail === "agent" || !("transcript" in snapshot)) return { type: "pane-chat-state", taskId, paneId, state: { provider: snapshot.provider } };
  const { transcript, screen, ...rest } = snapshot;
  return {
    type: "pane-chat-state",
    taskId,
    paneId,
    state: {
      ...rest,
      ...(wantsScreen ? { screen } : {}),
      transcript: { status: transcript.status, reason: transcript.reason, sessionId: transcript.sessionId, nextCursor: transcript.nextCursor },
    },
  };
}

function emitChangedState(watcher: PaneWatcher, snapshot: Pick<PaneChatSnapshot, "provider"> | PaneChatSnapshot): void {
  for (const subscriber of watcher.subscribers) {
    const event = buildSubscriberState(subscriber, snapshot);
    const serializedState = JSON.stringify(event.state);
    if (serializedState === subscriber.lastSentState) continue;
    subscriber.lastSentState = serializedState;
    subscriber.send(event);
  }
}

function hasChatSubscriber(watcher: PaneWatcher): boolean {
  return [...watcher.subscribers].some((subscriber) => subscriber.target.detail === "chat");
}

function scheduleRead(watcher: PaneWatcher, delayMs: number): void {
  if (watcher.readTimer) clearTimeout(watcher.readTimer);
  watcher.readTimer = setTimeout(() => {
    watcher.readTimer = null;
    void readWatchedPane(watcher);
  }, delayMs);
  watcher.readTimer.unref?.();
}

/** 지금 읽어 달라는 요청. 직전에 읽었으면 묶음 간격 뒤로 미루고, 읽는 중이면 끝난 뒤 한 번 더 읽는다 */
function requestRead(watcher: PaneWatcher): void {
  if (watcher.isReading) {
    watcher.isReadRequested = true;
    return;
  }
  const elapsedMs = Date.now() - watcher.lastReadAt;
  if (elapsedMs >= READ_COALESCE_WINDOW_MS) {
    if (watcher.readTimer) clearTimeout(watcher.readTimer);
    watcher.readTimer = null;
    void readWatchedPane(watcher);
    return;
  }
  scheduleRead(watcher, READ_COALESCE_WINDOW_MS - elapsedMs);
}

function watchTranscriptFile(watcher: PaneWatcher, transcriptFile: string | null): void {
  if (watcher.transcriptFile === transcriptFile) return;
  watcher.transcriptFileWatcher?.close();
  watcher.transcriptFileWatcher = null;
  watcher.transcriptFile = transcriptFile;
  if (!transcriptFile) return;
  try {
    const fileWatcher = watch(transcriptFile, () => requestRead(watcher));
    // 감시가 죽어도 주기적인 읽기가 남아 있으므로 조용히 내려놓고, 다음 읽기에서 다시 건다
    fileWatcher.on("error", () => {
      fileWatcher.close();
      if (watcher.transcriptFileWatcher === fileWatcher) {
        watcher.transcriptFileWatcher = null;
        watcher.transcriptFile = null;
      }
    });
    watcher.transcriptFileWatcher = fileWatcher;
  } catch {
    watcher.transcriptFile = null;
  }
}

async function readWatchedPane(watcher: PaneWatcher): Promise<void> {
  if (watcher.isReading || watcher.subscribers.size === 0) return;
  // 프롬프트를 치는 도중의 입력란을 "CLI 입력 중"으로 내보내면 보낸 사람의 입력창이 잠깐 사라졌다 돌아온다.
  // 다 치고 난 상태만 의미가 있으므로 끝날 때까지 읽지 않는다
  if (isPaneChatActionInProgress(watcher.taskId)) {
    scheduleRead(watcher, ACTION_SETTLE_RETRY_MS);
    return;
  }
  watcher.isReading = true;
  watcher.isReadRequested = false;
  try {
    if (hasChatSubscriber(watcher)) {
      const reading = await readPaneChat(watcher.taskId, watcher.paneId);
      if (watcher.subscribers.size === 0) return;
      watchTranscriptFile(watcher, reading.localTranscriptFile);
      emitChangedState(watcher, sequenceSnapshot(reading.snapshot));
    } else {
      const provider = await getPaneChatAgentProvider(watcher.taskId, watcher.paneId);
      if (watcher.subscribers.size === 0) return;
      watchTranscriptFile(watcher, null);
      emitChangedState(watcher, { provider });
    }
  } catch {
    // 한 번의 읽기 실패로 구독을 끊지 않는다. 다음 주기에 다시 읽는다
  } finally {
    watcher.isReading = false;
    watcher.lastReadAt = Date.now();
    if (watcher.subscribers.size > 0) {
      if (watcher.isReadRequested) requestRead(watcher);
      else scheduleRead(watcher, hasChatSubscriber(watcher) ? CHAT_READ_INTERVAL_MS : AGENT_PROBE_INTERVAL_MS);
    }
  }
}

function stopWatcher(key: string, watcher: PaneWatcher): void {
  if (watcher.readTimer) clearTimeout(watcher.readTimer);
  watcher.readTimer = null;
  watcher.transcriptFileWatcher?.close();
  watcher.transcriptFileWatcher = null;
  watchers.delete(key);
}

function removeSubscriber(key: string, subscriber: PaneSubscriber): void {
  const watcher = watchers.get(key);
  if (!watcher) return;
  watcher.subscribers.delete(subscriber);
  if (watcher.subscribers.size === 0) stopWatcher(key, watcher);
}

function addSubscriber(key: string, subscriber: PaneSubscriber): PaneWatcher {
  let watcher = watchers.get(key);
  if (!watcher) {
    watcher = {
      taskId: subscriber.target.taskId,
      paneId: subscriber.target.paneId,
      subscribers: new Set(),
      readTimer: null,
      lastReadAt: 0,
      isReading: false,
      isReadRequested: false,
      transcriptFile: null,
      transcriptFileWatcher: null,
    };
    watchers.set(key, watcher);
  }
  watcher.subscribers.add(subscriber);
  return watcher;
}

/**
 * 구독자 하나가 보려는 pane 전체를 통째로 바꾼다. 빈 목록이면 전부 해제한다.
 *
 * 구독을 먼저 붙인 뒤에 위치를 견준다. 견준 직후에 생긴 줄이 이 구독자에게 push로 가거나 "뒤처짐"으로 잡혀야 놓치지 않는다.
 *
 * @param owner 구독의 수명을 쥔 쪽. 소켓 하나가 구독자 하나다
 */
export function setPaneChatSubscriptions(
  owner: object,
  targets: (PaneChatSubscriptionTarget & { cursor?: PaneChatCursor | null })[],
  send: (event: PaneChatPushEvent) => void,
): PaneChatSubscriptionFreshness[] {
  const previousSubscribers = subscribersByOwner.get(owner) ?? new Map<string, PaneSubscriber>();
  const nextSubscribers = new Map<string, PaneSubscriber>();
  const changedWatchers = new Set<PaneWatcher>();

  for (const { cursor: _cursor, ...target } of targets) {
    const key = paneKey(target.taskId, target.paneId);
    const previous = previousSubscribers.get(key);
    if (previous && previous.target.detail === target.detail && previous.target.wantsScreen === target.wantsScreen) {
      nextSubscribers.set(key, previous);
      continue;
    }
    const subscriber: PaneSubscriber = { target, send, lastSentState: null };
    if (previous) removeSubscriberKeepingWatcher(key, previous);
    changedWatchers.add(addSubscriber(key, subscriber));
    nextSubscribers.set(key, subscriber);
  }
  for (const [key, previous] of previousSubscribers) {
    if (!nextSubscribers.has(key)) removeSubscriber(key, previous);
  }

  if (nextSubscribers.size > 0) subscribersByOwner.set(owner, nextSubscribers);
  else subscribersByOwner.delete(owner);
  for (const watcher of changedWatchers) requestRead(watcher);

  return targets.map(({ taskId, paneId, cursor }) => ({
    taskId,
    paneId,
    freshness: isPaneChatCursorCurrent(timelines.get(paneKey(taskId, paneId)) ?? null, cursor ?? null) ? "current" : "behind",
  }));
}

/** 같은 pane의 구독 조건만 바뀔 때 감시를 끊었다 다시 걸지 않도록 구독자만 뺀다 */
function removeSubscriberKeepingWatcher(key: string, subscriber: PaneSubscriber): void {
  watchers.get(key)?.subscribers.delete(subscriber);
}

export function clearPaneChatSubscriptions(owner: object): void {
  const subscribers = subscribersByOwner.get(owner);
  if (!subscribers) return;
  subscribersByOwner.delete(owner);
  for (const [key, subscriber] of subscribers) removeSubscriber(key, subscriber);
}

/** 순번이 붙은 스냅샷. 구독자는 여기서 받은 위치부터 push를 이어 받는다 */
export async function getSequencedPaneChatSnapshot(taskId: string, paneId?: string): Promise<PaneChatSnapshot> {
  return sequenceSnapshot((await readPaneChat(taskId, paneId)).snapshot);
}
