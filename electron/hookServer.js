const http = require("node:http");
const https = require("node:https");
const path = require("node:path");

const { rejectBrowserOriginRequest } = require("./browserOriginGuard.js");
const { readJsonBody, writeJson } = require("./httpBody.js");
const { loadOrCreateMobileTlsCertificate } = require("./mobileTlsCertificate.js");

const MOBILE_TLS_BIND_HOST = "0.0.0.0";

function getRuntimeModulePath(...moduleSegments) {
  const appRoot = path.join(__dirname, "..");

  if (process.env.KANVIBE_RENDERER_URL) {
    return path.join(appRoot, "src", ...moduleSegments) + ".ts";
  }

  return path.join(appRoot, "build", "main", "src", ...moduleSegments) + ".js";
}

function writeShellScript(response, script) {
  response.writeHead(200, { "Content-Type": "text/plain; charset=utf-8" });
  response.end(script);
}

/** 서버 하나가 죽어도 나머지 서버와 앱은 계속 돌아야 하므로 실패는 로그로만 남긴다 */
function logListenerLifecycle(server, label, host, port, onListening) {
  server.on("error", (error) => {
    console.error(`[kanvibe] ${label} failed:`, error);
  });
  server.listen(port, host, () => {
    console.log(`[kanvibe] ${label} listening on ${host}:${port}`);
    onListening?.();
  });
}

function createHookServer({ host, port, mobileTlsPort, mobileTlsDirectory, isDevPairingRouteEnabled }) {
  const hookService = require(getRuntimeModulePath("desktop", "main", "services", "hookService"));
  const hookInstallBundle = require(getRuntimeModulePath("lib", "hookInstallBundle"));
  const hookTunnels = require(getRuntimeModulePath("desktop", "main", "services", "hookTunnelService"));
  const lanPlaintext = require(getRuntimeModulePath("desktop", "main", "services", "lanPlaintextService"));
  const mobileBridge = require(getRuntimeModulePath("desktop", "main", "services", "mobileBridgeService"));
  const mobileNotifications = require(getRuntimeModulePath("desktop", "main", "services", "mobileNotificationService"));
  const taskCli = require(getRuntimeModulePath("desktop", "main", "services", "taskCliService"));
  const { handleMobileRequest, attachMobileStreamServer } = require("./mobileRoutes.js");
  const { handleTaskCliRequest } = require("./taskCliRoutes.js");

  /** @param sourceSshHosts 터널로 들어온 요청이면 그 터널이 맡은 호스트 별칭들. 평문 포트의 요청에는 없다 */
  async function handleHookRequest(request, response, sourceSshHosts) {
    /** task CLI 경로는 task 단위 토큰으로 자체 인증한다 */
    if (await handleTaskCliRequest(request, response, { taskCli, sourceSshHosts })) {
      return;
    }

    if (request.method === "GET" && request.url === "/api/hooks/health") {
      writeJson(response, 200, { success: true });
      return;
    }

    /** 원격 호스트가 hook 설치 산출물을 직접 내려받는 경로. 토큰이 맞아야만 스크립트를 내준다 */
    if (request.method === "GET" && request.url === hookInstallBundle.HOOK_INSTALL_SCRIPT_PATH) {
      const installScript = hookInstallBundle.readHookInstallScript(request.headers[hookInstallBundle.HOOK_INSTALL_TOKEN_HEADER]);

      if (!installScript) {
        writeJson(response, 404, { success: false, error: "Not found" });
        return;
      }

      writeShellScript(response, installScript);
      return;
    }

    if (request.method === "POST" && request.url === "/api/hooks/status") {
      try {
        const result = await hookService.updateHookTaskStatus(await readJsonBody(request), sourceSshHosts);
        writeJson(response, result.status || 200, result);
      } catch (error) {
        writeJson(response, 500, { success: false, error: error instanceof Error ? error.message : "서버 오류" });
      }
      return;
    }

    writeJson(response, 404, { success: false, error: "Not found" });
  }

  /** 응답 코드를 보고 세야 하므로 응답이 끝난 뒤에 기록한다 */
  function recordLanPlaintextUsageOnSuccess(request, response) {
    response.once("finish", () => {
      lanPlaintext.recordLanPlaintextResponse(request.socket.remoteAddress, request.url, response.statusCode);
    });
  }

  const mobileRouteOptions = { bridge: mobileBridge, host, port, isDevPairingRouteEnabled };

  const server = http.createServer(async (request, response) => {
    if (rejectBrowserOriginRequest(request, response)) {
      return;
    }

    recordLanPlaintextUsageOnSuccess(request, response);

    /** 모바일 경로는 자체 인증을 거치므로 hook 라우팅보다 먼저 가로챈다 */
    if (await handleMobileRequest(request, response, mobileRouteOptions)) {
      return;
    }

    await handleHookRequest(request, response);
  });

  /** 터널 소켓은 모바일 경로를 싣지 않는다. 원격 호스트의 프로세스가 기기 인증 경로에 닿을 이유가 없다 */
  const createTunnelRequestListener = (sshHosts) => async (request, response) => {
    if (rejectBrowserOriginRequest(request, response)) {
      return;
    }
    await handleHookRequest(request, response, sshHosts);
  };

  /** TLS 포트는 모바일 경로만 싣는다. hook 경로는 평문 포트와 터널이 맡는다 */
  function createMobileTlsServer(certificate) {
    return https.createServer(certificate, async (request, response) => {
      if (rejectBrowserOriginRequest(request, response)) {
        return;
      }
      if (!(await handleMobileRequest(request, response, mobileRouteOptions))) {
        writeJson(response, 404, { success: false, error: "Not found" });
      }
    });
  }

  mobileNotifications.startMobileNotifications({
    relayUrl: process.env.KANVIBE_MOBILE_PUSH_RELAY_URL,
    relayToken: process.env.KANVIBE_MOBILE_PUSH_RELAY_TOKEN,
    readSubscriptions: mobileBridge.listMobilePushSubscriptions,
    onUnregistered: mobileBridge.removeUnregisteredMobilePushSubscription,
  });
  attachMobileStreamServer(server, { bridge: mobileBridge, notifications: mobileNotifications, host, port });

  let mobileTlsServer = null;
  let isClosed = false;
  const closePlaintextServer = server.close.bind(server);
  server.close = (...args) => {
    isClosed = true;
    hookTunnels.stopHookTunnels();
    mobileTlsServer?.close();
    return closePlaintextServer(...args);
  };

  /** 포트를 얻지 못한 인스턴스가 터널을 열면 먼저 뜬 인스턴스의 원격 소켓을 지우게 되므로, 포트를 잡은 뒤에만 연다 */
  logListenerLifecycle(server, "Hook server", host, port, () => hookTunnels.startHookTunnels(createTunnelRequestListener));

  /** 인증서를 만들지 못해도 hook 경로는 계속 동작해야 하므로 평문 서버의 시작을 막지 않는다 */
  loadOrCreateMobileTlsCertificate(mobileTlsDirectory)
    .then((certificate) => {
      if (isClosed) return;
      mobileTlsServer = createMobileTlsServer(certificate);
      attachMobileStreamServer(mobileTlsServer, { bridge: mobileBridge, notifications: mobileNotifications, host, port: mobileTlsPort });
      logListenerLifecycle(mobileTlsServer, "Mobile TLS server", MOBILE_TLS_BIND_HOST, mobileTlsPort);
    })
    .catch((error) => {
      console.error("[kanvibe] Mobile TLS certificate failed:", error);
    });

  return server;
}

module.exports = { createHookServer };
