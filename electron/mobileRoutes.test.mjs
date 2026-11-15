/**
 * @vitest-environment node
 */
import { EventEmitter } from "node:events";
import http from "node:http";
import https from "node:https";
import net from "node:net";
import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WebSocket from "ws";

const require = createRequire(import.meta.url);
const { handleMobileRequest, attachMobileStreamServer } = require("./mobileRoutes");
const { MAX_BODY_BYTES, readJsonBody } = require("./httpBody");

const HOST = "127.0.0.1";
const transportCertificate = require("selfsigned").generate([{ name: "commonName", value: HOST }], { keySize: 2048 });

let bridge;
let server;
let streamServer;
let port;
let openSockets;
let unpairedListeners;
let mobileEventListeners;
let notificationChangeListeners;
let sessionChangeWatchers;

/** 헤더마다 다른 기기가 오게 두어야 스트림 하나를 끊는 것이 다른 기기까지 끊는지 가려낼 수 있다 */
const DEVICE_IDS = { "Bearer good": "device-good", "Bearer second": "device-second" };
const INBOX_NOTIFICATION = {
  id: "notice-1", title: "리뷰 요청", body: "변경을 확인해 주세요", taskId: "task-1", isRead: false, createdAt: "2026-11-01T00:00:00.000Z",
};

function createBridge(overrides = {}) {
  unpairedListeners = new Set();
  mobileEventListeners = new Set();
  notificationChangeListeners = new Set();
  sessionChangeWatchers = new Set();

  return {
    authorizeMobileRequest: vi.fn(async (header) => DEVICE_IDS[header] ?? null),
    onMobileDeviceUnpaired: vi.fn((listener) => {
      unpairedListeners.add(listener);
      return () => unpairedListeners.delete(listener);
    }),
    pairMobileDevice: vi.fn(async () => "paired-token"),
    unpairMobileDeviceByToken: vi.fn(async () => true),
    getMobileBoard: vi.fn(async () => ({ columns: [] })),
    getMobileDirectories: vi.fn(async () => ["Documents", "Projects"]),
    scanMobileProjects: vi.fn(async () => ({ registered: ["kanvibe"], skipped: [], errors: [], worktreeTasks: [], registeredWorktrees: [], hooksSetup: [] })),
    getMobileTask: vi.fn(async () => ({ id: "task-1", title: "알림 태스크", projectName: "kanvibe" })),
    getMobileNotificationSettings: vi.fn(async () => ({ isEnabled: true, enabledStatuses: ["review"] })),
    updateMobileNotificationSettings: vi.fn(async (settings) => settings),
    isMobileAiSessionProvider: (provider) => ["codex", "hermes"].includes(provider),
    getMobileAiSessions: vi.fn(async () => ({ sessions: [{ id: "session-1", provider: "hermes" }], sources: [], nextCursor: null })),
    getMobileAiSessionDetail: vi.fn(async () => ({ sessionId: "session-1", provider: "hermes", messages: [{ role: "assistant", fullText: "기록 본문" }], nextCursor: null })),
    getMobileNotifications: vi.fn(async () => [INBOX_NOTIFICATION]),
    markMobileNotificationRead: vi.fn(async () => ({ ...INBOX_NOTIFICATION, isRead: true })),
    markAllMobileNotificationsRead: vi.fn(async () => {}),
    markMobileTaskNotificationsRead: vi.fn(async () => 2),
    onMobileNotificationsChanged: vi.fn((listener) => {
      notificationChangeListeners.add(listener);
      return () => notificationChangeListeners.delete(listener);
    }),
    updateMobileNotificationSubscription: vi.fn(async () => true),
    deleteMobileNotificationSubscription: vi.fn(async () => true),
    getTaskSurfaces: vi.fn(async () => ({ panes: [] })),
    prepareMobilePaneConnection: vi.fn(async () => null),
    openRemoteMobilePaneStream: vi.fn(async () => null),
    createMobileTerminalTab: vi.fn(async () => ({
      surfaces: { taskId: "task-1", sessionType: "tmux", canResize: true, tabs: [] },
      paneId: "%20",
    })),
    subscribeToPane: vi.fn(async () => () => {}),
    setPaneChatSubscriptions: vi.fn((owner, targets) => targets.map(({ taskId, paneId }) => ({ taskId, paneId, freshness: "behind" }))),
    clearPaneChatSubscriptions: vi.fn(),
    writeToPane: vi.fn(async () => {}),
    executeMobileTerminalShortcut: vi.fn(async () => ({ surfaces: { tabs: [] }, screen: "screen" })),
    claimPaneViewport: vi.fn(async () => ({ resize: vi.fn(async () => {}), release: vi.fn(async () => {}) })),
    refreshPaneHistory: vi.fn(async () => {}),
    startMobilePairing: vi.fn(() => ({ code: "654321", expiresAt: 1_700_000_000_000 })),
    createMobileTask: vi.fn(async () => ({ id: "task-1", title: "feat/from-mobile" })),
    updateMobileTaskStatus: vi.fn(async () => ({ id: "task-1", status: "review" })),
    deleteMobileTask: vi.fn(async () => ({ failedPeerCount: 0 })),
    removeMobileTaskRegistration: vi.fn(async () => true),
    closeMobilePane: vi.fn(async () => ({ surfaces: { tabs: [] } })),
    getMobilePaneCloseInfo: vi.fn(async () => ({ processNames: [] })),
    getMobileProjectBranches: vi.fn(async () => ["main", "dev"]),
    changeMobileTaskSessionType: vi.fn(async () => ({ id: "task-1", sessionType: "zellij" })),
    isMobileTaskSessionAlive: vi.fn(async () => true),
    watchTaskSessionChange: vi.fn((taskId, onChanged) => {
      sessionChangeWatchers.add({ taskId, onChanged });
      return () => sessionChangeWatchers.forEach((watcher) => {
        if (watcher.onChanged === onChanged) sessionChangeWatchers.delete(watcher);
      });
    }),
    ...overrides,
  };
}

/** hookServer와 같은 방식으로 얹어야 "우리 경로가 아니면 false" 계약까지 같이 검증된다 */
async function startServer({ isDevPairingRouteEnabled = false, secure = false } = {}) {
  const certificate = secure ? await transportCertificate : null;
  return new Promise((resolve) => {
    openSockets = [];
    const listener = async (request, response) => {
      if (await handleMobileRequest(request, response, { bridge, host: HOST, port, isDevPairingRouteEnabled })) {
        return;
      }
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ handledBy: "hook-fallthrough" }));
    };
    const created = secure ? https.createServer({ cert: certificate.cert, key: certificate.private }, listener) : http.createServer(listener);
    created.on("connection", (socket) => openSockets.push(socket));
    streamServer = attachMobileStreamServer(created, {
      bridge,
      notifications: {
        subscribeToMobileEvents: (listener) => {
          mobileEventListeners.add(listener);
          return () => mobileEventListeners.delete(listener);
        },
      },
      host: HOST,
      port: 0,
    });
    created.listen(0, HOST, () => {
      port = created.address().port;
      resolve(created);
    });
  });
}

function request(path, { method = "GET", headers = {}, body } = {}) {
  return new Promise((resolve, reject) => {
    const transport = server instanceof https.Server ? https : http;
    const clientRequest = transport.request({ host: HOST, port, path, method, headers, rejectUnauthorized: false }, (response) => {
      let raw = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => {
        raw += chunk;
      });
      response.on("end", () => resolve({ status: response.statusCode, body: raw ? JSON.parse(raw) : null }));
    });
    clientRequest.on("error", reject);
    if (body !== undefined) {
      clientRequest.write(body);
    }
    clientRequest.end();
  });
}

/** 서비스가 태스크 세션 변경을 알리는 자리. 실제로는 보드 이벤트 `task-session-changed`가 이 신호를 보낸다 */
function notifyTaskSessionChanged(taskId) {
  [...sessionChangeWatchers].filter((watcher) => watcher.taskId === taskId).forEach((watcher) => watcher.onChanged());
}

/** 서비스가 기기 해제를 알리는 자리. 실제로는 `mobileBridgeService`의 두 해제 경로가 이 신호를 보낸다 */
function notifyUnpaired(deviceId) {
  unpairedListeners.forEach((listener) => listener(deviceId));
}

/** 테스트가 직접 pane 출력을 흘려보낼 수 있게, 구독 콜백을 모아 두는 가짜 구독을 세운다 */
function stubPaneChunks() {
  const chunkListeners = new Set();
  const stops = [];

  const endListeners = new Set();

  bridge.subscribeToPane.mockImplementation(async (taskId, paneId, onChunk, onEnd) => {
    chunkListeners.add(onChunk);
    endListeners.add(onEnd);
    const stop = vi.fn(() => chunkListeners.delete(onChunk));
    stops.push(stop);
    return stop;
  });

  return {
    emit: (chunk) => chunkListeners.forEach((listener) => listener(chunk)),
    endAll: () => endListeners.forEach((listener) => listener()),
    stops,
  };
}

