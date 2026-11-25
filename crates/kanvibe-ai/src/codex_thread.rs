//! 주소로 고정한 Codex 대화 하나에 대한 app-server 작업. 입력 보내기, 첫 입력 전 빈 대화 확인, 모델 설정 읽기·바꾸기를 한다.
//! TS `codexThreadInput.ts`, `codexThreadMetadata.ts`, `codexThreadSettings.ts`를 옮긴다.
//!
//! 같은 계정·작업의 메인 대화만 허용하고, 본문이나 다른 대화의 경로를 입력 대상으로 쓰지 않는다.
//! 대화를 재개·구독하거나 권한·로그인·작업 디렉터리를 바꾸지 않는다.

#[cfg(test)]
mod tests;

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::codex_app_server::{
    CodexAppServerConnection, CodexAppServerTarget, with_codex_app_server,
};
use crate::codex_sessions::is_codex_thread_id;
use crate::error::{AiError, AiResult};
use crate::javascript::js_string;
use crate::node_path;
use crate::pane_chat::model_settings::{
    PaneChatEffortLevel, PaneChatModelOption, PaneChatModelOptionKeyOrder, PaneChatModelProvider,
    PaneChatModelSettingChange, PaneChatModelSettings, build_pane_chat_model_command,
};
use crate::session_listing::determine_match_scope;

const MAIN_SOURCES: [&str; 4] = ["cli", "vscode", "exec", "appServer"];

/// app-server에 보내는 사용자 입력 조각
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
pub enum CodexUserInput {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "localImage")]
    LocalImage { path: String },
}

/// 같은 계정·작업의 메인 대화이면 그 대화의 rollout 경로를, 아니면 `None`을 돌려준다
pub fn codex_main_thread_source(
    thread: &Value,
    target: &CodexAppServerTarget<'_>,
    codex_home: &str,
    thread_id: &str,
) -> Option<String> {
    let cwd = thread.get("cwd").and_then(Value::as_str);
    let source = thread
        .get("source")
        .map_or_else(|| "undefined".to_string(), js_string);
    let is_main_thread = is_codex_thread_id(thread_id)
        && thread.get("id").and_then(Value::as_str) == Some(thread_id)
        && cwd.is_some()
        && determine_match_scope(cwd, target.context).is_some()
        && thread.get("parentThreadId") == Some(&Value::Null)
        && MAIN_SOURCES.contains(&source.as_str())
        && thread.get("ephemeral") == Some(&Value::Bool(false))
        && thread.get("canAcceptDirectInput") == Some(&Value::Bool(true));
    let thread_path = thread
        .get("path")
        .and_then(Value::as_str)
        .filter(|_| is_main_thread)?;
    let source_path = node_path::normalize(thread_path);
    let is_account_record = source_path.starts_with(&format!("{codex_home}/sessions/"))
        && node_path::basename(&source_path).ends_with(&format!("-{thread_id}.jsonl"));
    is_account_record.then_some(source_path)
}

fn read_thread(connection: &mut CodexAppServerConnection, thread_id: &str) -> AiResult<Value> {
    let result = connection.request(
        "thread/read",
        json!({ "threadId": thread_id, "includeTurns": false }),
        None,
    )?;
    Ok(result.get("thread").cloned().unwrap_or(Value::Null))
}

/// 첫 입력 전 JSONL이 없는 새 대화만 확인한다. 기존 TUI의 구독이나 설정을 바꾸지 않고, 어떤 실패도 `None`으로 본다
pub fn read_codex_empty_thread(
    target: &CodexAppServerTarget<'_>,
    thread_id: &str,
) -> Option<String> {
    if !is_codex_thread_id(thread_id) {
        return None;
    }
    with_codex_app_server(target, |connection| {
        let thread = read_thread(connection, thread_id)?;
        let source = codex_main_thread_source(&thread, target, connection.codex_home(), thread_id);
        let Some(source) =
            source.filter(|_| thread.get("preview").and_then(Value::as_str) == Some(""))
        else {
            return Ok(None);
        };
        Ok((!target.context.host.path_exists(&source)?).then_some(source))
    })
    .ok()
    .flatten()
}

