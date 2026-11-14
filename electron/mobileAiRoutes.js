const { randomUUID } = require("node:crypto");
const { readJsonBody, writeJson } = require("./httpBody.js");

const LIVE_PATH = /^\/api\/mobile\/tasks\/([^/]+)\/live-ai-sessions$/;
const GRAPH_PATH = /^\/api\/mobile\/tasks\/([^/]+)\/agent-call-graph$/;
const ACCOUNT_PATH = /^\/api\/mobile\/ai\/accounts\/([^/]+)$/;
const LOGIN_PATH = /^\/api\/mobile\/ai\/accounts\/([a-f0-9]{64})\/login$/;
const ACCOUNT_LIST_PATH = "/api/mobile/ai/accounts";
const USAGE_PATH = "/api/mobile/ai/usage";
const MAX_FRAME_BYTES = 64 * 1024;
// 출력을 읽지 않는 기기 때문에 OAuth 프로세스와 메인 프로세스 버퍼가 함께 자라지 않게 한다.
const MAX_BUFFERED_OUTPUT_BYTES = 1024 * 1024;
const DEFAULT_COLUMNS = 80;
const DEFAULT_ROWS = 24;
const MIN_COLUMNS = 2;
const MAX_COLUMNS = 500;
const MIN_ROWS = 1;
const MAX_ROWS = 250;
const ERROR_STATUSES = new Set([400, 403, 404, 409, 429]);

function responseError(error) {
  if (error instanceof SyntaxError || error instanceof URIError) return { status: 400, error: "요청 형식이 올바르지 않습니다" };
  if (ERROR_STATUSES.has(error?.statusCode)) return { status: error.statusCode, error: error.message };
  return { status: 500, error: "AI 기능 요청을 처리하지 못했습니다" };
}

// 인증은 기존 mobileRoutes의 공통 경계에서 먼저 수행한다. 이 파일은 허용된 AI 경로만 맡는다.
async function handleMobileAiRequest(request, response, { bridge, requestUrl }) {
  try {
    if (request.method === "GET" && requestUrl.pathname === "/api/mobile/ai/running-panes") {
      writeJson(response, 200, { success: true, panes: await bridge.getMobileRunningAgentPanes() });
      return true;
    }
    const live = requestUrl.pathname.match(LIVE_PATH);
    if (request.method === "GET" && live) {
      writeJson(response, 200, { success: true, liveSessions: await bridge.getMobileLiveAiSessions(decodeURIComponent(live[1])) });
      return true;
    }
    const graph = requestUrl.pathname.match(GRAPH_PATH);
    if (request.method === "GET" && graph) {
      const provider = requestUrl.searchParams.get("provider");
      const sessionId = requestUrl.searchParams.get("sessionId");
      if (!provider || !sessionId) {
        writeJson(response, 400, { success: false, error: "AI 제공자와 세션을 선택해 주세요" });
        return true;
      }
      writeJson(response, 200, { success: true, graph: await bridge.getMobileAgentCallGraph(decodeURIComponent(graph[1]), provider, sessionId) });
      return true;
    }
    if (requestUrl.pathname === ACCOUNT_LIST_PATH && request.method === "GET") {
      writeJson(response, 200, { success: true, accounts: await bridge.listMobileAiAccounts() });
      return true;
    }
    if (requestUrl.pathname === ACCOUNT_LIST_PATH && request.method === "POST") {
      writeJson(response, 201, { success: true, ...await bridge.addMobileAiAccount(await readJsonBody(request)) });
      return true;
    }
    const account = requestUrl.pathname.match(ACCOUNT_PATH);
    if (account && request.method === "DELETE") {
      writeJson(response, 200, { success: true, ...await bridge.removeMobileAiAccount(decodeURIComponent(account[1])) });
      return true;
    }
    if (requestUrl.pathname === USAGE_PATH && request.method === "GET") {
      writeJson(response, 200, { success: true, usage: await bridge.getMobileAiUsageSnapshot() });
      return true;
    }
    return false;
  } catch (error) {
    const failure = responseError(error);
    writeJson(response, failure.status, { success: false, error: failure.error });
    return true;
  }
}

function validTerminalSize(cols, rows) {
  return Number.isInteger(cols) && cols >= MIN_COLUMNS && cols <= MAX_COLUMNS && Number.isInteger(rows) && rows >= MIN_ROWS && rows <= MAX_ROWS;
}

async function attachMobileAiLoginStream(webSocket, { bridge, deviceId, requestUrl }) {
  const socketId = randomUUID();
  let isClosed = false;
  const isOpen = () => !isClosed && webSocket.readyState === webSocket.OPEN;
  const cleanup = () => {
    if (isClosed) return;
    isClosed = true;
    bridge.closeMobileAiLogin(deviceId, socketId);
  };
  const finish = (code, reason) => {
    cleanup();
    if (webSocket.readyState === webSocket.OPEN) webSocket.close(code, reason);
  };
  const send = (event) => {
    if (!isOpen()) return;
    if (webSocket.bufferedAmount > MAX_BUFFERED_OUTPUT_BYTES) {
      finish(1009, "login-output-overflow");
      return;
    }
    webSocket.send(JSON.stringify(event));
  };
  webSocket.once("close", cleanup);
  webSocket.once("error", () => finish(1011, "login-stream-error"));
  webSocket.on("message", (data, isBinary) => {
    if (!isOpen()) return;
    try {
      if (isBinary || Buffer.byteLength(data) > MAX_FRAME_BYTES) throw new Error("invalid-frame");
      const command = JSON.parse(data.toString());
      if (command?.type === "input" && typeof command.data === "string") {
        bridge.writeMobileAiLogin(deviceId, socketId, command.data);
      } else if (command?.type === "resize" && validTerminalSize(command.cols, command.rows)) {
        bridge.resizeMobileAiLogin(deviceId, socketId, command.cols, command.rows);
      } else {
        throw new Error("invalid-command");
      }
    } catch {
      send({ type: "error", error: "터미널 입력 형식이 올바르지 않습니다" });
      finish(1008, "invalid-login-input");
    }
  });
  try {
    const match = requestUrl.pathname.match(LOGIN_PATH);
    const cols = requestUrl.searchParams.has("cols") ? Number(requestUrl.searchParams.get("cols")) : DEFAULT_COLUMNS;
    const rows = requestUrl.searchParams.has("rows") ? Number(requestUrl.searchParams.get("rows")) : DEFAULT_ROWS;
    if (!match || !validTerminalSize(cols, rows)) {
      send({ type: "error", error: "계정 또는 터미널 크기를 확인해 주세요" });
      finish(1008, "invalid-login-request");
      return;
    }
    await bridge.openMobileAiLogin(deviceId, socketId, match[1], cols, rows, {
      isOpen,
      onEvent: (event) => {
        send(event);
        if (event.type === "exit") finish(1000, "login-finished");
        if (event.type === "error") finish(1011, "login-failed");
      },
    });
    if (!isOpen()) cleanup();
    else send({ type: "ready" });
  } catch (error) {
    send({ type: "error", error: responseError(error).error });
    finish(1011, "login-open-failed");
  }
}

module.exports = { handleMobileAiRequest, attachMobileAiLoginStream };
