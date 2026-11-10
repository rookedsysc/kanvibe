/**
 * `/api/mobile/events` 소켓으로 들어온 명령 한 건을 검증해 브리지에 넘기고, 응답 메시지 한 건을 만든다.
 *
 * 소켓의 인증과 수명은 `mobileRoutes.js`가 맡는다. 인증을 통과한 기기만 이 소켓을 열 수 있으므로 여기서는 다시 묻지 않고,
 * 메시지 하나가 응답 하나가 되는 일만 한다. 응답을 보낼 곳이 없는 메시지는 `null`을 돌려 무시한다.
 */

const MAX_COMMAND_BYTES = 1024;
const MAX_REQUEST_ID_LENGTH = 64;

/** 응답에 실려 앱 화면의 안내가 되는 실패. 앱은 `code`로 분기하고 `error`는 그대로 보여 줄 수 있는 한국어 문장이다 */
class CommandFailure extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

function invalidCommand() {
  return new CommandFailure("invalid-command", "명령 형식이 올바르지 않습니다");
}

function readNonEmptyString(value) {
  if (typeof value !== "string" || value.length === 0) {
    throw invalidCommand();
  }
  return value;
}

/** 각 핸들러는 성공 응답에 펼칠 값을 돌려주고, 실패는 던진다 */
const COMMAND_HANDLERS = {
  "delete-task": async (bridge, message) => {
    const taskId = readNonEmptyString(message.taskId);
    if (message.isForced !== undefined && typeof message.isForced !== "boolean") {
      throw invalidCommand();
    }

    const deletion = await bridge.deleteMobileTask(taskId, message.isForced === true);
    if (!deletion) {
      throw new CommandFailure("task-not-found", "없는 태스크입니다");
    }
    return { failedPeerCount: deletion.failedPeerCount };
  },
  "get-tab-close-info": async (bridge, message) => {
    const { processNames } = await bridge.getMobilePaneCloseInfo(readNonEmptyString(message.taskId), readNonEmptyString(message.paneId));
    return { processNames };
  },
  "close-tab": async (bridge, message) => {
    const { surfaces } = await bridge.closeMobilePane(readNonEmptyString(message.taskId), readNonEmptyString(message.paneId));
    return { surfaces };
  },
};

/** 핸들러 표에서 이름을 뽑아 `connected` 이벤트의 기능 목록과 디스패치가 같은 출처를 쓰게 한다 */
const SUPPORTED_COMMANDS = Object.keys(COMMAND_HANDLERS);

/** 형식이 맞는 명령이면 파싱한 메시지를, 응답할 길이 없으면 null을 돌려준다 */
function parseCommandMessage(rawMessage) {
  if (rawMessage.length > MAX_COMMAND_BYTES) {
    return null;
  }

  let parsed;
  try {
    parsed = JSON.parse(rawMessage.toString("utf8"));
  } catch {
    return null;
  }

  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed) || parsed.type !== "command") {
    return null;
  }
  const { requestId } = parsed;
  if (typeof requestId !== "string" || requestId.length === 0 || requestId.length > MAX_REQUEST_ID_LENGTH) {
    return null;
  }
  return parsed;
}

function failureResult(requestId, code, message) {
  return { type: "command-result", requestId, ok: false, code, error: message };
}

/** 브리지 오류는 이름으로 가린다. `mobileRoutes.js`의 HTTP 응답이 같은 오류를 이름으로 가리는 것과 같은 방식이다 */
function toFailureResult(requestId, error) {
  if (error instanceof CommandFailure) {
    return failureResult(requestId, error.code, error.message);
  }
  switch (error?.name) {
    case "DirtyWorktreeError": return failureResult(requestId, "dirty-worktree", "커밋하지 않은 변경이 있습니다");
    case "PaneNotFoundError": return failureResult(requestId, "pane-not-found", error.message);
    case "LastPaneError": return failureResult(requestId, "last-tab", error.message);
    case "SurfaceUnavailableError": return failureResult(requestId, "session-unavailable", "세션을 사용할 수 없습니다");
    default:
      console.error("[kanvibe] Mobile command failed:", error);
      return failureResult(requestId, "server-error", "서버 오류");
  }
}

async function runMobileCommand(bridge, rawMessage) {
  const rawBytes = typeof rawMessage === "string" ? Buffer.from(rawMessage) : rawMessage;
  const message = Buffer.isBuffer(rawBytes) ? parseCommandMessage(rawBytes) : null;
  if (!message) {
    return null;
  }

  const { requestId, command } = message;
  if (typeof command !== "string") {
    return failureResult(requestId, "invalid-command", "명령 형식이 올바르지 않습니다");
  }
  if (!Object.hasOwn(COMMAND_HANDLERS, command)) {
    return failureResult(requestId, "unsupported-command", "이 데스크탑이 지원하지 않는 명령입니다");
  }

  try {
    const payload = await COMMAND_HANDLERS[command](bridge, message);
    return { type: "command-result", requestId, ok: true, ...payload };
  } catch (error) {
    return toFailureResult(requestId, error);
  }
}

module.exports = { runMobileCommand, SUPPORTED_COMMANDS };