/// 입력을 보낼 대화. `client_user_message_id`는 표시 연결용이며 서버 중복 방지나 재시도 권한이 아니다
pub struct CodexThreadInput<'a> {
    pub target: CodexAppServerTarget<'a>,
    pub thread_id: String,
    pub source_ref: String,
    pub input: Vec<CodexUserInput>,
    pub client_user_message_id: String,
}

/// 주소로 고정한 대화에 입력을 한 번 보낸다. `before_send`가 실패하면 보내지 않고, 응답을 잃어도 다시 보내지 않는다
pub fn send_codex_thread_input(
    request: &CodexThreadInput<'_>,
    before_send: impl FnOnce() -> AiResult<()>,
    mut on_write: impl FnMut(),
) -> AiResult<()> {
    if request.input.is_empty() || request.client_user_message_id.is_empty() {
        return Err(AiError::message("보낼 Codex 입력이 없습니다."));
    }
    with_codex_app_server(&request.target, |connection| {
        let thread = read_thread(connection, &request.thread_id)?;
        let source = codex_main_thread_source(
            &thread,
            &request.target,
            connection.codex_home(),
            &request.thread_id,
        );
        if source.as_deref() != Some(request.source_ref.as_str()) {
            return Err(AiError::message(
                "CLI와 같은 작업의 Codex 대화를 확인하지 못했습니다.",
            ));
        }
        let status = thread.get("status").and_then(|status| status.get("type"));
        let status_name = status.map_or_else(|| "undefined".to_string(), js_string);
        if !["idle", "active"].contains(&status_name.as_str()) {
            return Err(AiError::message("Codex가 입력을 받을 수 없는 상태입니다."));
        }
        before_send()?;
        let params = json!({
            "threadId": request.thread_id,
            "input": request.input,
            "clientUserMessageId": request.client_user_message_id,
        });
        let result = connection.request("turn/start", params, Some(&mut on_write))?;
        let turn_id = result.get("turn").and_then(|turn| turn.get("id"));
        if !turn_id.is_some_and(crate::javascript::is_truthy) {
            return Err(AiError::message(
                "Codex 입력 수락 결과를 확인하지 못했습니다.",
            ));
        }
        Ok(())
    })
}

/// 모델 설정을 읽거나 바꿀 대화
pub struct CodexThreadSettingsTarget<'a> {
    pub target: CodexAppServerTarget<'a>,
    pub thread_id: String,
    pub source_ref: String,
}

fn read_settings(
    connection: &mut CodexAppServerConnection,
    settings_target: &CodexThreadSettingsTarget<'_>,
) -> AiResult<PaneChatModelSettings> {
    let thread = read_thread(connection, &settings_target.thread_id)?;
    let source = codex_main_thread_source(
        &thread,
        &settings_target.target,
        connection.codex_home(),
        &settings_target.thread_id,
    );
    if source.as_deref() != Some(settings_target.source_ref.as_str()) {
        return Err(AiError::message(
            "현재 CLI의 Codex 대화를 확인하지 못했습니다.",
        ));
    }
    let models = read_model_list(
        connection,
        thread.get("modelProvider").and_then(Value::as_str),
    )?;
    Ok(PaneChatModelSettings {
        model: thread
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        effort: thread
            .get("reasoningEffort")
            .and_then(Value::as_str)
            .map(str::to_string),
        is_fast: None,
        models: models
            .iter()
            .filter(|model| !crate::javascript::is_present_truthy(model.get("hidden")))
            .map(to_model_option)
            .collect::<AiResult<_>>()?,
    })
}

