/** @vitest-environment node */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { broadcastBoardUpdate, broadcastTaskStatusChanged, broadcastHookStatusTargetMissing, broadcastTaskHookInstallFailed } from "@/lib/boardNotifier";
import { startMobileNotifications, stopMobileNotifications, subscribeToMobileEvents } from "@/desktop/main/services/mobileNotificationService";
import { createPushRelayServer } from "../../../../../services/mobile-push-relay/server.mjs";

const notificationSettings = vi.hoisted(() => ({ isEnabled: true, enabledStatuses: ["todo", "progress", "pending", "review", "done"] }));
vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getNotificationSettings: async () => notificationSettings,
}));

const statusChange = { projectName: "kanvibe", branchName: "feat/mobile", taskTitle: "모바일 구현", description: null, newStatus: "review", taskId: "task-1" };
const subscriptions = [
  { deviceId: "device-1", pushToken: "push-1", platform: "ios" as const, connectionId: "connection-1" },
  { deviceId: "device-2", pushToken: "push-2", platform: "android" as const, connectionId: "connection-2" },
];
const fetchMock = vi.fn<typeof fetch>();
const nativeFetch = globalThis.fetch;
let unsubscribe = () => {};

beforeEach(() => {
  notificationSettings.isEnabled = true;
  notificationSettings.enabledStatuses = ["todo", "progress", "pending", "review", "done"];
  vi.stubGlobal("fetch", fetchMock);
  fetchMock.mockReset().mockResolvedValue(new Response(null, { status: 200 }));
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  stopMobileNotifications();
  unsubscribe();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("모바일 hook 상태 이벤트 전달", () => {
  it("긴 유니코드 설명도 알림과 data를 합쳐 FCM 4096바이트 제한 안에 보내고 WebSocket 문구와 일치한다", async () => {
    const events: unknown[] = [];
    unsubscribe = subscribeToMobileEvents((event) => events.push(event));
    const provider = { send: vi.fn<(message: Record<string, unknown>) => Promise<void>>().mockResolvedValue(undefined) };
    const relayToken = "relay-integration-secret".repeat(2);
    const relay = createPushRelayServer({ relayToken, provider });
    await new Promise<void>((resolve) => relay.listen(0, "127.0.0.1", resolve));
    vi.stubGlobal("fetch", nativeFetch);
    try {
      const port = (relay.address() as { port: number }).port;
      startMobileNotifications({
        relayUrl: `http://127.0.0.1:${port}/notifications`, relayToken,
        readSubscriptions: async () => [{ ...subscriptions[0], connectionId: "🚀".repeat(32) }],
      });
      broadcastTaskStatusChanged({ ...statusChange, projectName: "🚀".repeat(300), taskTitle: "한글".repeat(1000), description: "상세 설명".repeat(2000) });
      await vi.waitFor(() => expect(provider.send).toHaveBeenCalledTimes(1));
      const payload = { ...provider.send.mock.calls[0][0] };
      delete payload.token;
      expect(Buffer.byteLength(JSON.stringify(payload))).toBeLessThanOrEqual(4096);
      const { shouldNotify: _, ...event } = events[0] as Record<string, unknown>;
      expect(payload.data).toEqual({ ...event, connectionId: "🚀".repeat(32) });
      expect(payload.notification).toEqual(expect.objectContaining({ body: expect.stringContaining("한글") }));
    } finally {
      stopMobileNotifications();
      await new Promise<void>((resolve) => relay.close(() => resolve()));
    }
  });

  it("하나의 hook 이벤트를 같은 식별자로 WebSocket과 각 기기 푸시에 전달하며 페어링 비밀값은 전송하지 않는다", async () => {
    const events: unknown[] = [];
    unsubscribe = subscribeToMobileEvents((event) => events.push(event));
    startMobileNotifications({ relayUrl: "https://relay.example/notifications", relayToken: "relay-secret", readSubscriptions: async () => subscriptions });
    broadcastBoardUpdate();
    broadcastTaskStatusChanged(statusChange);
    await vi.waitFor(() => expect(events).toHaveLength(2));
    expect(events).toEqual([
      { type: "board-updated" },
      { type: "task-status-changed", eventId: expect.any(String), taskId: "task-1", title: "kanvibe — feat/mobile", body: "모바일 구현: review로 변경", status: "review", createdAt: expect.any(String), shouldNotify: true },
    ]);
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
    const payloads = fetchMock.mock.calls.map(([, options]) => JSON.parse(options!.body as string));
    const { shouldNotify: _, ...event } = events[1] as Record<string, unknown>;
    expect(payloads).toEqual(subscriptions.map((subscription) => ({
      token: subscription.pushToken,
      notification: { title: "kanvibe — feat/mobile", body: "모바일 구현: review로 변경" },
      data: { ...event, connectionId: subscription.connectionId },
    })));
    expect(fetchMock.mock.calls[0]).toEqual(["https://relay.example/notifications", expect.objectContaining({ method: "POST", headers: { "content-type": "application/json", authorization: "Bearer relay-secret" }, signal: expect.any(AbortSignal) })]);
    expect(Object.values(payloads[0].data).every((value) => typeof value === "string")).toBe(true);
  });

  it("릴레이 설정이 없어도 모든 hook 상태를 WebSocket으로 전달한다", async () => {
    const events: unknown[] = [];
    unsubscribe = subscribeToMobileEvents((event) => events.push(event));
    startMobileNotifications({ readSubscriptions: async () => subscriptions });
    broadcastTaskStatusChanged({ ...statusChange, newStatus: "progress" });
    broadcastTaskStatusChanged({ ...statusChange, newStatus: "done" });
    await vi.waitFor(() => expect(events).toHaveLength(2));
    expect(events).toEqual([expect.objectContaining({ status: "progress" }), expect.objectContaining({ status: "done" })]);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("멈춘 릴레이는 hook 이벤트 발행을 막지 않고 종료하면 진행 중 요청을 취소한다", async () => {
    fetchMock.mockImplementation(() => new Promise(() => {}));
    const events: unknown[] = [];
    unsubscribe = subscribeToMobileEvents((event) => events.push(event));
    startMobileNotifications({ relayUrl: "https://relay.example/notifications", relayToken: "relay-secret", readSubscriptions: async () => subscriptions.slice(0, 1) });
    broadcastTaskStatusChanged(statusChange);
    broadcastTaskStatusChanged({ ...statusChange, newStatus: "done" });
    await vi.waitFor(() => expect(events).toHaveLength(2));
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
    const signal = fetchMock.mock.calls[0][1]!.signal!;
    stopMobileNotifications();
    expect(signal.aborted).toBe(true);
    broadcastTaskStatusChanged(statusChange);
    await vi.waitFor(() => expect(events).toHaveLength(2));
  });

  it("실패 응답의 본문과 URL·토큰을 로그에 남기지 않고 다른 기기 푸시를 계속 보낸다", async () => {
    fetchMock.mockResolvedValueOnce(new Response("SECRET-FCM-TOKEN", { status: 503 }));
    startMobileNotifications({ relayUrl: "https://relay.example/notifications", relayToken: "relay-secret", readSubscriptions: async () => subscriptions });
    broadcastTaskStatusChanged(statusChange);
    await vi.waitFor(() => expect(console.error).toHaveBeenCalled());
    expect(fetchMock).toHaveBeenCalledTimes(2);
    const logs = JSON.stringify(vi.mocked(console.error).mock.calls);
    expect(logs).not.toContain("SECRET");
    expect(logs).not.toContain("relay-secret");
    expect(logs).not.toContain("push-1");
    expect(logs).not.toContain("https://");
  });

  it("구독 목록을 기다리는 중 서버가 멈추면 푸시를 보내지 않는다", async () => {
    let finishRead!: (value: typeof subscriptions) => void;
    startMobileNotifications({ relayUrl: "https://relay.example/notifications", relayToken: "relay-secret", readSubscriptions: () => new Promise((resolve) => { finishRead = resolve; }) });
    broadcastTaskStatusChanged(statusChange);
    await vi.waitFor(() => expect(finishRead).toBeTypeOf("function"));
    stopMobileNotifications();
    finishRead(subscriptions);
    await Promise.resolve();
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

it.each([false, true])("알림 활성화=%s일 때 선택한 상태만 푸시하고 모든 상태 동기화는 유지한다", async (isEnabled) => {
  notificationSettings.isEnabled = isEnabled;
  notificationSettings.enabledStatuses = ["review"];
  const events: unknown[] = [];
  unsubscribe = subscribeToMobileEvents((event) => events.push(event));
  startMobileNotifications({ relayUrl: "https://relay.example/notifications", relayToken: "relay-secret", readSubscriptions: async () => subscriptions.slice(0, 1) });
  broadcastTaskStatusChanged(statusChange);
  broadcastTaskStatusChanged({ ...statusChange, newStatus: "done" });
  await vi.waitFor(() => expect(events).toHaveLength(2));
  expect(events).toEqual([
    expect.objectContaining({ status: "review", shouldNotify: isEnabled }),
    expect.objectContaining({ status: "done", shouldNotify: false }),
  ]);
  expect(fetchMock).toHaveBeenCalledTimes(isEnabled ? 1 : 0);
});


describe("모바일 운영 경고 전달", () => {
  it("hook 설치 실패는 비밀 오류를 빼고 전경 경고로 전달한다", async () => {
    const events: unknown[] = [];
    unsubscribe = subscribeToMobileEvents((event) => events.push(event));
    startMobileNotifications({ relayUrl: "https://relay.example/notifications", relayToken: "relay-secret", readSubscriptions: async () => subscriptions });
    broadcastTaskHookInstallFailed({ taskId: "task-1", taskTitle: "fixture", error: "private shell command" });
    await vi.waitFor(() => expect(events).toHaveLength(1));
    expect(events[0]).toMatchObject({ type: "operational-warning", kind: "task-hook-install-failed", taskId: "task-1", shouldNotify: false });
    expect(JSON.stringify(events)).not.toContain("private shell command");
    expect(fetchMock).not.toHaveBeenCalled();
  });
  it("대상 누락은 기존 상태별 설정을 따르고 WebSocket과 푸시가 같은 eventId를 쓴다", async () => {
    const events: unknown[] = [];
    unsubscribe = subscribeToMobileEvents((event) => events.push(event));
    startMobileNotifications({ relayUrl: "https://relay.example/notifications", relayToken: "relay-secret", readSubscriptions: async () => subscriptions.slice(0, 1) });
    broadcastHookStatusTargetMissing({ taskId: "missing", requestedStatus: "review", reason: "task-not-found" });
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce());
    const payload = JSON.parse(fetchMock.mock.calls[0][1]!.body as string);
    expect(payload.data).toMatchObject({ type: "operational-warning", kind: "hook-status-target-missing", eventId: (events[0] as { eventId: string }).eventId });
    notificationSettings.isEnabled = false;
    broadcastHookStatusTargetMissing({ taskId: "missing", requestedStatus: "review", reason: "task-not-found" });
    await vi.waitFor(() => expect(events).toHaveLength(2));
    expect(events[1]).toMatchObject({ shouldNotify: false });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
  it("대상 누락 경고를 실제 중계 HTTP 계약으로 전달하고 알 수 없는 종류는 거절한다", async () => {
    const provider = { send: vi.fn<(message: Record<string, unknown>) => Promise<void>>().mockResolvedValue(undefined) };
    const relayToken = "operational-warning-fixture-token";
    const relay = createPushRelayServer({ relayToken, provider });
    await new Promise<void>((resolve) => relay.listen(0, "127.0.0.1", resolve));
    vi.stubGlobal("fetch", nativeFetch);
    try {
      const port = (relay.address() as { port: number }).port;
      startMobileNotifications({ relayUrl: `http://127.0.0.1:${port}/notifications`, relayToken, readSubscriptions: async () => subscriptions.slice(0, 1) });
      broadcastHookStatusTargetMissing({ taskId: "missing", requestedStatus: "review", reason: "task-not-found" });
      await vi.waitFor(() => expect(provider.send).toHaveBeenCalledOnce());
      const message = provider.send.mock.calls[0][0];
      expect(message.data).toMatchObject({ type: "operational-warning", kind: "hook-status-target-missing", taskId: "missing" });
      const badResponse = await nativeFetch(`http://127.0.0.1:${port}/notifications`, { method: "POST", headers: { authorization: `Bearer ${relayToken}`, "content-type": "application/json" }, body: JSON.stringify({ token: message.token, notification: message.notification, data: { ...(message.data as Record<string, unknown>), kind: "arbitrary-command" } }) });
      expect(badResponse.status).toBe(400);
      expect(provider.send).toHaveBeenCalledTimes(1);
    } finally {
      stopMobileNotifications();
      await new Promise<void>((resolve) => relay.close(() => resolve()));
    }
  });

});