async function openPaneStreamClient(authorization) {
  const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/stream?taskId=task-1&paneId=pane-1`, {
    headers: { authorization },
  });
  await new Promise((resolve, reject) => {
    client.once("open", resolve);
    client.once("error", reject);
  });
  return client;
}

function waitForClose(client) {
  return new Promise((resolve) => {
    client.once("close", (code, reason) => resolve({ code, reason: reason.toString() }));
  });
}

function sleep(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function waitFor(predicate, timeoutMs = 3000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) {
      return;
    }
    await sleep(10);
  }
  throw new Error("조건이 시간 안에 만족되지 않았다");
}

beforeEach(async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  bridge = createBridge();
  server = await startServer();
});

afterEach(async () => {
  for (const socket of openSockets) {
    socket.destroy();
  }
  await new Promise((resolve) => server.close(resolve));
  vi.restoreAllMocks();
});

describe("모바일 라우트 인증 경계", () => {
  it("페어링 요청은 토큰 없이도 지나간다", async () => {
    const response = await request("/api/mobile/pair", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: "123456", deviceName: "iPhone" }),
    });

    expect(response.status).toBe(200);
    expect(response.body).toEqual({ success: true, token: "paired-token" });
    expect(bridge.authorizeMobileRequest).not.toHaveBeenCalled();
  });

  it("보드와 pane 경로는 토큰 없이는 401로 막힌다", async () => {
    const board = await request("/api/mobile/board");
    const surfaces = await request("/api/mobile/tasks/task-1/surfaces");

    expect(board.status).toBe(401);
    expect(surfaces.status).toBe(401);
    expect(bridge.getMobileBoard).not.toHaveBeenCalled();
    expect(bridge.getTaskSurfaces).not.toHaveBeenCalled();
  });

  it("모바일 접두사 아래의 모르는 경로는 404를 준다", async () => {
    const response = await request("/api/mobile/unknown", { headers: { authorization: "Bearer good" } });

    expect(response.status).toBe(404);
    expect(response.body).toEqual({ success: false, error: "Not found" });
  });

  it("모바일 경로가 아니면 false를 돌려 hook 라우팅으로 넘긴다", async () => {
    const response = await request("/api/hooks/health");

    expect(response.status).toBe(200);
    expect(response.body).toEqual({ handledBy: "hook-fallthrough" });
  });
});

describe("모바일 상태 알림 경로", () => {
  const subscription = { pushToken: "fcm-token", platform: "ios", connectionId: "connection-1" };

  it("알림 등록과 해제, 태스크 조회는 페어링 인증을 통과해야 한다", async () => {
    const [register, unregister, task] = await Promise.all([
      request("/api/mobile/notifications", { method: "PUT", body: JSON.stringify(subscription) }),
      request("/api/mobile/notifications", { method: "DELETE" }),
      request("/api/mobile/tasks/task-1"),
    ]);
    expect([register.status, unregister.status, task.status]).toEqual([401, 401, 401]);
    expect(bridge.updateMobileNotificationSubscription).not.toHaveBeenCalled();
  });

  it("알림 토큰은 요청에 인증된 기기에만 등록하고 해제한다", async () => {
    const headers = { authorization: "Bearer good", "content-type": "application/json" };
    expect((await request("/api/mobile/notifications", { method: "PUT", headers, body: JSON.stringify(subscription) })).status).toBe(200);
    expect(bridge.updateMobileNotificationSubscription).toHaveBeenCalledWith("Bearer good", subscription);
    expect((await request("/api/mobile/notifications", { method: "DELETE", headers })).status).toBe(200);
    expect(bridge.deleteMobileNotificationSubscription).toHaveBeenCalledWith("Bearer good");
  });

  it.each([
    ["null 본문", null],
    ["빈 토큰", { ...subscription, pushToken: "" }],
    ["모르는 플랫폼", { ...subscription, platform: "web" }],
    ["빈 연결 식별자", { ...subscription, connectionId: "" }],
    ["토큰 길이 초과", { ...subscription, pushToken: "a".repeat(4097) }],
    ["연결 식별자 바이트 초과", { ...subscription, connectionId: "🚀".repeat(33) }],
  ])("잘못된 알림 구독은 저장하지 않는다: %s", async (_label, body) => {
    const response = await request("/api/mobile/notifications", {
      method: "PUT", headers: { authorization: "Bearer good" }, body: JSON.stringify(body),
    });
    expect(response.status).toBe(400);
    expect(bridge.updateMobileNotificationSubscription).not.toHaveBeenCalled();
  });

  it("인증 뒤에 해제된 기기는 알림 구독을 등록할 수 없다", async () => {
    bridge.updateMobileNotificationSubscription.mockResolvedValue(false);
    const response = await request("/api/mobile/notifications", {
      method: "PUT", headers: { authorization: "Bearer good" }, body: JSON.stringify(subscription),
    });
    expect(response.status).toBe(401);
  });

  it("보드 첫 페이지 밖의 태스크도 알림 클릭으로 조회하고 없는 태스크는 404를 준다", async () => {
    const response = await request("/api/mobile/tasks/task-1", { headers: { authorization: "Bearer good" } });
    expect(response).toEqual({ status: 200, body: { success: true, task: { id: "task-1", title: "알림 태스크", projectName: "kanvibe" } } });
    bridge.getMobileTask.mockResolvedValue(null);
    expect((await request("/api/mobile/tasks/missing", { headers: { authorization: "Bearer good" } })).status).toBe(404);
  });

  it("pane 채팅 push는 구독한 소켓에만 가고, 소켓이 닫히면 그 구독을 지운다", async () => {
    const open = async () => {
      const messages = [];
      const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/events`, { headers: { authorization: "Bearer good" } });
      client.on("message", (message) => messages.push(JSON.parse(message.toString())));
      await new Promise((resolve, reject) => { client.once("open", resolve); client.once("error", reject); });
      await waitFor(() => messages.length === 1);
      return { client, messages };
    };
    const subscriber = await open();
    const bystander = await open();
    const pane = { taskId: "task-1", paneId: "%3", detail: "chat", wantsScreen: true, cursor: { epoch: "epoch-a", seq: 4 } };
    subscriber.client.send(JSON.stringify({ type: "command", requestId: "sub-1", command: "set-pane-chat-subscription", panes: [pane] }));
    await waitFor(() => subscriber.messages.length === 2);
    expect(subscriber.messages[1]).toEqual({ type: "command-result", requestId: "sub-1", ok: true, panes: [{ taskId: "task-1", paneId: "%3", freshness: "behind" }] });
    const [owner, targets, send] = bridge.setPaneChatSubscriptions.mock.calls[0];
    expect(targets).toEqual([pane]);

    const pushed = { type: "pane-chat-rows", taskId: "task-1", paneId: "%3", epoch: "epoch-a", rows: [{ seq: 5, message: { role: "assistant", fullText: "새 답변" } }] };
    send(pushed);
    await waitFor(() => subscriber.messages.length === 3);
    expect(subscriber.messages[2]).toEqual(pushed);
    expect(bystander.messages).toHaveLength(1);

    subscriber.client.close();
    await waitFor(() => bridge.clearPaneChatSubscriptions.mock.calls.length === 1);
    expect(bridge.clearPaneChatSubscriptions).toHaveBeenCalledWith(owner);
    expect(() => send(pushed)).not.toThrow();
    bystander.client.close();
  });

  it("상태 WebSocket은 연결 확인과 보드·태스크 알림을 보내고 다른 기기 해제에 영향받지 않는다", async () => {
    const messages = [];
    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/events`, { headers: { authorization: "Bearer good" } });
    client.on("message", (message) => messages.push(JSON.parse(message.toString())));
    await new Promise((resolve, reject) => { client.once("open", resolve); client.once("error", reject); });
    await waitFor(() => messages.length === 1);
    const event = { type: "task-status-changed", eventId: "event-1", taskId: "task-1", title: "kanvibe", body: "review", status: "review", createdAt: "2026-10-23T00:00:00Z" };
    mobileEventListeners.forEach((listener) => { listener({ type: "board-updated" }); listener(event); });
    await waitFor(() => messages.length === 3);
    expect(messages).toEqual([{ type: "connected", commands: ["delete-task", "remove-task-registration", "get-tab-close-info", "close-tab", "start-session", "set-pane-chat-subscription"] }, { type: "board-updated" }, event]);
    expect(bridge.subscribeToPane).not.toHaveBeenCalled();
    notifyUnpaired("device-second");
    expect(client.readyState).toBe(WebSocket.OPEN);
    const closing = waitForClose(client);
    notifyUnpaired("device-good");
    expect(await closing).toEqual({ code: 4001, reason: "device-unpaired" });
  });

  it("상태 WebSocket은 인증하지 않은 기기의 핸드셰이크를 거절한다", async () => {
    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/events`);
    const status = await new Promise((resolve) => {
      client.once("unexpected-response", (_request, response) => { resolve(response.statusCode); response.resume(); client.terminate(); });
      client.on("error", () => {});
    });
    expect(status).toBe(401);
  });

  it("서버 종료는 이벤트 구독과 연결된 상태 소켓을 닫는다", async () => {
    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/events`, { headers: { authorization: "Bearer good" } });
    await new Promise((resolve, reject) => { client.once("open", resolve); client.once("error", reject); });
    const closing = waitForClose(client);
    server.close();
    expect(await closing).toEqual({ code: 1001, reason: "server-shutdown" });
    expect(mobileEventListeners.size).toBe(0);
    expect(notificationChangeListeners.size).toBe(0);
  });
});

describe("상태 WebSocket 명령", () => {
  /** 연결 확인(connected)까지 받은 상태로 돌려준다. 이후 도착한 메시지는 messages에 쌓인다 */
  async function connectEvents(authorization = "Bearer good") {
    const messages = [];
    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/events`, { headers: { authorization } });
    client.on("message", (message) => messages.push(JSON.parse(message.toString())));
    await new Promise((resolve, reject) => { client.once("open", resolve); client.once("error", reject); });
    await waitFor(() => messages.length === 1);
    return { client, messages };
  }

  function sendCommand(client, fields, requestId = "r1") {
    client.send(JSON.stringify({ type: "command", requestId, ...fields }));
  }

  async function nextResult(messages, requestId = "r1") {
    await waitFor(() => messages.some((message) => message.requestId === requestId));
    return messages.find((message) => message.requestId === requestId);
  }

  it("세션 시작 명령은 준비된 pane을 요청한 소켓에 돌려준다", async () => {
    const { client, messages } = await connectEvents();
    const surfaces = { taskId: "task-1", sessionType: "herdr", canResize: true, tabs: [{ id: "w1:t1", panes: [{ id: "w1:p1", command: "codex" }] }] };
    bridge.getTaskSurfaces.mockResolvedValue(surfaces);

    sendCommand(client, { command: "start-session", taskId: "task-1" });

    expect(await nextResult(messages)).toEqual({ type: "command-result", requestId: "r1", ok: true, surfaces });
  });

  it.each(["", null, 1])("세션 시작은 잘못된 taskId %s를 거절한다", async (taskId) => {
    const { client, messages } = await connectEvents();
    sendCommand(client, { command: "start-session", taskId });

    expect(await nextResult(messages)).toMatchObject({ ok: false, code: "invalid-command" });
    expect(bridge.getTaskSurfaces).not.toHaveBeenCalled();
  });

  it.each(["herdr-missing", "remote-unreachable", "session-not-running", "no-session"])("세션 시작 실패 사유 %s를 유지한다", async (reason) => {
    const { client, messages } = await connectEvents();
    bridge.getTaskSurfaces.mockRejectedValue(Object.assign(new Error(reason), { name: "SurfaceUnavailableError", reason }));
    sendCommand(client, { command: "start-session", taskId: "task-1" });

    expect(await nextResult(messages)).toMatchObject({ ok: false, code: reason, error: "세션을 시작하지 못했습니다" });
  });

  it("연결 확인에 지원하는 명령 목록을 싣는다", async () => {
    const { messages } = await connectEvents();

    expect(messages[0]).toEqual({ type: "connected", commands: ["delete-task", "remove-task-registration", "get-tab-close-info", "close-tab", "start-session", "set-pane-chat-subscription"] });
  });

  it("삭제 결과는 요청한 소켓에만 돌려주고 다른 기기의 소켓에는 보내지 않는다", async () => {
    const requester = await connectEvents("Bearer good");
    const bystander = await connectEvents("Bearer second");

    sendCommand(requester.client, { command: "delete-task", taskId: "task-1", isForced: false });
    const result = await nextResult(requester.messages);
    mobileEventListeners.forEach((listener) => listener({ type: "board-updated" }));
    await waitFor(() => bystander.messages.length === 2);

    expect(result).toEqual({ type: "command-result", requestId: "r1", ok: true, failedPeerCount: 0 });
    expect(bridge.deleteMobileTask).toHaveBeenCalledExactlyOnceWith("task-1", false);
    expect(bystander.messages.slice(1)).toEqual([{ type: "board-updated" }]);
  });

  it("커밋하지 않은 변경이 있으면 dirty-worktree로 알리고 강제 삭제 요청은 isForced를 넘긴다", async () => {
    const { client, messages } = await connectEvents();
    bridge.deleteMobileTask.mockRejectedValueOnce(Object.assign(new Error("dirty"), { name: "DirtyWorktreeError" }));

    sendCommand(client, { command: "delete-task", taskId: "task-1" }, "first");
    sendCommand(client, { command: "delete-task", taskId: "task-1", isForced: true }, "forced");

    expect(await nextResult(messages, "first")).toMatchObject({ ok: false, code: "dirty-worktree" });
    expect(await nextResult(messages, "forced")).toMatchObject({ ok: true });
    expect(bridge.deleteMobileTask.mock.calls).toEqual([["task-1", false], ["task-1", true]]);
  });

  it("없는 태스크는 task-not-found로 알린다", async () => {
    const { client, messages } = await connectEvents();
    bridge.deleteMobileTask.mockResolvedValue(null);

    sendCommand(client, { command: "delete-task", taskId: "missing" });

    expect(await nextResult(messages)).toMatchObject({ ok: false, code: "task-not-found" });
  });

  it("탭 닫기는 지정한 pane만 브리지에 넘기고 닫힌 뒤의 surfaces를 돌려준다", async () => {
    const { client, messages } = await connectEvents();
    const surfaces = { taskId: "task-1", sessionType: "tmux", canResize: true, tabs: [] };
    bridge.closeMobilePane.mockResolvedValue({ surfaces });

    sendCommand(client, { command: "close-tab", taskId: "task-1", paneId: "%20" });

    expect(await nextResult(messages)).toEqual({ type: "command-result", requestId: "r1", ok: true, surfaces });
    expect(bridge.closeMobilePane).toHaveBeenCalledExactlyOnceWith("task-1", "%20");
  });

  it.each([
    ["LastPaneError", "last-tab"],
    ["PaneNotFoundError", "pane-not-found"],
    ["SurfaceUnavailableError", "session-unavailable"],
  ])("탭 닫기 중 %s는 %s로 알린다", async (errorName, code) => {
    const { client, messages } = await connectEvents();
    bridge.closeMobilePane.mockRejectedValue(Object.assign(new Error(errorName), { name: errorName, reason: "no-session" }));

    sendCommand(client, { command: "close-tab", taskId: "task-1", paneId: "%20" });

    expect(await nextResult(messages)).toMatchObject({ ok: false, code });
  });

  it("예상하지 못한 오류는 내부 사유를 숨기고 server-error로 알린다", async () => {
    const { client, messages } = await connectEvents();
    bridge.closeMobilePane.mockRejectedValue(new Error("tmux: /private/path failed"));

    sendCommand(client, { command: "close-tab", taskId: "task-1", paneId: "%20" });

    const result = await nextResult(messages);
    expect(result).toMatchObject({ ok: false, code: "server-error" });
    expect(JSON.stringify(result)).not.toContain("/private/path");
  });

  it("탭 닫기 정보는 실행 중인 프로세스 이름을 돌려준다", async () => {
    const { client, messages } = await connectEvents();
    bridge.getMobilePaneCloseInfo.mockResolvedValue({ processNames: ["vim"] });

    sendCommand(client, { command: "get-tab-close-info", taskId: "task-1", paneId: "%20" });

    expect(await nextResult(messages)).toEqual({ type: "command-result", requestId: "r1", ok: true, processNames: ["vim"] });
  });

  it("모르는 명령은 응답하고, requestId 없는 메시지는 응답 없이 소켓을 유지한다", async () => {
    const { client, messages } = await connectEvents();

    client.send(JSON.stringify({ type: "command", command: "delete-task", taskId: "task-1" }));
    sendCommand(client, { command: "rename-task", taskId: "task-1" }, "unknown");
    const unknown = await nextResult(messages, "unknown");

    expect(unknown).toMatchObject({ ok: false, code: "unsupported-command" });
    expect(messages).toHaveLength(2);
    expect(client.readyState).toBe(WebSocket.OPEN);
    expect(bridge.deleteMobileTask).not.toHaveBeenCalled();
  });

  it("바이너리 프레임은 명령으로 읽지 않는다", async () => {
    const { client, messages } = await connectEvents();

    client.send(Buffer.from(JSON.stringify({ type: "command", requestId: "bin", command: "delete-task", taskId: "task-1" })));
    sendCommand(client, { command: "rename-task", taskId: "task-1" }, "after");
    await nextResult(messages, "after");

    expect(messages.some((message) => message.requestId === "bin")).toBe(false);
    expect(bridge.deleteMobileTask).not.toHaveBeenCalled();
  });
});

