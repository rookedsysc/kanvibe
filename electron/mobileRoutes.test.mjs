/**
 * @vitest-environment node
 */
import { EventEmitter } from "node:events";
import http from "node:http";
import net from "node:net";
import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WebSocket from "ws";

const require = createRequire(import.meta.url);
const { handleMobileRequest, attachMobileStreamServer } = require("./mobileRoutes");
const { MAX_BODY_BYTES, readJsonBody } = require("./httpBody");

const HOST = "127.0.0.1";

let bridge;
let server;
let streamServer;
let port;
let openSockets;
let unpairedListeners;

/** 헤더마다 다른 기기가 오게 두어야 스트림 하나를 끊는 것이 다른 기기까지 끊는지 가려낼 수 있다 */
const DEVICE_IDS = { "Bearer good": "device-good", "Bearer second": "device-second" };

function createBridge(overrides = {}) {
  unpairedListeners = new Set();

  return {
    authorizeMobileRequest: vi.fn(async (header) => DEVICE_IDS[header] ?? null),
    onMobileDeviceUnpaired: vi.fn((listener) => {
      unpairedListeners.add(listener);
      return () => unpairedListeners.delete(listener);
    }),
    pairMobileDevice: vi.fn(async () => "paired-token"),
    unpairMobileDeviceByToken: vi.fn(async () => true),
    getMobileBoard: vi.fn(async () => ({ columns: [] })),
    getTaskSurfaces: vi.fn(async () => ({ panes: [] })),
    subscribeToPane: vi.fn(async () => () => {}),
    writeToPane: vi.fn(async () => {}),
    ...overrides,
  };
}

/** hookServer와 같은 방식으로 얹어야 "우리 경로가 아니면 false" 계약까지 같이 검증된다 */
function startServer() {
  return new Promise((resolve) => {
    openSockets = [];
    const created = http.createServer(async (request, response) => {
      if (await handleMobileRequest(request, response, { bridge, host: HOST, port })) {
        return;
      }
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ handledBy: "hook-fallthrough" }));
    });
    created.on("connection", (socket) => openSockets.push(socket));
    streamServer = attachMobileStreamServer(created, { bridge, host: HOST, port: 0 });
    created.listen(0, HOST, () => {
      port = created.address().port;
      resolve(created);
    });
  });
}

function request(path, { method = "GET", headers = {}, body } = {}) {
  return new Promise((resolve, reject) => {
    const clientRequest = http.request({ host: HOST, port, path, method, headers }, (response) => {
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
