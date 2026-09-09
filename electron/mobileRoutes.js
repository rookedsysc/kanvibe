const { WebSocketServer } = require("ws");

const { readJsonBody, writeJson } = require("./httpBody.js");

/**
 * 모바일 클라이언트가 쓰는 `/api/mobile/*` 경로.
 *
 * hook 서버와 같은 포트에 얹는다. 포트를 하나 더 열면 사용자가 방화벽과 원격 접속 설정을 두 번 해야 한다.
 * 다만 신뢰 경계는 다르다. `/api/hooks/*`는 인증이 없고 그대로 두지만, 여기는 페어링한 기기만 지나갈 수 있다.
 * 터미널을 읽고 쓰는 경로가 이 아래에 있기 때문이다.
 */

const MOBILE_PATH_PREFIX = "/api/mobile/";
const STREAM_PATH = "/api/mobile/stream";

/**
 * 연결이 해제되어 끊는 스트림의 종료 코드.
 * 스트림 실패(4000)와 나누어야 기기가 "세션을 못 비춘다"와 "이 기기는 더 이상 연결되어 있지 않다"를 구분할 수 있다.
 */
const DEVICE_UNPAIRED_CLOSE_CODE = 4001;

/**
 * 목록에서 기기를 알아보기 위한 이름이라 사람이 읽을 만한 길이면 충분하다.
 * 본문 상한(`httpBody.js`)까지의 문자열이 그대로 저장되면 설정 화면의 항목 하나가 레이아웃을 밀어
 * 그 기기를 끊는 버튼까지 화면 밖으로 보내, 사용자가 지울 수 없는 항목이 된다.
 */
const MAX_DEVICE_NAME_LENGTH = 64;

/** 태스크 pane 경로에서 태스크 id를 꺼낸다 */
const SURFACES_PATH_PATTERN = /^\/api\/mobile\/tasks\/([^/]+)\/surfaces$/;

/**
 * `/api/mobile/*` 요청을 처리한다. 이 경로가 아니면 false를 돌려 hook 서버가 자기 라우팅을 이어가게 한다.
 *
 * 우리 경로로 판정된 뒤부터는 전부 try로 감싼다. 호출자가 `http.createServer(async ...)` 안에서 await하기 때문에
 * 여기서 던지면 Node가 받아 주지 않아 응답이 영영 안 나가고(기기는 타임아웃까지 매달린다) 처리되지 않은 거부로 메인 프로세스가 죽는다.
 */
