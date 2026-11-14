/**
 * `/api/mobile/events` 소켓으로 들어온 명령 한 건을 검증해 브리지에 넘기고, 응답 메시지 한 건을 만든다.
 *
 * 소켓의 인증과 수명은 `mobileRoutes.js`가 맡는다. 인증을 통과한 기기만 이 소켓을 열 수 있으므로 여기서는 다시 묻지 않고,
 * 메시지 하나가 응답 하나가 되는 일만 한다. 응답을 보낼 곳이 없는 메시지는 `null`을 돌려 무시한다.
 */

const MAX_COMMAND_BYTES = 1024;
const MAX_REQUEST_ID_LENGTH = 64;
/** 한 화면이 동시에 보는 pane은 채팅 하나와 선택된 터미널 하나 정도다. 명령 크기 제한 안에 들어오는 수로 묶는다 */
const MAX_SUBSCRIBED_PANES = 4;

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

/** 구독자가 대화를 어디까지 받았는지. 처음 구독할 때는 없다 */
function readPaneChatCursor(value) {
  if (value === undefined || value === null) {
    return null;
  }
  if (typeof value !== "object" || typeof value.epoch !== "string" || !Number.isSafeInteger(value.seq) || value.seq < 0) {
    throw invalidCommand();
  }
  return { epoch: value.epoch, seq: value.seq };
}

function readPaneChatSubscriptionTarget(value) {
  if (value === null || typeof value !== "object") {
    throw invalidCommand();
  }
  if (value.detail !== "chat" && value.detail !== "agent") {
    throw invalidCommand();
  }
  if (value.wantsScreen !== undefined && typeof value.wantsScreen !== "boolean") {
    throw invalidCommand();
  }
  return {
    taskId: readNonEmptyString(value.taskId),
    paneId: readNonEmptyString(value.paneId),
    detail: value.detail,
    wantsScreen: value.wantsScreen === true,
    cursor: readPaneChatCursor(value.cursor),
  };
}

/**
 * 각 핸들러는 성공 응답에 펼칠 값을 돌려주고, 실패는 던진다.
 * 세 번째 인자는 명령을 보낸 소켓이다. 응답 한 번으로 끝나지 않고 그 소켓에 계속 밀어 줘야 하는 명령만 쓴다.
 */
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
  "remove-task-registration": async (bridge, message) => {
    const isRemoved = await bridge.removeMobileTaskRegistration(readNonEmptyString(message.taskId));
    if (!isRemoved) {
      throw new CommandFailure("task-not-found", "없는 태스크입니다");
    }
    return { failedPeerCount: 0 };
  },
  "get-tab-close-info": async (bridge, message) => {
    const { processNames } = await bridge.getMobilePaneCloseInfo(readNonEmptyString(message.taskId), readNonEmptyString(message.paneId));
    return { processNames };
  },
  "close-tab": async (bridge, message) => {
    const { surfaces } = await bridge.closeMobilePane(readNonEmptyString(message.taskId), readNonEmptyString(message.paneId));
    return { surfaces };
  },
  "start-session": async (bridge, message) => {
    const taskId = readNonEmptyString(message.taskId);
    try {
      return { surfaces: await bridge.getTaskSurfaces(taskId) };
    } catch (error) {
      if (error?.name === "SurfaceUnavailableError") {
        throw new CommandFailure(error.reason, "세션을 시작하지 못했습니다");
      }
      throw error;
    }
  },
  "set-pane-chat-subscription": async (bridge, message, connection) => {
    if (!Array.isArray(message.panes) || message.panes.length > MAX_SUBSCRIBED_PANES) {
      throw invalidCommand();
    }
    const targets = message.panes.map(readPaneChatSubscriptionTarget);
    return { panes: bridge.setPaneChatSubscriptions(connection.owner, targets, connection.send) };
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
    case "InvalidWorktreeError": return failureResult(requestId, "invalid-worktree", error.message);
    case "PaneNotFoundError": return failureResult(requestId, "pane-not-found", error.message);
    case "LastPaneError": return failureResult(requestId, "last-tab", error.message);
    case "SurfaceUnavailableError": return failureResult(requestId, "session-unavailable", "세션을 사용할 수 없습니다");
    default:
      console.error("[kanvibe] Mobile command failed:", error);
      return failureResult(requestId, "server-error", "서버 오류");
  }
}

/** @param connection 명령을 보낸 소켓. `owner`는 구독의 수명을 쥔 식별자이고 `send`는 그 소켓 하나에만 이벤트를 보낸다 */
async function runMobileCommand(bridge, rawMessage, connection) {
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
    const payload = await COMMAND_HANDLERS[command](bridge, message, connection);
    return { type: "command-result", requestId, ok: true, ...payload };
  } catch (error) {
    return toFailureResult(requestId, error);
  }
}

module.exports = { runMobileCommand, SUPPORTED_COMMANDS };
