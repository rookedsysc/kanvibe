const { WebSocketServer } = require("ws");

/**
 * 모바일 클라이언트가 쓰는 `/api/mobile/*` 경로.
 *
 * hook 서버와 같은 포트에 얹는다. 포트를 하나 더 열면 사용자가 방화벽과 원격 접속 설정을 두 번 해야 한다.
 * 다만 신뢰 경계는 다르다. `/api/hooks/*`는 인증이 없고 그대로 두지만, 여기는 페어링한 기기만 지나갈 수 있다.
 * 터미널을 읽고 쓰는 경로가 이 아래에 있기 때문이다.
 */

const MOBILE_PATH_PREFIX = "/api/mobile/";
const STREAM_PATH = "/api/mobile/stream";

/** 태스크 pane 경로에서 태스크 id를 꺼낸다 */
const SURFACES_PATH_PATTERN = /^\/api\/mobile\/tasks\/([^/]+)\/surfaces$/;

function readJsonBody(request) {
  return new Promise((resolve, reject) => {
    let body = "";
    request.on("data", (chunk) => {
      body += chunk;
    });
    request.on("end", () => {
      try {
        resolve(body ? JSON.parse(body) : {});
      } catch (error) {
        reject(error);
      }
    });
    request.on("error", reject);
  });
}

function writeJson(response, statusCode, payload) {
  response.writeHead(statusCode, { "Content-Type": "application/json" });
  response.end(JSON.stringify(payload));
}

/**
 * `/api/mobile/*` 요청을 처리한다. 이 경로가 아니면 false를 돌려 hook 서버가 자기 라우팅을 이어가게 한다.
 */
async function handleMobileRequest(request, response, { bridge, host, port }) {
  if (!request.url || !request.url.startsWith(MOBILE_PATH_PREFIX)) {
    return false;
  }

  const requestUrl = new URL(request.url, `http://${host}:${port}`);

  if (request.method === "POST" && requestUrl.pathname === "/api/mobile/pair") {
    await handlePairRequest(request, response, bridge);
    return true;
  }

  if (!(await bridge.authorizeMobileRequest(request.headers.authorization))) {
    writeJson(response, 401, { success: false, error: "연결되지 않은 기기입니다" });
    return true;
  }

  if (request.method === "GET" && requestUrl.pathname === "/api/mobile/board") {
    writeJson(response, 200, { success: true, board: await bridge.getMobileBoard() });
    return true;
  }

  const surfacesMatch = requestUrl.pathname.match(SURFACES_PATH_PATTERN);
  if (request.method === "GET" && surfacesMatch) {
    await handleSurfacesRequest(response, bridge, decodeURIComponent(surfacesMatch[1]));
    return true;
  }

  writeJson(response, 404, { success: false, error: "Not found" });
  return true;
}

async function handlePairRequest(request, response, bridge) {
  let body;
  try {
    body = await readJsonBody(request);
  } catch {
    writeJson(response, 400, { success: false, error: "잘못된 요청입니다" });
    return;
  }

  const submittedCode = typeof body.code === "string" ? body.code : "";
  const deviceName = typeof body.deviceName === "string" && body.deviceName ? body.deviceName : "모바일 기기";
  const token = await bridge.pairMobileDevice(submittedCode, deviceName);

  if (!token) {
    writeJson(response, 401, { success: false, error: "코드가 맞지 않거나 만료되었습니다" });
    return;
  }

  writeJson(response, 200, { success: true, token });
}

async function handleSurfacesRequest(response, bridge, taskId) {
  try {
    writeJson(response, 200, { success: true, surfaces: await bridge.getTaskSurfaces(taskId) });
  } catch (error) {
    /** 세션이 없거나 꺼져 있는 것은 서버 오류가 아니라 화면이 안내해야 하는 상태다 */
    if (error && error.name === "SurfaceUnavailableError") {
      writeJson(response, 409, { success: false, reason: error.reason });
      return;
    }
    writeJson(response, 500, { success: false, error: error instanceof Error ? error.message : "서버 오류" });
  }
}

/**
 * pane 스트림용 WebSocket을 hook 서버에 붙인다.
 *
 * `noServer`로 만들고 upgrade를 직접 받는 이유는, 인증을 통과하지 못한 요청에는 WebSocket 핸드셰이크를 아예 열어 주지 않기 위해서다.
 * 핸드셰이크를 먼저 맺고 나서 닫으면 연결이 잠깐이라도 성립한다.
 */
function attachMobileStreamServer(server, { bridge, host, port }) {
  const webSocketServer = new WebSocketServer({ noServer: true });

  server.on("upgrade", async (request, socket, head) => {
    const requestUrl = new URL(request.url || "", `http://${host}:${port}`);
    if (requestUrl.pathname !== STREAM_PATH) {
      return;
    }

    if (!(await bridge.authorizeMobileRequest(request.headers.authorization))) {
      socket.write("HTTP/1.1 401 Unauthorized\r\n\r\n");
      socket.destroy();
      return;
    }

    const taskId = requestUrl.searchParams.get("taskId");
    const paneId = requestUrl.searchParams.get("paneId");
    if (!taskId || !paneId) {
      socket.write("HTTP/1.1 400 Bad Request\r\n\r\n");
      socket.destroy();
      return;
    }

    webSocketServer.handleUpgrade(request, socket, head, (webSocket) => {
      openPaneStream(webSocket, bridge, taskId, paneId);
    });
  });

  return webSocketServer;
}

async function openPaneStream(webSocket, bridge, taskId, paneId) {
  let unsubscribe = null;
  let isClosed = false;

  /** 구독이 붙기 전에 기기가 나가면 파이프를 걸어 둔 채로 남으므로 곧바로 되돌린다 */
  webSocket.on("close", () => {
    isClosed = true;
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
  });

  webSocket.on("message", (data) => {
    void bridge.writeToPane(taskId, paneId, data.toString()).catch(() => {});
  });

  try {
    const stop = await bridge.subscribeToPane(taskId, paneId, (chunk) => {
      if (webSocket.readyState === webSocket.OPEN) {
        webSocket.send(chunk);
      }
    });

    if (isClosed) {
      stop();
      return;
    }
    unsubscribe = stop;
  } catch (error) {
    const reason = error && error.name === "SurfaceUnavailableError" ? error.reason : "stream-failed";
    webSocket.close(4000, reason);
  }
}

module.exports = { handleMobileRequest, attachMobileStreamServer };