describe("모바일 알림함 경로", () => {
  const headers = { authorization: "Bearer good" };

  it("저장된 알림 목록을 성공 응답으로 돌려준다", async () => {
    expect(await request("/api/mobile/notifications", { headers })).toEqual({
      status: 200, body: { success: true, notifications: [INBOX_NOTIFICATION] },
    });
  });

  it("알림이 없으면 빈 목록을 돌려준다", async () => {
    bridge.getMobileNotifications.mockResolvedValue([]);

    expect(await request("/api/mobile/notifications", { headers })).toEqual({
      status: 200, body: { success: true, notifications: [] },
    });
  });

  it("본문 없는 읽음 요청은 경로의 식별자를 풀어 서비스에 넘기고 변경 알림을 돌려준다", async () => {
    expect(await request("/api/mobile/notifications/notice%201/read", { method: "PATCH", headers })).toEqual({
      status: 200, body: { success: true, notification: { ...INBOX_NOTIFICATION, isRead: true } },
    });
    expect(bridge.markMobileNotificationRead).toHaveBeenCalledWith("notice 1");
  });

  it("없는 알림의 읽음 요청은 404로 답한다", async () => {
    bridge.markMobileNotificationRead.mockResolvedValue(null);

    expect(await request("/api/mobile/notifications/missing/read", { method: "PATCH", headers })).toEqual({
      status: 404, body: { success: false, error: "없는 알림입니다" },
    });
  });

  it("본문 없는 모두 읽음 요청은 성공 응답을 돌려준다", async () => {
    expect(await request("/api/mobile/notifications/read-all", { method: "POST", headers })).toEqual({
      status: 200, body: { success: true },
    });
    expect(bridge.markAllMobileNotificationsRead).toHaveBeenCalledTimes(1);
  });

  it("task 알림 읽음 요청은 경로의 task 식별자를 풀어 서비스에 넘기고 읽음 처리한 개수를 돌려준다", async () => {
    expect(await request("/api/mobile/tasks/task%201/notifications/read", { method: "POST", headers })).toEqual({
      status: 200, body: { success: true, updatedCount: 2 },
    });
    expect(bridge.markMobileTaskNotificationsRead).toHaveBeenCalledWith("task 1");
  });

  it.each([
    ["GET", "/api/mobile/notifications", "getMobileNotifications"],
    ["PATCH", "/api/mobile/notifications/notice-1/read", "markMobileNotificationRead"],
    ["POST", "/api/mobile/notifications/read-all", "markAllMobileNotificationsRead"],
    ["POST", "/api/mobile/tasks/task-1/notifications/read", "markMobileTaskNotificationsRead"],
  ])("%s %s는 인증 전 서비스를 호출하지 않는다", async (method, path, service) => {
    expect(await request(path, { method })).toEqual({
      status: 401, body: { success: false, error: "연결되지 않은 기기입니다" },
    });
    expect(bridge[service]).not.toHaveBeenCalled();
  });

  it.each([
    ["GET", "/api/mobile/notifications", "getMobileNotifications"],
    ["PATCH", "/api/mobile/notifications/notice-1/read", "markMobileNotificationRead"],
    ["POST", "/api/mobile/notifications/read-all", "markAllMobileNotificationsRead"],
    ["POST", "/api/mobile/tasks/task-1/notifications/read", "markMobileTaskNotificationsRead"],
  ])("%s %s 서비스 오류는 내부 내용을 감춘 500으로 답한다", async (method, path, service) => {
    bridge[service].mockRejectedValue(new Error("private settings path"));

    expect(await request(path, { method, headers })).toEqual({
      status: 500, body: { success: false, error: "서버 오류" },
    });
  });

  it("알림 저장소 변경은 연결된 기기들에게 invalidation만 보낸다", async () => {
    const firstMessages = [];
    const secondMessages = [];
    const first = new WebSocket(`ws://${HOST}:${port}/api/mobile/events`, { headers });
    const second = new WebSocket(`ws://${HOST}:${port}/api/mobile/events`, { headers: { authorization: "Bearer second" } });
    first.on("message", (message) => firstMessages.push(JSON.parse(message.toString())));
    second.on("message", (message) => secondMessages.push(JSON.parse(message.toString())));
    await waitFor(() => firstMessages.length === 1 && secondMessages.length === 1);

    notificationChangeListeners.forEach((listener) => listener());

    await waitFor(() => firstMessages.length === 2 && secondMessages.length === 2);
    expect(firstMessages).toEqual([{ type: "connected", commands: ["delete-task", "remove-task-registration", "get-tab-close-info", "close-tab", "start-session", "set-pane-chat-subscription"] }, { type: "notifications-changed" }]);
    expect(secondMessages).toEqual(firstMessages);
    first.close();
    second.close();
  });
});

/**
 * 기기가 자기 연결을 끊는 경로. `/pair`와 달리 인증 뒤에 있어야 지우는 대상이 요청이 증명한 기기 하나로 정해진다.
 */
