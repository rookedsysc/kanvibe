//! 이벤트 소켓의 명령 한 건(TS `electron/mobileCommands.js`). 메시지 하나가 응답 하나가 되고, 응답할 길이 없으면 무시한다.

use std::sync::Arc;

use serde_json::{Map, Value, json};

use super::socket::SocketHandle;
use crate::bridge::{BridgeError, MobileBridge};
use crate::routes::mobile::call_bridge;
use crate::server::ServerState;

const MAX_COMMAND_BYTES: usize = 1024;
const MAX_REQUEST_ID_LENGTH: usize = 64;
/// 한 화면이 동시에 보는 pane은 채팅 하나와 선택된 터미널 하나 정도다
const MAX_SUBSCRIBED_PANES: usize = 4;

/// 핸들러 표의 이름. `connected` 이벤트의 기능 목록과 디스패치가 같은 출처를 쓴다
pub const SUPPORTED_COMMANDS: [&str; 8] = [
    "delete-task",
    "remove-task-registration",
    "get-tab-close-info",
    "close-tab",
    "start-session",
    "set-pane-chat-subscription",
    "get-pane-chat-modes",
    "set-pane-chat-mode",
];

/// 앱이 `code`로 분기하고 `error`는 그대로 보여 주는 실패
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandFailure {
    code: String,
    message: String,
}

impl CommandFailure {
    fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }

    fn invalid() -> Self {
        Self::new("invalid-command", "명령 형식이 올바르지 않습니다")
    }

    fn task_not_found() -> Self {
        Self::new("task-not-found", "없는 태스크입니다")
    }
}

/// 브리지 오류는 이름으로 가린다(HTTP 경로와 같은 방식)
impl From<BridgeError> for CommandFailure {
    fn from(error: BridgeError) -> Self {
        match error {
            BridgeError::DirtyWorktree(_) => {
                Self::new("dirty-worktree", "커밋하지 않은 변경이 있습니다")
            }
            BridgeError::InvalidWorktree(message) => Self::new("invalid-worktree", &message),
            BridgeError::PaneNotFound(message) => Self::new("pane-not-found", &message),
            BridgeError::LastPane(message) => Self::new("last-tab", &message),
            BridgeError::SurfaceUnavailable(_) => {
                Self::new("session-unavailable", "세션을 사용할 수 없습니다")
            }
            _ => Self::new("server-error", "서버 오류"),
        }
    }
}

/// 형식이 맞는 명령이면 파싱한 메시지를 돌려준다
fn parse_command_message(raw: &[u8]) -> Option<Map<String, Value>> {
    if raw.len() > MAX_COMMAND_BYTES {
        return None;
    }
    let Ok(Value::Object(message)) = serde_json::from_str::<Value>(&String::from_utf8_lossy(raw))
    else {
        return None;
    };
    if message.get("type") != Some(&Value::String("command".into())) {
        return None;
    }
    let request_id = message.get("requestId")?.as_str()?;
    let length = kanvibe_ai::javascript::utf16_length(request_id);
    (1..=MAX_REQUEST_ID_LENGTH)
        .contains(&length)
        .then_some(message)
}

pub async fn run_mobile_command(
    state: &Arc<ServerState>,
    raw: &[u8],
    socket: &Arc<SocketHandle>,
) -> Option<Value> {
    let message = parse_command_message(raw)?;
    let request_id = message.get("requestId").cloned().unwrap_or(Value::Null);
    let Some(command) = message
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Some(failure_result(&request_id, &CommandFailure::invalid()));
    };
    if !SUPPORTED_COMMANDS.contains(&command.as_str()) {
        let failure = CommandFailure::new(
            "unsupported-command",
            "이 데스크탑이 지원하지 않는 명령입니다",
        );
        return Some(failure_result(&request_id, &failure));
    }
    let outcome = run_command(state, &command, message, socket).await;
    Some(match outcome {
        Ok(payload) => {
            let mut result = Map::new();
            result.insert("type".into(), "command-result".into());
            result.insert("requestId".into(), request_id);
            result.insert("ok".into(), Value::Bool(true));
            result.extend(payload);
            Value::Object(result)
        }
        Err(failure) => failure_result(&request_id, &failure),
    })
}