async function handleMobileRequest(request, response, { bridge, host, port }) {
  if (!request.url || !request.url.startsWith(MOBILE_PATH_PREFIX)) {
    return false;
  }

  try {
    const requestUrl = new URL(request.url, `http://${host}:${port}`);

    if (request.method === "POST" && requestUrl.pathname === "/api/mobile/pair") {
      await handlePairRequest(request, response, bridge);
      return true;
    }

    if (!(await bridge.authorizeMobileRequest(request.headers.authorization))) {
      writeJson(response, 401, { success: false, error: "연결되지 않은 기기입니다" });
      return true;
    }

    if (request.method === "DELETE" && requestUrl.pathname === "/api/mobile/device") {
      await handleUnpairRequest(request, response, bridge);
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
  } catch (error) {
    console.error("[kanvibe] Mobile request failed:", error);
    writeJson(response, 500, { success: false, error: "서버 오류" });
    return true;
  }
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
  const submittedName = typeof body.deviceName === "string" ? body.deviceName.slice(0, MAX_DEVICE_NAME_LENGTH) : "";
  const deviceName = submittedName || "모바일 기기";
  const token = await bridge.pairMobileDevice(submittedCode, deviceName);

  if (!token) {
    writeJson(response, 401, { success: false, error: "코드가 맞지 않거나 만료되었습니다" });
    return;
  }

  writeJson(response, 200, { success: true, token });
}

/**
 * 기기가 스스로 연결을 끊는다. `/pair`와 달리 인증 뒤에 있으므로 지우는 대상은 요청에 실린 토큰의 주인 하나로 정해져 있다.
 *
 * 인증과 삭제 사이에 설정 화면이 같은 기기를 이미 끊었을 수 있어, 지운 것이 없으면 앞단과 같은 401로 답한다.
 * 이미 열려 있는 pane 스트림은 서비스가 알리는 해제 신호를 받아 [attachMobileStreamServer]가 닫는다.
 * 설정 화면의 연결 해제도 같은 신호를 지나므로, 어느 쪽에서 끊어도 남는 스트림이 없다.
 */
async function handleUnpairRequest(request, response, bridge) {
  if (await bridge.unpairMobileDeviceByToken(request.headers.authorization)) {
    writeJson(response, 200, { success: true });
    return;
  }

  writeJson(response, 401, { success: false, error: "연결되지 않은 기기입니다" });
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
    /** 예외 메시지에는 tmux/ssh 명령줄과 호스트 별칭이 섞여 있어 네트워크로 내보내지 않고 로그에만 남긴다 */
    console.error("[kanvibe] Mobile surfaces request failed:", error);
    writeJson(response, 500, { success: false, error: "서버 오류" });
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

  /**
   * 기기별로 열려 있는 스트림 소켓.
   *
   * 연결을 해제한 기기의 스트림은 토큰을 지우는 것만으로 멎지 않는다. 이미 맺어진 소켓은 다시 인증을 지나지 않으므로
   * 사용자가 끊었다고 믿는 기기가 그대로 터미널을 계속 비춘다. 그것을 닫으려면 어느 소켓이 누구 것인지 알아야 한다.
   */
  const socketsByDeviceId = new Map();

  function registerDeviceSocket(deviceId, webSocket) {
    const sockets = socketsByDeviceId.get(deviceId);
    if (sockets) {
      sockets.add(webSocket);
      return;
    }
    socketsByDeviceId.set(deviceId, new Set([webSocket]));
  }

  /** 빈 Set을 남기면 스트림을 여닫기만 반복해도 지도가 기기 수만큼 자란다 */
  function forgetDeviceSocket(deviceId, webSocket) {
    const sockets = socketsByDeviceId.get(deviceId);
    if (!sockets) {
      return;
    }

    sockets.delete(webSocket);
    if (sockets.size === 0) {
      socketsByDeviceId.delete(deviceId);
    }
  }

  /**
   * 해제된 기기의 소켓만 닫는다. pane 구독은 `taskId#paneId`로 묶여 여러 기기가 나눠 쓰므로 여기서 건드리지 않는다.
   * 남은 기기가 있으면 그 화면은 그대로 흘러야 하고, 마지막 구독자가 나가면 서비스 쪽 참조 수가 스트림을 알아서 멈춘다.
   */
  bridge.onMobileDeviceUnpaired((deviceId) => {
    const sockets = socketsByDeviceId.get(deviceId);
    if (!sockets) {
      return;
    }

    /** 닫는 도중에 close가 먼저 돌아 지도를 고쳐도 순회가 깨지지 않도록 복사본을 돈다 */
    for (const webSocket of [...sockets]) {
      webSocket.close(DEVICE_UNPAIRED_CLOSE_CODE, "device-unpaired");
    }
  });

  /**
   * upgrade 리스너를 달면 소켓 수명이 통째로 우리 몫이 되어 Node가 대신 닫아 주지 않는다.
   * 그래서 스트림 경로가 아니거나 도중에 예외가 나도 반드시 소켓을 끊는다. 인증 앞단이라 그대로 두면 LAN의 아무나 FD를 쌓을 수 있다.
   */
  server.on("upgrade", async (request, socket, head) => {
    try {
      const requestUrl = new URL(request.url || "", `http://${host}:${port}`);
      if (requestUrl.pathname !== STREAM_PATH) {
        socket.destroy();
        return;
      }

      const deviceId = await bridge.authorizeMobileRequest(request.headers.authorization);
      if (!deviceId) {
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
        /**
         * 인증이 끝나고 이 등록이 돌기까지의 짧은 사이에 해제가 오면 그 소켓 하나는 놓친다.
         * 핸드셰이크 뒤에 페어링을 다시 읽으면 막히지만 upgrade마다 조회가 한 번 더 늘고,
         * 놓친 기기도 다음 요청에서 401을 받아 앱이 페어링 화면으로 돌아간다.
         */
        registerDeviceSocket(deviceId, webSocket);
        openPaneStream(webSocket, bridge, taskId, paneId, () => forgetDeviceSocket(deviceId, webSocket));
      });
    } catch (error) {
      console.error("[kanvibe] Mobile stream upgrade failed:", error);
      socket.destroy();
    }
  });

  return webSocketServer;
}

/** [onClosed]는 닫힌 소켓을 기기 지도에서 지운다. 죽은 소켓이 남아 있으면 나중의 해제가 남의 자리를 뒤진다 */
async function openPaneStream(webSocket, bridge, taskId, paneId, onClosed) {
  let unsubscribe = null;
  let isClosed = false;

  /** 구독이 붙기 전에 기기가 나가면 파이프를 걸어 둔 채로 남으므로 곧바로 되돌린다 */
  webSocket.on("close", () => {
    isClosed = true;
    onClosed();
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
  });

  /**
   * 키 입력을 한 줄로 세운다.
   *
   * `writeToPane` 한 번은 세션 조회(DB 왕복)와 `sh -c` 프로세스 하나를 지나므로 짧지 않다. 그대로 띄우면
   * 겹친 두 입력의 `tmux send-keys` 순서를 OS 스케줄러가 정해, 빠르게 친 `ls`가 `sl`로 들어간다.
   * 재현이 간헐적이라 사용자가 원인을 짚기 가장 어려운 종류의 결함이다. 줄은 소켓 하나 몫이면 충분하다 —
   * 서로 다른 기기의 입력 사이에는 지켜야 할 순서가 없다.
   */
  let writeQueue = Promise.resolve();
  webSocket.on("message", (data) => {
    writeQueue = writeQueue.then(() => bridge.writeToPane(taskId, paneId, data.toString())).catch(() => {});
  });

  try {
    const stop = await bridge.subscribeToPane(
      taskId,
      paneId,
      (chunk) => {
        if (webSocket.readyState === webSocket.OPEN) {
          webSocket.send(chunk);
        }
      },
      /** 서버 쪽 스트림이 스스로 죽었다. 구독 시작 실패와 같은 코드로 닫아 화면이 같은 안내를 띄우게 한다 */
      () => webSocket.close(4000, "stream-failed"),
    );

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