/// `model/list`를 다음 커서가 없을 때까지 읽는다. 같은 커서가 되돌아오면 끝없이 돌지 않고 실패한다
fn read_model_list(
    connection: &mut CodexAppServerConnection,
    model_provider: Option<&str>,
) -> AiResult<Vec<Value>> {
    let mut models = Vec::new();
    let mut cursor = Value::Null;
    loop {
        let mut params = Map::new();
        params.insert("cursor".to_string(), cursor.clone());
        params.insert("limit".to_string(), json!(100));
        params.insert("includeHidden".to_string(), json!(false));
        if let Some(provider) = model_provider {
            params.insert("modelProvider".to_string(), json!(provider));
        }
        let page = connection.request("model/list", Value::Object(params), None)?;
        let data = page
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(model_list_error)?;
        models.extend(data.iter().cloned());
        let next_cursor = page.get("nextCursor").cloned().unwrap_or(Value::Null);
        if next_cursor == cursor && !cursor.is_null() {
            return Err(AiError::message(
                "Codex 모델 목록을 끝까지 읽지 못했습니다.",
            ));
        }
        cursor = next_cursor;
        if !crate::javascript::is_truthy(&cursor) {
            return Ok(models);
        }
    }
}

fn model_list_error() -> AiError {
    AiError::message("Codex 모델 목록을 읽지 못했습니다.")
}

fn to_model_option(model: &Value) -> AiResult<PaneChatModelOption> {
    let text = |key: &str| {
        model
            .get(key)
            .map_or_else(|| "undefined".to_string(), js_string)
    };
    let efforts = model
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .ok_or_else(model_list_error)?;
    let supports_fast = model
        .get("serviceTiers")
        .and_then(Value::as_array)
        .is_some_and(|tiers| {
            tiers
                .iter()
                .any(|tier| tier.get("id").and_then(Value::as_str) == Some("priority"))
        });
    Ok(PaneChatModelOption {
        id: text("model"),
        label: text("displayName"),
        effort_levels: efforts
            .iter()
            .map(|level| {
                let id = level
                    .get("reasoningEffort")
                    .map_or_else(|| "undefined".to_string(), js_string);
                PaneChatEffortLevel {
                    id: id.clone(),
                    label: id,
                }
            })
            .collect(),
        supports_fast,
        key_order: PaneChatModelOptionKeyOrder::EffortLevelsFirst,
    })
}

/// 기존 CLI 계정의 실제 모델 목록과 현재 대화 설정을 API에서 읽는다
pub fn read_codex_thread_settings(
    settings_target: &CodexThreadSettingsTarget<'_>,
) -> AiResult<PaneChatModelSettings> {
    with_codex_app_server(&settings_target.target, |connection| {
        read_settings(connection, settings_target)
    })
}

/// 같은 대화의 이후 턴 설정만 바꾼다. 목록에 없는 값이면 보내지 않는다
pub fn update_codex_thread_settings(
    settings_target: &CodexThreadSettingsTarget<'_>,
    change: &PaneChatModelSettingChange,
    before_write: impl FnOnce() -> AiResult<()>,
) -> AiResult<()> {
    with_codex_app_server(&settings_target.target, |connection| {
        let settings = read_settings(connection, settings_target)?;
        if build_pane_chat_model_command(PaneChatModelProvider::Codex, &settings, change).is_none()
        {
            return Err(AiError::message("현재 모델에서 지원하지 않는 설정입니다."));
        }
        let mut params = Map::new();
        params.insert("threadId".to_string(), json!(settings_target.thread_id));
        let (key, value) = match change {
            PaneChatModelSettingChange::Model { model } => ("model", json!(model)),
            PaneChatModelSettingChange::Effort { effort } => ("effort", json!(effort)),
            PaneChatModelSettingChange::Fast { is_fast } => (
                "serviceTier",
                if *is_fast {
                    json!("priority")
                } else {
                    Value::Null
                },
            ),
            PaneChatModelSettingChange::Reset => ("effort", Value::Null),
        };
        params.insert(key.to_string(), value);
        before_write()?;
        connection.request("thread/settings/update", Value::Object(params), None)?;
        Ok(())
    })
}