fn failure_result(request_id: &Value, failure: &CommandFailure) -> Value {
    json!({ "type": "command-result", "requestId": request_id, "ok": false, "code": failure.code, "error": failure.message })
}

fn read_non_empty_string(
    message: &Map<String, Value>,
    name: &str,
) -> Result<String, CommandFailure> {
    match message.get(name) {
        Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
        _ => Err(CommandFailure::invalid()),
    }
}

async fn bridge_call<T: Send + 'static>(
    state: &ServerState,
    call: impl FnOnce(&dyn MobileBridge) -> Result<T, BridgeError> + Send + 'static,
) -> Result<T, CommandFailure> {
    call_bridge(state, call).await.map_err(CommandFailure::from)
}

fn single(name: &str, value: Value) -> Map<String, Value> {
    let mut payload = Map::new();
    payload.insert(name.to_string(), value);
    payload
}

async fn run_command(
    state: &Arc<ServerState>,
    command: &str,
    message: Map<String, Value>,
    socket: &Arc<SocketHandle>,
) -> Result<Map<String, Value>, CommandFailure> {
    match command {
        "delete-task" => delete_task(state, &message).await,
        "remove-task-registration" => {
            let task_id = read_non_empty_string(&message, "taskId")?;
            let is_removed = bridge_call(state, move |bridge| {
                bridge.remove_mobile_task_registration(&task_id)
            })
            .await?;
            if !is_removed {
                return Err(CommandFailure::task_not_found());
            }
            Ok(single("failedPeerCount", json!(0)))
        }
        "get-tab-close-info" | "close-tab" => pane_command(state, command, &message).await,
        "start-session" => start_session(state, &message).await,
        "set-pane-chat-subscription" => subscribe_pane_chat(state, &message, socket).await,
        "get-pane-chat-modes" => {
            let task_id = read_non_empty_string(&message, "taskId")?;
            let modes =
                bridge_call(state, move |bridge| bridge.get_pane_chat_modes(&task_id)).await?;
            Ok(single("modes", modes))
        }
        _ => set_pane_chat_mode(state, &message).await,
    }
}

async fn delete_task(
    state: &ServerState,
    message: &Map<String, Value>,
) -> Result<Map<String, Value>, CommandFailure> {
    let task_id = read_non_empty_string(message, "taskId")?;
    let is_forced = match message.get("isForced") {
        None => false,
        Some(Value::Bool(is_forced)) => *is_forced,
        Some(_) => return Err(CommandFailure::invalid()),
    };
    let deletion = bridge_call(state, move |bridge| {
        bridge.delete_mobile_task(&task_id, is_forced)
    })
    .await?;
    let failed_peer_count = deletion.ok_or_else(CommandFailure::task_not_found)?;
    Ok(single("failedPeerCount", json!(failed_peer_count)))
}

async fn pane_command(
    state: &ServerState,
    command: &str,
    message: &Map<String, Value>,
) -> Result<Map<String, Value>, CommandFailure> {
    let task_id = read_non_empty_string(message, "taskId")?;
    let pane_id = read_non_empty_string(message, "paneId")?;
    if command == "close-tab" {
        let surfaces = bridge_call(state, move |bridge| {
            bridge.close_mobile_pane(&task_id, &pane_id)
        })
        .await?;
        return Ok(single("surfaces", surfaces));
    }
    let info = bridge_call(state, move |bridge| {
        bridge.get_mobile_pane_close_info(&task_id, &pane_id)
    })
    .await?;
    Ok(single(
        "processNames",
        info.get("processNames").cloned().unwrap_or(Value::Null),
    ))
}