describe("기기 연결 해제 경로", () => {
  it("자기 토큰으로 부르면 그 토큰만 넘겨 지우고 200을 준다", async () => {
    const response = await request("/api/mobile/device", {
      method: "DELETE",
      headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(200);
    expect(response.body).toEqual({ success: true });
    expect(bridge.unpairMobileDeviceByToken).toHaveBeenCalledWith("Bearer good");
  });

  it("토큰 없이는 인증 앞단에서 막혀 아무것도 지우지 않는다", async () => {
    const response = await request("/api/mobile/device", { method: "DELETE" });

    expect(response.status).toBe(401);
    expect(bridge.unpairMobileDeviceByToken).not.toHaveBeenCalled();
  });

  it("모르는 토큰도 인증 앞단에서 막힌다", async () => {
    const response = await request("/api/mobile/device", {
      method: "DELETE",
      headers: { authorization: "Bearer unknown" },
    });

    expect(response.status).toBe(401);
    expect(bridge.unpairMobileDeviceByToken).not.toHaveBeenCalled();
  });

  it("인증과 삭제 사이에 이미 끊긴 기기는 401로 답한다", async () => {
    bridge.unpairMobileDeviceByToken.mockResolvedValue(false);

    const response = await request("/api/mobile/device", {
      method: "DELETE",
      headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(401);
    expect(response.body).toEqual({ success: false, error: "연결되지 않은 기기입니다" });
  });

  it("삭제가 던져도 매달리지 않고 500을 돌려준다", async () => {
    bridge.unpairMobileDeviceByToken.mockRejectedValue(new Error("DB가 닫혔다"));

    const response = await request("/api/mobile/device", {
      method: "DELETE",
      headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
  });
});

describe("모바일 터미널 탭 생성", () => {
  for (const authorization of [undefined, "Bearer unknown"]) {
    it(`인증되지 않은 ${authorization ?? "빈"} 토큰은 새 탭을 만들지 못한다`, async () => {
      const response = await request("/api/mobile/tasks/task-1/tabs", {
        method: "POST",
        headers: authorization ? { authorization } : {},
      });

      expect(response.status).toBe(401);
      expect(bridge.createMobileTerminalTab).not.toHaveBeenCalled();
    });
  }

  it("인증한 태스크에 탭을 추가하고 갱신된 목록과 새 pane을 반환한다", async () => {
    const created = {
      surfaces: { taskId: "task id", sessionType: "terminal", canResize: true, tabs: [{ id: "tab-2", panes: [{ id: "tab-2" }] }] },
      paneId: "tab-2",
    };
    bridge.createMobileTerminalTab.mockResolvedValue(created);

    const response = await request("/api/mobile/tasks/task%20id/tabs", {
      method: "POST",
      headers: { authorization: "Bearer good", "Content-Type": "application/json" },
      body: JSON.stringify({ sshHost: "injected", command: "injected", worktreePath: "/injected" }),
    });

    expect(bridge.createMobileTerminalTab).toHaveBeenCalledExactlyOnceWith("task id");
    expect(response.status).toBe(200);
    expect(response.body).toEqual({ success: true, ...created });
  });

  it("탭을 만들 세션이 없으면 409와 사유를 반환한다", async () => {
    bridge.createMobileTerminalTab.mockRejectedValue(Object.assign(new Error("no-session"), {
      name: "SurfaceUnavailableError",
      reason: "no-session",
    }));

    const response = await request("/api/mobile/tasks/task-1/tabs", {
      method: "POST", headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(409);
    expect(response.body).toEqual({ success: false, reason: "no-session" });
  });

  it("탭 생성이 실패해도 SSH 명령과 호스트를 응답에 노출하지 않는다", async () => {
    bridge.createMobileTerminalTab.mockRejectedValue(new Error("ssh secret-host tmux new-window /secret/path"));

    const response = await request("/api/mobile/tasks/task-1/tabs", {
      method: "POST", headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
  });
});

describe("모바일 지연 최소화 계약", () => {
  it("최소 surfaces 요청만 새 옵션을 전달하고 기본/알 수 없는 값은 기존 계약을 쓴다", async () => {
    const headers = { authorization: "Bearer good" };
    expect((await request("/api/mobile/tasks/task-1/surfaces?metadata=minimal", { headers })).status).toBe(200);
    expect(bridge.getTaskSurfaces).toHaveBeenLastCalledWith("task-1", { metadata: "minimal" });
    await request("/api/mobile/tasks/task-1/surfaces?metadata=minimal&tabId=w1%3At2", { headers });
    expect(bridge.getTaskSurfaces).toHaveBeenLastCalledWith("task-1", { metadata: "minimal", tabId: "w1:t2" });
    await request("/api/mobile/tasks/task-1/surfaces?metadata=unknown", { headers });
    expect(bridge.getTaskSurfaces).toHaveBeenLastCalledWith("task-1");
    await request("/api/mobile/tasks/task-1/surfaces", { headers });
    expect(bridge.getTaskSurfaces).toHaveBeenLastCalledWith("task-1");
  });

  it("아이콘 중복 제거는 명시 요청에만 적용하고 인증을 유지한다", async () => {
    const headers = { authorization: "Bearer good" };
    expect((await request("/api/mobile/board?iconFormat=deduplicated", { headers })).status).toBe(200);
    expect(bridge.getMobileBoard).toHaveBeenLastCalledWith({ iconFormat: "deduplicated" });
    await request("/api/mobile/board", { headers });
    expect(bridge.getMobileBoard).toHaveBeenLastCalledWith();
    bridge.getMobileBoard.mockClear();
    expect((await request("/api/mobile/board?iconFormat=deduplicated")).status).toBe(401);
    expect(bridge.getMobileBoard).not.toHaveBeenCalled();
  });
});

describe("모바일 라우트 오류 응답", () => {
  it("세션이 없는 pane은 409와 이유를 준다", async () => {
    const unavailable = Object.assign(new Error("세션 없음"), {
      name: "SurfaceUnavailableError",
      reason: "session-missing",
    });
    bridge.getTaskSurfaces.mockRejectedValue(unavailable);

    const response = await request("/api/mobile/tasks/task-1/surfaces", {
      headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(409);
    expect(response.body).toEqual({ success: false, reason: "session-missing" });
  });

  it("그 밖의 예외는 내부 메시지를 감춘 500으로 나간다", async () => {
    bridge.getTaskSurfaces.mockRejectedValue(new Error("SSH 호스트를 찾을 수 없습니다: secret-alias"));

    const response = await request("/api/mobile/tasks/task-1/surfaces", {
      headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
    expect(JSON.stringify(response.body)).not.toContain("secret-alias");
  });

  it("보드 조회가 던져도 매달리지 않고 500을 돌려준다", async () => {
    bridge.getMobileBoard.mockRejectedValue(new Error("DB가 닫혔다"));

    const response = await request("/api/mobile/board", { headers: { authorization: "Bearer good" } });

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
  });

  it("경로에 깨진 퍼센트 인코딩이 와도 매달리지 않고 500을 돌려준다", async () => {
    const response = await request("/api/mobile/tasks/%/surfaces", {
      headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
  });

  it("인증 조회가 던져도 매달리지 않고 500을 돌려준다", async () => {
    bridge.authorizeMobileRequest.mockRejectedValue(new Error("DB가 닫혔다"));

    const response = await request("/api/mobile/board");

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
  });
});

describe("요청 본문 상한", () => {
  it("상한을 넘는 본문은 버퍼에 쌓지 않고 소켓을 끊으며 거절한다", async () => {
    const fakeRequest = new EventEmitter();
    fakeRequest.destroy = vi.fn();

    const rejected = readJsonBody(fakeRequest).then(
      () => "resolved",
      (error) => error,
    );

    const chunk = Buffer.alloc(16 * 1024, 0x61);
    for (let index = 0; index * chunk.length <= MAX_BODY_BYTES; index += 1) {
      fakeRequest.emit("data", chunk);
    }

    await expect(rejected).resolves.toBeInstanceOf(Error);
    expect(fakeRequest.destroy).toHaveBeenCalled();
  });

  it("상한을 넘는 페어링 본문은 400으로 거절한다", async () => {
    const fakeRequest = new EventEmitter();
    fakeRequest.url = "/api/mobile/pair";
    fakeRequest.method = "POST";
    fakeRequest.headers = {};
    fakeRequest.destroy = vi.fn();

    const fakeResponse = { writeHead: vi.fn(), end: vi.fn() };
    const handled = handleMobileRequest(fakeRequest, fakeResponse, { bridge, host: HOST, port });

    await sleep(0);
    const chunk = Buffer.alloc(16 * 1024, 0x61);
    for (let index = 0; index * chunk.length <= MAX_BODY_BYTES; index += 1) {
      fakeRequest.emit("data", chunk);
    }

    await expect(handled).resolves.toBe(true);
    expect(fakeResponse.writeHead).toHaveBeenCalledWith(400, { "Content-Type": "application/json" });
    expect(bridge.pairMobileDevice).not.toHaveBeenCalled();
  });
});

describe("pane 스트림 업그레이드", () => {
  it("스트림 경로가 아닌 업그레이드 요청은 소켓을 끊는다", async () => {
    const socket = net.connect(port, HOST);
    await new Promise((resolve, reject) => {
      socket.once("connect", resolve);
      socket.once("error", reject);
    });

    let closed = false;
    socket.on("close", () => {
      closed = true;
    });
    socket.write(`GET / HTTP/1.1\r\nHost: ${HOST}:${port}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`);

    await waitFor(() => closed);
    expect(closed).toBe(true);
    expect(bridge.authorizeMobileRequest).not.toHaveBeenCalled();
  });

  it("taskId나 paneId가 빠지면 400으로 끊는다", async () => {
    const socket = net.connect(port, HOST);
    await new Promise((resolve, reject) => {
      socket.once("connect", resolve);
      socket.once("error", reject);
    });

    let received = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      received += chunk;
    });
    socket.write(
      `GET /api/mobile/stream?taskId=task-1 HTTP/1.1\r\nHost: ${HOST}:${port}\r\n` +
        `Authorization: Bearer good\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`,
    );

    await waitFor(() => received.includes("400"));
    expect(received).toContain("400 Bad Request");
  });

  it("구독이 붙기 전에 기기가 나가면 구독 해제를 곧바로 부른다", async () => {
    let releaseSubscription;
    const stop = vi.fn();
    bridge.subscribeToPane.mockImplementation(
      () =>
        new Promise((resolve) => {
          releaseSubscription = () => resolve(stop);
        }),
    );

    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/stream?taskId=task-1&paneId=pane-1`, {
      headers: { authorization: "Bearer good" },
    });
    await new Promise((resolve, reject) => {
      client.once("open", resolve);
      client.once("error", reject);
    });
    await waitFor(() => Boolean(releaseSubscription));

    const serverSocket = openSockets[openSockets.length - 1];
    const serverSocketClosed = new Promise((resolve) => serverSocket.once("close", resolve));
    client.close();
    await serverSocketClosed;
    await sleep(50);

    releaseSubscription();
    await waitFor(() => stop.mock.calls.length > 0);
    expect(stop).toHaveBeenCalledTimes(1);
  });
});

/**
 * 이 이름은 네트워크에서 오는 값이고 데스크탑 설정 화면에 그대로 그려진다.
 * 본문 상한까지의 문자열이 저장되면 그 항목이 레이아웃을 밀어 끊기 버튼까지 화면 밖으로 보낸다.
 */
describe("기기 이름", () => {
  it("긴 이름은 잘라서 저장한다", async () => {
    await request("/api/mobile/pair", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: "123456", deviceName: "가".repeat(500) }),
    });

    const [, storedName] = bridge.pairMobileDevice.mock.calls[0];
    expect(storedName).toBe("가".repeat(64));
  });

  it("이름이 없으면 기본 이름을 쓴다", async () => {
    await request("/api/mobile/pair", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: "123456" }),
    });

    expect(bridge.pairMobileDevice.mock.calls[0][1]).toBe("모바일 기기");
  });
});

describe("키 입력 순서", () => {
  it("크기 변경이 끝난 화면을 다시 보내 마지막 출력을 보존한다", async () => {
    const resize = vi.fn(async () => "fresh-history-through-180");
    bridge.claimPaneViewport.mockResolvedValue({ resize, release: vi.fn(async () => {}) });
    const client = await openPaneStreamClient("Bearer good");
    const chunks = [];
    client.on("message", (chunk) => chunks.push(chunk.toString()));
    client.send(Buffer.from(JSON.stringify({ type: "resize", cols: 47, rows: 48 })));
    await waitFor(() => chunks.includes("fresh-history-through-180"));
    expect(resize).toHaveBeenCalledWith(47, 48);
    client.close();
  });

  it("binary resize만 크기를 바꾸고 같은 JSON의 text 입력은 그대로 보낸다", async () => {
    const resize = vi.fn(async () => {});
    const release = vi.fn(async () => {});
    bridge.claimPaneViewport.mockResolvedValue({ resize, release });
    const client = await openPaneStreamClient("Bearer good");
    const message = JSON.stringify({ type: "resize", cols: 44, rows: 70 });
    client.send(Buffer.from(message));
    client.send(message);
    await waitFor(() => bridge.writeToPane.mock.calls.length > 0);
    expect(bridge.claimPaneViewport).toHaveBeenCalledWith("task-1", "pane-1");
    expect(resize).toHaveBeenCalledWith(44, 70);
    expect(bridge.writeToPane).toHaveBeenCalledWith("task-1", "pane-1", message);
    client.close();
    await waitFor(() => release.mock.calls.length > 0);
    expect(release).toHaveBeenCalledTimes(1);
  });

  it("binary 이력 갱신 요청은 미러에 전달하고 터미널 입력이나 크기 변경으로 쓰지 않는다", async () => {
    const client = await openPaneStreamClient("Bearer good");
    client.send(Buffer.from(JSON.stringify({ type: "refresh-history" })));
    client.send("done");
    await waitFor(() => bridge.writeToPane.mock.calls.length > 0);
    expect(bridge.refreshPaneHistory).toHaveBeenCalledWith("task-1", "pane-1");
    expect(bridge.claimPaneViewport).not.toHaveBeenCalled();
    expect(bridge.writeToPane.mock.calls.map((call) => call[2])).toEqual(["done"]);
    client.close();
  });

  it("잘못된 binary 크기와 JSON은 터미널 입력으로 보내지 않는다", async () => {
    const client = await openPaneStreamClient("Bearer good");
    for (const message of ["broken-json", ...[0, -1, 1.5, 501, "44"].map((cols) => JSON.stringify({ type: "resize", cols, rows: 70 }))]) {
      client.send(Buffer.from(message));
    }
    client.send("done");
    await waitFor(() => bridge.writeToPane.mock.calls.length > 0);
    expect(bridge.claimPaneViewport).not.toHaveBeenCalled();
    expect(bridge.writeToPane.mock.calls.map((call) => call[2])).toEqual(["done"]);
    client.close();
  });

  /**
   * `writeToPane` 한 번은 DB 왕복과 `sh -c` 프로세스를 지나므로 짧지 않다.
   * 그대로 띄우면 겹친 두 입력의 도착 순서를 OS 스케줄러가 정해 `ls`가 `sl`로 들어간다.
   */
  it("앞선 쓰기가 끝나기 전에는 다음 키를 보내지 않는다", async () => {
    const completedWrites = [];
    const pendingWrites = [];
    bridge.writeToPane.mockImplementation(
      (taskId, paneId, input) =>
        new Promise((resolve) => {
          pendingWrites.push(() => {
            completedWrites.push(input);
            resolve();
          });
        }),
    );

    const client = await openPaneStreamClient("Bearer good");
    client.send("l");
    client.send("s");

    await waitFor(() => pendingWrites.length === 1);
    expect(bridge.writeToPane).toHaveBeenCalledTimes(1);

    pendingWrites[0]();
    await waitFor(() => pendingWrites.length === 2);
    pendingWrites[1]();
    await waitFor(() => completedWrites.length === 2);

    expect(completedWrites).toEqual(["l", "s"]);
    client.close();
  });

  it("한 번의 쓰기가 실패해도 다음 키가 막히지 않는다", async () => {
    const receivedInputs = [];
    bridge.writeToPane.mockImplementation(async (taskId, paneId, input) => {
      receivedInputs.push(input);
      if (input === "l") {
        throw new Error("send-keys 실패");
      }
    });

    const client = await openPaneStreamClient("Bearer good");
    client.send("l");
    client.send("s");

    await waitFor(() => receivedInputs.length === 2);
    expect(receivedInputs).toEqual(["l", "s"]);
    client.close();
  });
});

describe("서버 쪽 스트림이 죽는 경우", () => {
  it("스트림이 죽으면 구독 시작 실패와 같은 코드로 소켓을 닫는다", async () => {
    const paneChunks = stubPaneChunks();
    const client = await openPaneStreamClient("Bearer good");
    await waitFor(() => bridge.subscribeToPane.mock.calls.length === 1);

    const closed = waitForClose(client);
    paneChunks.endAll();

    expect(await closed).toEqual({ code: 4000, reason: "stream-failed" });
  });
});

/**
 * 목록에서 지운 기기라도 이미 맺어진 소켓은 다시 인증을 지나지 않는다.
 * 그래서 연결을 끊었다는 말이 참이 되려면 그 기기의 소켓까지 닫아야 한다.
 */
describe("연결 해제와 열려 있는 스트림", () => {
  it("해제된 기기의 스트림은 해제 코드로 닫힌다", async () => {
    const client = await openPaneStreamClient("Bearer good");
    await waitFor(() => bridge.subscribeToPane.mock.calls.length === 1);

    const closed = waitForClose(client);
    notifyUnpaired("device-good");

    expect(await closed).toEqual({ code: 4001, reason: "device-unpaired" });
  });

  it("같은 pane을 보는 다른 기기의 스트림은 그대로 흐른다", async () => {
    const paneChunks = stubPaneChunks();
    const removedClient = await openPaneStreamClient("Bearer good");
    const keptClient = await openPaneStreamClient("Bearer second");
    await waitFor(() => bridge.subscribeToPane.mock.calls.length === 2);

    const receivedChunks = [];
    keptClient.on("message", (chunk) => receivedChunks.push(chunk.toString()));

    const closed = waitForClose(removedClient);
    notifyUnpaired("device-good");
    await closed;

    paneChunks.emit("남은 기기가 볼 출력");
    await waitFor(() => receivedChunks.length > 0);
    expect(receivedChunks).toEqual(["남은 기기가 볼 출력"]);
    expect(keptClient.readyState).toBe(WebSocket.OPEN);
  });

  it("지운 기기가 없는 해제 신호는 어떤 소켓도 닫지 않는다", async () => {
    const client = await openPaneStreamClient("Bearer good");
    await waitFor(() => bridge.subscribeToPane.mock.calls.length === 1);

    notifyUnpaired("device-unknown");
    await sleep(50);

    expect(client.readyState).toBe(WebSocket.OPEN);
  });

  it("정상적으로 닫힌 소켓은 지도에서 빠져 나중의 해제가 건드리지 않는다", async () => {
    const client = await openPaneStreamClient("Bearer good");
    await waitFor(() => streamServer.clients.size === 1);
    const [serverSocket] = streamServer.clients;
    const closeSpy = vi.spyOn(serverSocket, "close");

    client.close();
    await waitFor(() => serverSocket.readyState === WebSocket.CLOSED);
    closeSpy.mockClear();

    notifyUnpaired("device-good");
    await sleep(50);

    expect(closeSpy).not.toHaveBeenCalled();
  });
});

/**
 * 개발 실행에서 Maestro가 데스크탑 화면 없이 페어링 코드를 받아 가는 경로.
 * 빌드한 앱과 다른 기기에는 이 경로가 있다는 사실조차 보이면 안 되므로, 막힐 때는 모르는 경로와 똑같이 답해야 한다.
 */
describe("개발용 페어링 코드 발급 경로", () => {
  async function restartServer(options) {
    for (const socket of openSockets) {
      socket.destroy();
    }
    await new Promise((resolve) => server.close(resolve));
    server = await startServer(options);
  }

  it("개발 실행에서 이 머신이 부르면 코드를 발급해 돌려준다", async () => {
    await restartServer({ isDevPairingRouteEnabled: true });

    const response = await request("/api/mobile/dev/pairing-code", { method: "POST" });

    expect(response.status).toBe(200);
    expect(response.body).toEqual({ success: true, code: "654321", expiresAt: 1_700_000_000_000 });
    expect(bridge.startMobilePairing).toHaveBeenCalledTimes(1);
  });

  it("빌드한 앱에서는 모르는 경로와 같은 응답을 주고 코드를 발급하지 않는다", async () => {
    const unknownPath = await request("/api/mobile/unknown", { method: "POST" });
    const devPath = await request("/api/mobile/dev/pairing-code", { method: "POST" });

    expect(devPath).toEqual(unknownPath);
    expect(bridge.startMobilePairing).not.toHaveBeenCalled();
  });

  it("개발 실행이라도 다른 기기가 부르면 모르는 경로와 같은 응답을 주고 코드를 발급하지 않는다", async () => {
    const lanRequest = { url: "/api/mobile/dev/pairing-code", method: "POST", headers: {}, socket: { remoteAddress: "192.168.0.24" } };
    const unknownRequest = { ...lanRequest, url: "/api/mobile/unknown" };
    const devResponse = { writeHead: vi.fn(), end: vi.fn() };
    const unknownResponse = { writeHead: vi.fn(), end: vi.fn() };

    await handleMobileRequest(lanRequest, devResponse, { bridge, host: HOST, port, isDevPairingRouteEnabled: true });
    await handleMobileRequest(unknownRequest, unknownResponse, { bridge, host: HOST, port, isDevPairingRouteEnabled: true });

    expect(devResponse.writeHead.mock.calls).toEqual(unknownResponse.writeHead.mock.calls);
    expect(devResponse.end.mock.calls).toEqual(unknownResponse.end.mock.calls);
    expect(bridge.startMobilePairing).not.toHaveBeenCalled();
  });
});

/** 서비스가 던지는 값 오류. 이름으로만 가려내므로 라우팅이 보는 모양 그대로 만든다 */
function inputError(message) {
  const error = new Error(message);
  error.name = "MobileTaskInputError";
  return error;
}

describe("태스크 생성 경로", () => {
  const draft = { projectId: "project-1", branchName: "feat/from-mobile" };

  function createTask(body, headers = { Authorization: "Bearer good" }) {
    return request("/api/mobile/tasks", { method: "POST", headers, body: JSON.stringify(body) });
  }

  it("연결된 기기가 보낸 초안을 서비스로 넘기고 만들어진 태스크를 돌려준다", async () => {
    const response = await createTask(draft);

    expect(response.status).toBe(200);
    expect(response.body.task).toEqual({ id: "task-1", title: "feat/from-mobile" });
    expect(bridge.createMobileTask).toHaveBeenCalledTimes(1);
    expect(bridge.createMobileTask.mock.calls[0][0]).toMatchObject(draft);
  });

  it("선택한 세션 타입을 생성 서비스로 전달한다", async () => {
    await createTask({ ...draft, sessionType: "tmux" });

    expect(bridge.createMobileTask.mock.calls[0][0]).toHaveProperty("sessionType", "tmux");
  });

  it("토큰 없이는 인증 앞단에서 막혀 아무것도 만들지 않는다", async () => {
    const response = await createTask(draft, {});

    expect(response.status).toBe(401);
    expect(bridge.createMobileTask).not.toHaveBeenCalled();
  });

  it("프로젝트나 브랜치 이름이 없으면 서비스를 부르지 않고 400을 준다", async () => {
    expect((await createTask({ branchName: "feat/x" })).status).toBe(400);
    expect((await createTask({ projectId: "project-1" })).status).toBe(400);
    expect((await createTask({ ...draft, branchName: "   " })).status).toBe(400);

    expect(bridge.createMobileTask).not.toHaveBeenCalled();
  });

  it("본문이 JSON이 아니면 400으로 거절한다", async () => {
    const response = await createTask("깨진 본문");

    expect(response.status).toBe(400);
    expect(bridge.createMobileTask).not.toHaveBeenCalled();
  });

  it("값이 잘못됐다는 서비스 오류는 사유를 그대로 돌려준다", async () => {
    bridge.createMobileTask.mockRejectedValue(inputError("브랜치 이름에 쓸 수 없는 문자가 있습니다"));

    const response = await createTask(draft);

    expect(response.status).toBe(400);
    expect(response.body.error).toBe("브랜치 이름에 쓸 수 없는 문자가 있습니다");
  });

  it("그 밖의 예외는 내부 메시지를 감춘 500으로 나간다", async () => {
    bridge.createMobileTask.mockRejectedValue(new Error("git -C /home/me/repo worktree add 실패"));

    const response = await createTask(draft);

    expect(response.status).toBe(500);
    expect(response.body.error).toBe("서버 오류");
  });
});

describe("태스크 상태 이동 경로", () => {
  function moveTask(body, headers = { Authorization: "Bearer good" }) {
    return request("/api/mobile/tasks/task-1/status", { method: "PATCH", headers, body: JSON.stringify(body) });
  }

  it("아는 상태를 보내면 서비스로 넘기고 옮겨진 태스크를 돌려준다", async () => {
    const response = await moveTask({ status: "review" });

    expect(response.status).toBe(200);
    expect(response.body.task).toEqual({ id: "task-1", status: "review" });
    expect(bridge.updateMobileTaskStatus).toHaveBeenCalledWith("task-1", "review");
  });

  it("토큰 없이는 인증 앞단에서 막혀 아무것도 옮기지 않는다", async () => {
    const response = await moveTask({ status: "review" }, {});

    expect(response.status).toBe(401);
    expect(bridge.updateMobileTaskStatus).not.toHaveBeenCalled();
  });

  it("상태가 없으면 서비스를 부르지 않고 400을 준다", async () => {
    expect((await moveTask({})).status).toBe(400);
    expect(bridge.updateMobileTaskStatus).not.toHaveBeenCalled();
  });

  it("인증과 이동 사이에 지워진 태스크는 404로 답한다", async () => {
    bridge.updateMobileTaskStatus.mockResolvedValue(null);

    expect((await moveTask({ status: "done" })).status).toBe(404);
  });

  it("모르는 상태라는 서비스 오류는 사유를 그대로 돌려준다", async () => {
    bridge.updateMobileTaskStatus.mockRejectedValue(inputError("상태 값이 올바르지 않습니다"));

    const response = await moveTask({ status: "archived" });

    expect(response.status).toBe(400);
    expect(response.body.error).toBe("상태 값이 올바르지 않습니다");
  });
});

describe("태스크 세션 타입 변경 경로", () => {
  function changeSessionType(body, headers = { Authorization: "Bearer good" }) {
    return request("/api/mobile/tasks/task-1/session-type", { method: "PATCH", headers, body: JSON.stringify(body) });
  }

  it("세션 타입과 기존 세션 닫기 여부를 서비스로 넘기고 바뀐 태스크를 돌려준다", async () => {
    const response = await changeSessionType({ sessionType: "zellij", shouldCloseExistingSession: true });

    expect(response.status).toBe(200);
    expect(response.body.task).toEqual({ id: "task-1", sessionType: "zellij" });
    expect(bridge.changeMobileTaskSessionType).toHaveBeenCalledWith("task-1", "zellij", true);
  });

  it("닫기 여부가 참이 아니면 기존 세션을 남기는 쪽으로 넘긴다", async () => {
    await changeSessionType({ sessionType: "zellij", shouldCloseExistingSession: "yes" });

    expect(bridge.changeMobileTaskSessionType).toHaveBeenCalledWith("task-1", "zellij", false);
  });

  it("토큰 없이는 인증 앞단에서 막혀 바꾸지 않는다", async () => {
    expect((await changeSessionType({ sessionType: "zellij" }, {})).status).toBe(401);
    expect(bridge.changeMobileTaskSessionType).not.toHaveBeenCalled();
  });

  it("세션 타입이 없으면 서비스를 부르지 않고 400을 준다", async () => {
    expect((await changeSessionType({ shouldCloseExistingSession: true })).status).toBe(400);
    expect(bridge.changeMobileTaskSessionType).not.toHaveBeenCalled();
  });

  it("서비스가 값을 거절하면 400으로 이유를 전한다", async () => {
    bridge.changeMobileTaskSessionType.mockRejectedValue(inputError("세션 타입 값이 올바르지 않습니다"));

    const response = await changeSessionType({ sessionType: "screen" });

    expect(response.status).toBe(400);
    expect(response.body.error).toBe("세션 타입 값이 올바르지 않습니다");
  });

  it("새 세션 도구가 없으면 409로 데스크탑에서 설치하라는 이유를 전한다", async () => {
    const dependencyError = new Error("zellij이(가) 설치되어 있지 않습니다. 데스크탑에서 설치한 뒤 다시 시도해 주세요.");
    dependencyError.name = "MobileSessionDependencyMissingError";
    bridge.changeMobileTaskSessionType.mockRejectedValue(dependencyError);

    const response = await changeSessionType({ sessionType: "zellij" });

    expect(response.status).toBe(409);
    expect(response.body.error).toContain("데스크탑에서 설치");
  });

  it("없는 태스크는 404로 답한다", async () => {
    bridge.changeMobileTaskSessionType.mockResolvedValue(null);

    expect((await changeSessionType({ sessionType: "zellij" })).status).toBe(404);
    expect(bridge.changeMobileTaskSessionType).toHaveBeenCalledWith("task-1", "zellij", false);
  });

  it("기존 세션이 켜져 있는지 알려 준다", async () => {
    bridge.isMobileTaskSessionAlive.mockResolvedValue(false);

    const response = await request("/api/mobile/tasks/task-1/session-alive", { headers: { Authorization: "Bearer good" } });

    expect(response.status).toBe(200);
    expect(response.body).toEqual({ success: true, isAlive: false });
    expect(bridge.isMobileTaskSessionAlive).toHaveBeenCalledWith("task-1");
  });
});

describe("세션이 바뀐 태스크의 열린 스트림", () => {
  it("이 태스크의 세션이 바뀌면 세션 변경 코드로 닫아 기기가 새 세션을 다시 받게 한다", async () => {
    const client = await openPaneStreamClient("Bearer good");
    await waitFor(() => bridge.subscribeToPane.mock.calls.length === 1);

    const closed = waitForClose(client);
    notifyTaskSessionChanged("task-1");

    expect(await closed).toEqual({ code: 4002, reason: "session-changed" });
  });

  it("다른 태스크의 세션 변경에는 닫히지 않고, 닫힌 스트림은 더 이상 지켜보지 않는다", async () => {
    const client = await openPaneStreamClient("Bearer good");
    await waitFor(() => bridge.subscribeToPane.mock.calls.length === 1);

    notifyTaskSessionChanged("task-2");
    await sleep(50);
    expect(client.readyState).toBe(WebSocket.OPEN);

    const closed = waitForClose(client);
    client.close();
    await closed;
    await waitFor(() => sessionChangeWatchers.size === 0);
  });
});

describe("프로젝트 브랜치 목록 경로", () => {
  it("연결된 기기에는 브랜치 목록을 준다", async () => {
    const response = await request("/api/mobile/projects/project-1/branches", {
      headers: { Authorization: "Bearer good" },
    });

    expect(response.status).toBe(200);
    expect(response.body.branches).toEqual(["main", "dev"]);
    expect(bridge.getMobileProjectBranches).toHaveBeenCalledWith("project-1");
  });

  it("토큰 없이는 인증 앞단에서 막힌다", async () => {
    const response = await request("/api/mobile/projects/project-1/branches");

    expect(response.status).toBe(401);
    expect(bridge.getMobileProjectBranches).not.toHaveBeenCalled();
  });

  it("브랜치 조회가 던져도 매달리지 않고 500을 돌려준다", async () => {
    bridge.getMobileProjectBranches.mockRejectedValue(new Error("git 실패"));

    const response = await request("/api/mobile/projects/project-1/branches", {
      headers: { Authorization: "Bearer good" },
    });

    expect(response.status).toBe(500);
  });
});


describe("모바일 pane 단축키 API", () => {
  const shortcutPath = "/api/mobile/tasks/task%20id/panes/%2519/shortcuts";

  it("인증 없는 단축키는 실행하지 않는다", async () => {
    const response = await request(shortcutPath, { method: "POST", body: JSON.stringify({ action: "split-right" }) });
    expect(response.status).toBe(401);
    expect(bridge.executeMobileTerminalShortcut).not.toHaveBeenCalled();
  });

  it("경로의 대상과 action만 서비스로 보내고 화면을 반환한다", async () => {
    const response = await request(shortcutPath, {
      method: "POST", headers: { authorization: "Bearer good" },
      body: JSON.stringify({ action: "split-right", command: "injected", sshHost: "injected" }),
    });
    expect(bridge.executeMobileTerminalShortcut).toHaveBeenCalledExactlyOnceWith("task id", "%19", "split-right");
    expect(response.status).toBe(200);
    expect(response.body).toEqual({ success: true, surfaces: { tabs: [] }, screen: "screen" });
  });

  it.each([null, {}, { action: 1 }, { action: "" }])("action이 없는 본문 %s는 400으로 거절한다", async (body) => {
    const response = await request(shortcutPath, {
      method: "POST", headers: { authorization: "Bearer good" }, body: JSON.stringify(body),
    });
    expect(response.status).toBe(400);
    expect(bridge.executeMobileTerminalShortcut).not.toHaveBeenCalled();
  });

  it("서비스가 거절한 action은 400으로 반환한다", async () => {
    bridge.executeMobileTerminalShortcut.mockRejectedValue(Object.assign(new Error("지원하지 않는 단축키입니다"), { name: "MobileTaskInputError" }));
    const response = await request(shortcutPath, {
      method: "POST", headers: { authorization: "Bearer good" }, body: JSON.stringify({ action: "unknown" }),
    });
    expect(response.status).toBe(400);
  });

  it("다른 세션 pane은 409로 거절한다", async () => {
    bridge.executeMobileTerminalShortcut.mockRejectedValue(Object.assign(new Error("missing"), { name: "SurfaceUnavailableError", reason: "session-not-running" }));
    const response = await request(shortcutPath, {
      method: "POST", headers: { authorization: "Bearer good" }, body: JSON.stringify({ action: "split-right" }),
    });
    expect(response.status).toBe(409);
    expect(response.body).toEqual({ success: false, reason: "session-not-running" });
  });

  it("실패한 CLI 명령을 응답에 노출하지 않는다", async () => {
    bridge.executeMobileTerminalShortcut.mockRejectedValue(new Error("ssh secret-host secret-path"));
    const response = await request(shortcutPath, {
      method: "POST", headers: { authorization: "Bearer good" }, body: JSON.stringify({ action: "split-right" }),
    });
    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
  });
});

describe("모바일 알림 상태 설정", () => {
  const headers = { Authorization: "Bearer good" };
  it("인증한 PC의 알림 상태 설정을 조회한다", async () => {
    expect(await request("/api/mobile/notification-settings", { headers })).toEqual({
      status: 200, body: { success: true, settings: { isEnabled: true, enabledStatuses: ["review"] } },
    });
  });
  it.each(["GET", "PATCH"])("%s는 인증 없이 설정을 읽거나 바꾸지 못한다", async (method) => {
    expect((await request("/api/mobile/notification-settings", { method })).status).toBe(401);
    expect(bridge.getMobileNotificationSettings).not.toHaveBeenCalled();
    expect(bridge.updateMobileNotificationSettings).not.toHaveBeenCalled();
  });
  it("설정을 저장하고 저장된 값을 반환한다", async () => {
    const settings = { isEnabled: false, enabledStatuses: [] };
    const response = await request("/api/mobile/notification-settings", { method: "PATCH", headers, body: JSON.stringify(settings) });
    expect(response).toEqual({ status: 200, body: { success: true, settings } });
    expect(bridge.updateMobileNotificationSettings).toHaveBeenCalledWith(settings);
  });
  it("잘못된 상태와 JSON은 400으로 거절한다", async () => {
    bridge.updateMobileNotificationSettings.mockRejectedValue(Object.assign(new Error("잘못된 상태"), { name: "MobileTaskInputError" }));
    expect((await request("/api/mobile/notification-settings", { method: "PATCH", headers, body: '{"isEnabled":true}' })).status).toBe(400);
    expect((await request("/api/mobile/notification-settings", { method: "PATCH", headers, body: 'invalid' })).status).toBe(400);
  });
});

describe("모바일 AI History", () => {
  it("목록과 상세 모두 인증을 요구한다", async () => {
    for (const path of ["/api/mobile/tasks/task-1/ai-sessions", "/api/mobile/tasks/task-1/ai-sessions/hermes/session-1"]) {
      expect((await request(path)).status).toBe(401);
    }
    expect(bridge.getMobileAiSessions).not.toHaveBeenCalled();
  });

  it("목록의 검색과 페이지 값을 서비스에 전달한다", async () => {
    const result = await request("/api/mobile/tasks/task-1/ai-sessions?query=fix&cursor=20&limit=10", { headers: { Authorization: "Bearer good" } });
    expect(result.status).toBe(200);
    expect(result.body.history.sessions[0]).toEqual({ id: "session-1", provider: "hermes" });
    expect(bridge.getMobileAiSessions).toHaveBeenCalledWith("task-1", { query: "fix", cursor: "20", limit: 10 });
  });

  it("상세는 역할 필터를 전달하고 서버의 대화를 돌려준다", async () => {
    const result = await request("/api/mobile/tasks/task-1/ai-sessions/hermes/session-1?roles=user,assistant", { headers: { Authorization: "Bearer good" } });
    expect(result.status).toBe(200);
    expect(result.body.detail.messages[0].fullText).toBe("기록 본문");
    expect(bridge.getMobileAiSessionDetail).toHaveBeenCalledWith("task-1", "hermes", "session-1", { query: undefined, cursor: undefined, limit: 20, roles: ["user", "assistant"] });
  });

  it.each(["limit=0", "limit=101", "limit=2x", "cursor=-1", "cursor=garbage", "roles=invalid", "sourceRef=/private/file"])("잘못된 조회 값 %s를 거절한다", async (query) => {
    const result = await request(`/api/mobile/tasks/task-1/ai-sessions/hermes/session-1?${query}`, { headers: { Authorization: "Bearer good" } });
    expect(result.status).toBe(400);
    expect(bridge.getMobileAiSessionDetail).not.toHaveBeenCalled();
  });

  it("없는 Agent는 400, 없는 태스크나 세션은 404다", async () => {
    const headers = { Authorization: "Bearer good" };
    expect((await request("/api/mobile/tasks/task-1/ai-sessions/unknown/s", { headers })).status).toBe(400);
    bridge.getMobileTask.mockResolvedValueOnce(null);
    expect((await request("/api/mobile/tasks/deleted/ai-sessions", { headers })).status).toBe(404);
    bridge.getMobileAiSessionDetail.mockResolvedValueOnce(null);
    expect((await request("/api/mobile/tasks/task-1/ai-sessions/hermes/deleted", { headers })).status).toBe(404);
  });
});

it("실제 Codex fixture 리더의 목록과 상세가 HTTP 계약으로 이어진다", async () => {
  const { mkdtemp, mkdir, writeFile, rm } = await import("node:fs/promises");
  const { tmpdir } = await import("node:os");
  const path = await import("node:path");
  const home = await mkdtemp(path.join(tmpdir(), "mobile-chat-contract-"));
  vi.stubEnv("HOME", home);
  try {
    const { readCodexSessions, readCodexSessionDetail } = await import("../src/lib/aiSessions/readCodexSessions.ts");
    const directory = path.join(home, ".codex", "sessions");
    await mkdir(directory, { recursive: true });
    const sourceRef = path.join(directory, "fixture.jsonl");
    const context = { worktreePath: "/qa/chat/repo", repoPath: "/qa/chat/repo" };
    await writeFile(sourceRef, [
      { type: "session_meta", payload: { id: "wire-session", cwd: context.worktreePath } },
      { type: "response_item", timestamp: "2026-11-02T01:00:00Z", payload: { type: "message", role: "user", content: [{ type: "input_text", text: "모바일 계약 질문" }] } },
      { type: "response_item", timestamp: "2026-11-02T01:00:01Z", payload: { type: "message", role: "assistant", content: [{ type: "output_text", text: "실제 fixture 응답" }] } },
    ].map((entry) => JSON.stringify(entry)).join("\n") + "\n");
    bridge.getMobileAiSessions.mockImplementation(async (_taskId, options) => {
      const result = await readCodexSessions({ ...context, ...options });
      return { sessions: result.sessions.map(({ sourceRef: _, ...entry }) => entry), sources: [{ provider: result.provider, available: result.available }], nextCursor: null };
    });
    bridge.getMobileAiSessionDetail.mockImplementation(async (_taskId, provider, sessionId, options) => {
      const detail = await readCodexSessionDetail({ ...context, roles: options.roles }, sessionId, sourceRef, options.cursor, options.limit);
      const { sourceRef: _, ...safeDetail } = detail;
      return safeDetail;
    });
    const headers = { Authorization: "Bearer good" };
    const list = await request("/api/mobile/tasks/task-1/ai-sessions", { headers });
    expect(list.status).toBe(200);
    expect(list.body.history.sessions[0]).toMatchObject({ id: "wire-session", provider: "codex", title: "모바일 계약 질문" });
    expect(list.body.history.sessions[0]).not.toHaveProperty("sourceRef");
    const detail = await request("/api/mobile/tasks/task-1/ai-sessions/codex/wire-session?roles=assistant", { headers });
    expect(detail.status).toBe(200);
    expect(detail.body.detail.messages.map((entry) => entry.fullText)).toEqual(["실제 fixture 응답"]);
  } finally {
    vi.unstubAllEnvs();
    await rm(home, { recursive: true, force: true });
  }
});


describe("모바일 프로젝트 폴더 등록", () => {
  const headers = { Authorization: "Bearer good" };
  const scan = (body, requestHeaders = headers) => request("/api/mobile/projects/scan", {
    method: "POST", headers: requestHeaders, body: JSON.stringify(body),
  });

  it("연결된 PC의 홈 폴더부터 하위 폴더 이름을 조회한다", async () => {
    const response = await request("/api/mobile/directories", { headers });
    expect(response).toEqual({ status: 200, body: { success: true, directories: ["Documents", "Projects"] } });
    expect(bridge.getMobileDirectories).toHaveBeenCalledWith("~");
    await request("/api/mobile/directories?path=%2Fworkspace%2Fspace%20repo", { headers });
    expect(bridge.getMobileDirectories).toHaveBeenLastCalledWith("/workspace/space repo");
  });

  it("인증 없이는 폴더 조회와 프로젝트 등록을 모두 막는다", async () => {
    expect((await request("/api/mobile/directories")).status).toBe(401);
    expect((await scan({ rootPath: "/workspace" }, {})).status).toBe(401);
    expect(bridge.getMobileDirectories).not.toHaveBeenCalled();
    expect(bridge.scanMobileProjects).not.toHaveBeenCalled();
  });

  it("선택한 폴더만 기존 스캔 서비스에 넘기고 등록 결과를 반환한다", async () => {
    const response = await scan({ rootPath: "~/Projects" });
    expect(response.status).toBe(200);
    expect(response.body.result.registered).toEqual(["kanvibe"]);
    expect(bridge.scanMobileProjects).toHaveBeenCalledWith("~/Projects");
  });

  it.each([{}, { rootPath: "" }, { rootPath: "   " }, { rootPath: 42 }, null, []])("등록 경로 없는 본문 %j를 거절한다", async (body) => {
    expect((await scan(body)).status).toBe(400);
    expect(bridge.scanMobileProjects).not.toHaveBeenCalled();
  });

  it("잘못된 JSON은 서비스를 부르지 않고 거절한다", async () => {
    const response = await request("/api/mobile/projects/scan", { method: "POST", headers, body: "{" });
    expect(response.status).toBe(400);
    expect(bridge.scanMobileProjects).not.toHaveBeenCalled();
  });

  it("이미 등록된 폴더는 재시도해도 skipped 결과를 그대로 보여 준다", async () => {
    bridge.scanMobileProjects.mockResolvedValue({ registered: [], skipped: ["/workspace/kanvibe"], errors: [], worktreeTasks: [], registeredWorktrees: [], hooksSetup: [] });
    expect((await scan({ rootPath: "/workspace" })).body.result.skipped).toEqual(["/workspace/kanvibe"]);
  });

  it("없는 폴더는 서비스가 제공한 안전한 안내로 응답한다", async () => {
    bridge.getMobileDirectories.mockRejectedValue(inputError("폴더를 찾을 수 없습니다"));
    const response = await request("/api/mobile/directories?path=%2Fmissing", { headers });
    expect(response).toEqual({ status: 400, body: { success: false, error: "폴더를 찾을 수 없습니다" } });
  });

  it("내부 스캔 오류 메시지는 네트워크 응답에서 숨긴다", async () => {
    bridge.scanMobileProjects.mockRejectedValue(new Error("secret filesystem path"));
    const response = await scan({ rootPath: "/workspace" });
    expect(response).toEqual({ status: 500, body: { success: false, error: "서버 오류" } });
  });
});

describe("현재 pane 채팅 인증 및 대상 경계", () => {
  const headers = { authorization: "Bearer good", "Content-Type": "application/json" };
  beforeEach(() => {
    bridge.getSequencedPaneChatSnapshot = vi.fn(async () => ({ taskId: "task-1", paneId: "%3", canSend: true, inputState: "ready", targetToken: "opaque-token" }));
    bridge.sendPaneChatPrompt = vi.fn(async () => ({ success: true }));
    bridge.interruptPaneChat = vi.fn(async () => ({ success: true }));
  });
  it("인증된 기기는 요청한 정확한 pane 스냅샷을 읽는다", async () => {
    const result = await request("/api/mobile/tasks/task-1/pane-chat?paneId=%253", { headers });
    expect(result).toMatchObject({ status: 200, body: { success: true, snapshot: { paneId: "%3", targetToken: "opaque-token" } } });
    expect(bridge.getSequencedPaneChatSnapshot).toHaveBeenCalledWith("task-1", "%3");
  });
  it("토큰 없는 기기는 스냅샷과 메시지 전송에 접근할 수 없다", async () => {
    expect((await request("/api/mobile/tasks/task-1/pane-chat")).status).toBe(401);
    expect((await request("/api/mobile/tasks/task-1/pane-chat/prompt", { method: "POST", body: JSON.stringify({ targetToken: "opaque-token", text: "Hi" }) })).status).toBe(401);
    expect(bridge.getSequencedPaneChatSnapshot).not.toHaveBeenCalled();
    expect(bridge.sendPaneChatPrompt).not.toHaveBeenCalled();
  });
  it("인증과 대상 토큰을 보존해 메시지를 서비스에 전달한다", async () => {
    const result = await request("/api/mobile/tasks/task-1/pane-chat/prompt", { method: "POST", headers, body: JSON.stringify({ targetToken: "opaque-token", text: "First\nSecond" }) });
    expect(result).toEqual({ status: 200, body: { success: true } });
    expect(bridge.sendPaneChatPrompt).toHaveBeenCalledWith("task-1", "opaque-token", "First\nSecond");
  });
  it("모델 설정 변경은 대상 토큰과 변경 내용을 그대로 넘기고 거절은 400으로 알린다", async () => {
    const headers = { authorization: "Bearer good" };
    bridge.changePaneChatModelSetting = vi.fn(async () => ({ success: true }));
    const change = { kind: "effort", effort: "high" };
    const send = (requestHeaders) => request("/api/mobile/tasks/task-1/pane-chat/model-setting", { method: "POST", headers: requestHeaders, body: JSON.stringify({ targetToken: "opaque-token", change }) });
    expect(await send(headers)).toEqual({ status: 200, body: { success: true } });
    expect(bridge.changePaneChatModelSetting).toHaveBeenCalledWith("task-1", "opaque-token", change);
    bridge.changePaneChatModelSetting.mockRejectedValue(Object.assign(new Error("지금은 이 설정을 바꿀 수 없습니다."), { name: "PaneChatInputError" }));
    expect(await send(headers)).toEqual({ status: 400, body: { success: false, error: "지금은 이 설정을 바꿀 수 없습니다." } });
    expect((await send(undefined)).status).toBe(401);
  });

  it("대상이 교체되면 충돌을 반환하고 빈 토큰은 거부한다", async () => {
    bridge.interruptPaneChat.mockRejectedValue(Object.assign(new Error("대상 변경"), { name: "PaneChatTargetChangedError" }));
    expect(await request("/api/mobile/tasks/task-1/pane-chat/interrupt", { method: "POST", headers, body: JSON.stringify({ targetToken: "stale-token" }) })).toEqual({ status: 409, body: { success: false, error: "대상 변경", reason: "target-changed" } });
    expect((await request("/api/mobile/tasks/task-1/pane-chat/prompt", { method: "POST", headers, body: JSON.stringify({ text: "Hi" }) })).status).toBe(400);
    expect(bridge.sendPaneChatPrompt).not.toHaveBeenCalled();
  });
});

describe("채팅 composer 첨부와 자동완성 HTTP", () => {
  const headers = { authorization: "Bearer good", "Content-Type": "application/json" };
  beforeEach(() => {
    bridge.stagePaneChatAttachment = vi.fn(async () => ({ id: "owned-id", name: "file.txt", mimeType: "text/plain", kind: "file", sizeBytes: 65537 }));
    bridge.removePaneChatAttachment = vi.fn(async () => ({ success: true }));
    bridge.getPaneChatSuggestions = vi.fn(async () => ({ targetToken: "token", items: [{ id: "skill:review", label: "review", description: "Review", insertText: "$review" }], truncated: false }));
    bridge.sendPaneChatPrompt = vi.fn(async () => ({ success: true }));
    bridge.getMobileAiSessionAttachment = vi.fn(async () => ({ id: "image-id", name: "image.png", mimeType: "image/png", dataUrl: "data:image/png;base64,aGVsbG8=" }));
    bridge.isMobileAiSessionProvider = vi.fn((provider) => ["claude", "codex"].includes(provider));
  });
  it("인증 업로드만 기존 64KiB를 넘는 첨부 본문을 읽고 토큰을 보존한다", async () => {
    const attachment = { name: "file.txt", mimeType: "text/plain", dataUrl: `data:text/plain;base64,${Buffer.alloc(65537).toString("base64")}` };
    const result = await request("/api/mobile/tasks/task-1/pane-chat/attachments", { method: "POST", headers, body: JSON.stringify({ targetToken: "token", attachment }) });
    expect(result).toMatchObject({ status: 200, body: { success: true, attachment: { id: "owned-id" } } });
    expect(bridge.stagePaneChatAttachment).toHaveBeenCalledWith("task-1", "token", attachment);
    expect((await request("/api/mobile/tasks/task-1/pane-chat/attachments", { method: "POST", body: JSON.stringify({ targetToken: "token", attachment }) })).status).toBe(401);
    expect(bridge.stagePaneChatAttachment).toHaveBeenCalledTimes(1);
  });
  it("첨부 삭제와 검색에 같은 대상 토큰을 사용한다", async () => {
    const body = JSON.stringify({ targetToken: "token", id: "owned-id" });
    expect((await request("/api/mobile/tasks/task-1/pane-chat/attachments", { method: "DELETE", headers: { ...headers, "Content-Length": Buffer.byteLength(body) }, body })).status).toBe(200);
    expect(bridge.removePaneChatAttachment).toHaveBeenCalledWith("task-1", "token", "owned-id");
    const result = await request("/api/mobile/tasks/task-1/pane-chat/suggestions?targetToken=token&kind=skill&query=review", { headers });
    expect(result).toMatchObject({ status: 200, body: { targetToken: "token", items: [{ insertText: "$review" }] } });
    expect(bridge.getPaneChatSuggestions).toHaveBeenCalledWith("task-1", "token", "skill", "review");
  });
  it("첨부 ID와 중복 방지 식별자를 메시지와 함께 전달한다", async () => {
    await request("/api/mobile/tasks/task-1/pane-chat/prompt", { method: "POST", headers, body: JSON.stringify({ targetToken: "token", text: "caption", attachmentIds: ["owned-id"], requestId: "once" }) });
    expect(bridge.sendPaneChatPrompt).toHaveBeenCalledWith("task-1", "token", "caption", ["owned-id"], "once");
  });
  it("활성 CLI 없이 인증된 히스토리 첨부만 가져오고 누락 파일을 구분한다", async () => {
    const url = "/api/mobile/tasks/task-1/ai-sessions/codex/session-1/attachments/image-id";
    expect((await request(url)).status).toBe(401);
    expect(bridge.getMobileAiSessionAttachment).not.toHaveBeenCalled();
    expect((await request(url, { headers })).body.attachment).toMatchObject({ id: "image-id", mimeType: "image/png" });
    expect(bridge.getMobileAiSessionAttachment).toHaveBeenCalledWith("task-1", "codex", "session-1", "image-id", undefined);
    bridge.getMobileAiSessionAttachment.mockResolvedValue(null);
    expect((await request(url, { headers })).status).toBe(404);
  });
  it("연결된 기기만 태스크 작업 폴더의 파일을 읽고 경로는 디코딩해 넘긴다", async () => {
    bridge.getMobileTaskFile = vi.fn(async () => ({ path: "docs/새 계획.md", content: "# plan" }));
    const url = `/api/mobile/tasks/task-1/workspace-file?path=${encodeURIComponent("/work/task/docs/새 계획.md")}`;
    expect((await request(url)).status).toBe(401);
    expect(bridge.getMobileTaskFile).not.toHaveBeenCalled();
    expect(await request(url, { headers })).toEqual({ status: 200, body: { success: true, file: { path: "docs/새 계획.md", content: "# plan" } } });
    expect(bridge.getMobileTaskFile).toHaveBeenCalledWith("task-1", "/work/task/docs/새 계획.md");
  });
  it("파일 경로가 없거나 비어 있으면 읽지 않고 400으로 거절한다", async () => {
    bridge.getMobileTaskFile = vi.fn();
    expect(await request("/api/mobile/tasks/task-1/workspace-file", { headers })).toEqual({ status: 400, body: { success: false, error: "파일 경로가 필요합니다" } });
    expect((await request("/api/mobile/tasks/task-1/workspace-file?path=", { headers })).status).toBe(400);
    expect(bridge.getMobileTaskFile).not.toHaveBeenCalled();
  });
  it.each([
    ["outside-worktree", 400, "작업 폴더 밖의 파일은 열 수 없습니다"],
    ["not-found", 404, "파일을 찾을 수 없습니다"],
    ["too-large", 413, "파일이 너무 커서 열 수 없습니다"],
    ["unreadable", 500, "파일을 읽을 수 없습니다"],
  ])("파일을 열지 못한 사유 %s는 %i과 사유로 구분해 알린다", async (reason, status, error) => {
    bridge.getMobileTaskFile = vi.fn(async () => { throw Object.assign(new Error(reason), { name: "TaskWorkspaceFileError", reason }); });
    expect(await request("/api/mobile/tasks/task-1/workspace-file?path=a.md", { headers })).toEqual({ status, body: { success: false, error, reason } });
  });
  it("파일 읽기의 알 수 없는 실패는 내부 메시지를 내보내지 않는다", async () => {
    bridge.getMobileTaskFile = vi.fn(async () => { throw new Error("private host path"); });
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(await request("/api/mobile/tasks/task-1/workspace-file?path=a.md", { headers })).toEqual({ status: 500, body: { success: false, error: "서버 오류" } });
    consoleError.mockRestore();
  });
  it("부분 전송 뒤 불확실한 결과는 재전송 유도 없이 명시한다", async () => {
    bridge.sendPaneChatPrompt.mockRejectedValue(Object.assign(new Error("전송 결과 불확실"), { name: "PaneChatDeliveryUncertainError" }));
    expect(await request("/api/mobile/tasks/task-1/pane-chat/prompt", { method: "POST", headers, body: JSON.stringify({ targetToken: "token", text: "caption", requestId: "once" }) })).toEqual({ status: 409, body: { success: false, error: "전송 결과 불확실", reason: "delivery-uncertain" } });
  });
});

describe("원격 pane 지속 연결 경로", () => {
  const connectionPath = "/api/mobile/tasks/task-1/panes/w1%3Ap1/connection";
  const grant = {
    version: 1, grantId: "grant-one", expiresAt: 2_000_000_000_000,
    direct: { host: "b.example", port: 19443, path: "/stream/grant-one", token: "B-only-capability", certificateFingerprint: "b".repeat(64) },
  };
  const useTls = async () => {
    for (const socket of openSockets) socket.destroy();
    await new Promise((resolve) => server.close(resolve));
    server = await startServer({ secure: true });
  };

  it("인증된 TLS 요청에만 task/pane/device 범위의 B 접속 정보를 보낸다", async () => {
    await useTls();
    bridge.prepareMobilePaneConnection.mockResolvedValue(grant);
    const response = await request(connectionPath, { method: "POST", headers: { authorization: "Bearer good" } });
    expect(response).toEqual({ status: 200, body: { success: true, connection: grant } });
    expect(bridge.prepareMobilePaneConnection).toHaveBeenCalledWith("task-1", "w1:p1", "device-good");
  });

  it("평문 A 접속에는 B 접속 토큰을 발급하지 않는다", async () => {
    bridge.prepareMobilePaneConnection.mockResolvedValue(grant);
    const response = await request(connectionPath, { method: "POST", headers: { authorization: "Bearer good" } });
    expect(response).toEqual({ status: 200, body: { success: true, connection: null } });
    expect(bridge.prepareMobilePaneConnection).not.toHaveBeenCalled();
  });

  it("연결 준비 중 해제된 기기에는 B capability를 반환하지 않는다", async () => {
    await useTls();
    bridge.prepareMobilePaneConnection.mockImplementation(async () => {
      bridge.authorizeMobileRequest.mockResolvedValue(null);
      return grant;
    });
    const response = await request(connectionPath, { method: "POST", headers: { authorization: "Bearer good" } });
    expect(response.status).toBe(401);
    expect(response.body).not.toHaveProperty("connection");
  });

  it("유효하지 않은 pane 접속 준비는 세션 오류로 응답한다", async () => {
    await useTls();
    bridge.prepareMobilePaneConnection.mockRejectedValue(Object.assign(new Error("missing pane"), { name: "SurfaceUnavailableError", reason: "session-not-running" }));
    const response = await request(connectionPath, { method: "POST", headers: { authorization: "Bearer good" } });
    expect(response).toEqual({ status: 409, body: { success: false, reason: "session-not-running" } });
  });

  it("A relay 핸드셰이크 중 연결이 해제된 기기를 다시 인증하고 닫는다", async () => {
    bridge.authorizeMobileRequest.mockResolvedValueOnce("device-good").mockResolvedValue(null);
    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/stream?taskId=task-1&paneId=w1%3Ap1`, { headers: { authorization: "Bearer good" } });
    const closed = waitForClose(client);
    await new Promise((resolve, reject) => { client.once("open", resolve); client.once("error", reject); });
    await waitFor(() => bridge.authorizeMobileRequest.mock.calls.length === 2);
    await expect(closed).resolves.toEqual({ code: 4001, reason: "device-unpaired" });
  });

  it("다시 인증하는 동안 받은 키는 승인 뒤에만 B relay로 보낸다", async () => {
    let authorize;
    bridge.authorizeMobileRequest.mockResolvedValueOnce("device-good").mockImplementation(() => new Promise((resolve) => { authorize = resolve; }));
    const relay = { write: vi.fn(), close: vi.fn() };
    bridge.openRemoteMobilePaneStream.mockResolvedValue(relay);
    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/stream?taskId=task-1&paneId=w1%3Ap1`, { headers: { authorization: "Bearer good" } });
    await new Promise((resolve, reject) => { client.once("open", resolve); client.once("error", reject); });
    client.send("pending-input");
    const pong = new Promise((resolve) => client.once("pong", resolve)); client.ping(); await pong;
    expect(bridge.openRemoteMobilePaneStream).not.toHaveBeenCalled();
    expect(bridge.subscribeToPane).not.toHaveBeenCalled();
    expect(relay.write).not.toHaveBeenCalled();
    authorize("device-good");
    await waitFor(() => relay.write.mock.calls.length === 1);
    expect(relay.write.mock.calls[0].map((value) => typeof value === "boolean" ? value : value.toString())).toEqual(["pending-input", false]);
    client.close();
  });

  it("다시 인증하는 동안 해제된 기기의 상태 명령은 실행하지 않는다", async () => {
    let authorize;
    bridge.authorizeMobileRequest.mockResolvedValueOnce("device-good").mockImplementation(() => new Promise((resolve) => { authorize = resolve; }));
    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/events`, { headers: { authorization: "Bearer good" } });
    const messages = [];
    client.on("message", (data) => messages.push(data.toString()));
    const closed = waitForClose(client);
    await new Promise((resolve, reject) => { client.once("open", resolve); client.once("error", reject); });
    client.send(JSON.stringify({ type: "command", requestId: "blocked", command: "start-session", taskId: "task-1" }));
    const pong = new Promise((resolve) => client.once("pong", resolve)); client.ping(); await pong;
    expect(bridge.getTaskSurfaces).not.toHaveBeenCalled();
    expect(messages).toEqual([]);
    authorize(null);
    await expect(closed).resolves.toEqual({ code: 4001, reason: "device-unpaired" });
    expect(bridge.getTaskSurfaces).not.toHaveBeenCalled();
  });

  it("B relay는 입력과 크기 프레임을 기존 SSH 명령 경로 없이 전달한다", async () => {
    const relay = { write: vi.fn(), close: vi.fn() };
    let emit;
    bridge.openRemoteMobilePaneStream.mockImplementation(async (_task, _pane, _device, _grant, onData) => { emit = onData; return relay; });
    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/stream?taskId=task-1&paneId=w1%3Ap1&grantId=grant-one`, { headers: { authorization: "Bearer good" } });
    await new Promise((resolve, reject) => { client.once("open", resolve); client.once("error", reject); });
    await waitFor(() => bridge.openRemoteMobilePaneStream.mock.calls.length === 1);
    const output = new Promise((resolve) => client.once("message", (data) => resolve(data.toString())));
    emit("B terminal output");
    await expect(output).resolves.toBe("B terminal output");
    client.send(Buffer.from(JSON.stringify({ type: "refresh-history" })));
    client.send("ls\n");
    const resize = Buffer.from(JSON.stringify({ type: "resize", cols: 100, rows: 30 }));
    client.send(resize);
    await waitFor(() => relay.write.mock.calls.length === 2);
    expect(relay.write.mock.calls.map(([data, binary]) => [data.toString(), binary])).toEqual([["ls\n", false], [resize.toString(), true]]);
    expect(bridge.refreshPaneHistory).not.toHaveBeenCalled();
    expect(bridge.writeToPane).not.toHaveBeenCalled();
    expect(bridge.claimPaneViewport).not.toHaveBeenCalled();
    expect(bridge.subscribeToPane).not.toHaveBeenCalled();
    const closed = waitForClose(client); client.close(); await closed;
    await waitFor(() => relay.close.mock.calls.length === 1);
  });

  it("relay 준비 중 닫힌 모바일의 늦은 결과를 즉시 닫는다", async () => {
    const relay = { write: vi.fn(), close: vi.fn() };
    let finish;
    bridge.openRemoteMobilePaneStream.mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/stream?taskId=task-1&paneId=w1%3Ap1`, { headers: { authorization: "Bearer good" } });
    await new Promise((resolve, reject) => { client.once("open", resolve); client.once("error", reject); });
    await waitFor(() => typeof finish === "function");
    const closed = waitForClose(client); client.close(); await closed;
    finish(relay);
    await waitFor(() => relay.close.mock.calls.length === 1);
    expect(relay.write).not.toHaveBeenCalled();
  });
});