async fn start_session(
    state: &ServerState,
    message: &Map<String, Value>,
) -> Result<Map<String, Value>, CommandFailure> {
    let task_id = read_non_empty_string(message, "taskId")?;
    match call_bridge(state, move |bridge| {
        bridge.get_task_surfaces(&task_id, None)
    })
    .await
    {
        Ok(surfaces) => Ok(single("surfaces", surfaces)),
        Err(BridgeError::SurfaceUnavailable(reason)) => {
            Err(CommandFailure::new(&reason, "세션을 시작하지 못했습니다"))
        }
        Err(error) => Err(error.into()),
    }
}

/// 구독 대상 형식을 본 뒤 정규화한 값을 서비스에 넘긴다
fn read_subscription_targets(message: &Map<String, Value>) -> Result<Value, CommandFailure> {
    let Some(Value::Array(panes)) = message.get("panes") else {
        return Err(CommandFailure::invalid());
    };
    if panes.len() > MAX_SUBSCRIBED_PANES {
        return Err(CommandFailure::invalid());
    }
    let targets: Result<Vec<Value>, CommandFailure> =
        panes.iter().map(read_subscription_target).collect();
    Ok(Value::Array(targets?))
}

fn read_subscription_target(value: &Value) -> Result<Value, CommandFailure> {
    let Value::Object(target) = value else {
        return Err(CommandFailure::invalid());
    };
    let detail = target
        .get("detail")
        .and_then(Value::as_str)
        .filter(|detail| matches!(*detail, "chat" | "agent"));
    let detail = detail.ok_or_else(CommandFailure::invalid)?;
    let wants_screen = match target.get("wantsScreen") {
        None => false,
        Some(Value::Bool(wants_screen)) => *wants_screen,
        Some(_) => return Err(CommandFailure::invalid()),
    };
    let task_id = read_non_empty_string(target, "taskId")?;
    let pane_id = read_non_empty_string(target, "paneId")?;
    let cursor = read_cursor(target.get("cursor"))?;
    Ok(
        json!({ "taskId": task_id, "paneId": pane_id, "detail": detail, "wantsScreen": wants_screen, "cursor": cursor }),
    )
}

/// 구독자가 대화를 어디까지 받았는지. 처음 구독할 때는 없다
fn read_cursor(value: Option<&Value>) -> Result<Value, CommandFailure> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(Value::Null);
    };
    let epoch = value.get("epoch").and_then(Value::as_str);
    let seq = value
        .get("seq")
        .and_then(Value::as_f64)
        .filter(|seq| *seq >= 0.0 && seq.fract() == 0.0 && *seq <= 9_007_199_254_740_991.0);
    match (epoch, seq) {
        (Some(epoch), Some(seq)) => Ok(json!({ "epoch": epoch, "seq": seq as u64 })),
        _ => Err(CommandFailure::invalid()),
    }
}

async fn subscribe_pane_chat(
    state: &ServerState,
    message: &Map<String, Value>,
    socket: &Arc<SocketHandle>,
) -> Result<Map<String, Value>, CommandFailure> {
    let targets = read_subscription_targets(message)?;
    let owner = socket.id;
    let push_socket = socket.clone();
    let send = Arc::new(move |event: Value| push_socket.send_json(&event));
    let panes = bridge_call(state, move |bridge| {
        bridge.set_pane_chat_subscriptions(owner, &targets, send)
    })
    .await?;
    Ok(single("panes", panes))
}

async fn set_pane_chat_mode(
    state: &ServerState,
    message: &Map<String, Value>,
) -> Result<Map<String, Value>, CommandFailure> {
    let task_id = read_non_empty_string(message, "taskId")?;
    let pane_id = read_non_empty_string(message, "paneId")?;
    let Some(Value::Bool(is_enabled)) = message.get("isEnabled").cloned() else {
        return Err(CommandFailure::invalid());
    };
    bridge_call(state, move |bridge| {
        bridge.set_pane_chat_mode(&task_id, &pane_id, is_enabled)
    })
    .await?;
    Ok(Map::new())
}
