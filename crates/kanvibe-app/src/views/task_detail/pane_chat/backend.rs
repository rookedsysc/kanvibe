//! pane 채팅 화면이 부르는 서비스 경계. TS 렌더러의 `invokeDesktop("paneChat", 메서드, ...인자)` 한 갈래를 그대로 옮겨,
//! 호출 이름과 JSON 인자가 TS IPC와 같다. 그래서 QA 고장 주입(`paneChat.getPaneChatSnapshot` 같은 이름)과
//! 테스트의 가짜 서비스가 같은 자리에서 끼어든다. 실제 호출은 앱 런타임의 pane chat 서비스 하나로 간다.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use gpui_kit::{App, BackgroundExecutor};
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_runtime::DesktopRuntime;
use kanvibe_terminal::pane_chat::service::PaneChatError;
use serde_json::{Value, json};

use crate::qa::faults::{FaultOutcome, FaultRegistry};
use crate::services::{AppServices, apply_fault};

/// 호출 한 번의 결과. 실제 서비스는 백그라운드 스레드에서 이 future를 기다리는 동안 돈다
pub type PaneChatCall = Pin<Box<dyn Future<Output = Result<Value, PaneChatCallError>> + Send>>;

/// TS IPC 호출 하나. `target`은 `namespace.method`, `arguments`는 TS 인자 순서 그대로다(없는 값은 `null`)
#[derive(Debug, Clone, PartialEq)]
pub struct PaneChatRequest {
    pub target: &'static str,
    pub arguments: Vec<Value>,
}

/// 서비스가 거절한 이유. 화면은 `message`를 그대로 보여 준다(TS `describeActionError`가 IPC 머리말을 뗀 글)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatCallError {
    pub message: String,
    /// CLI에 글을 넣기 시작한 뒤 실패해 전달됐는지 알 수 없다(TS `PaneChatDeliveryUncertainError`)
    pub is_delivery_uncertain: bool,
}

/// Electron이 IPC 오류 앞에 붙이던 호출 경로와 오류 이름. 고장 주입 문구에 붙어 오면 떼어 낸다
const IPC_ERROR_PREFIX: &str = "Error invoking remote method '";
const DELIVERY_UNCERTAIN_ERROR_NAME: &str = "PaneChatDeliveryUncertainError";

impl PaneChatCallError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            is_delivery_uncertain: false,
        }
    }

    /// TS와 같은 모양의 오류 글(`Error invoking remote method '…': 이름: 안내`)에서 안내만 남긴다
    pub fn from_ipc_message(raw: &str) -> Self {
        let is_delivery_uncertain = raw.contains(DELIVERY_UNCERTAIN_ERROR_NAME);
        Self {
            message: strip_ipc_prefix(raw).to_string(),
            is_delivery_uncertain,
        }
    }
}

fn strip_ipc_prefix(raw: &str) -> &str {
    let Some(rest) = raw.strip_prefix(IPC_ERROR_PREFIX) else {
        return raw;
    };
    let Some((_, message)) = rest.split_once("': ") else {
        return raw;
    };
    match message.split_once(": ") {
        Some((name, detail)) if !name.is_empty() && name.chars().all(char::is_alphanumeric) => {
            detail
        }
        _ => message,
    }
}

impl From<PaneChatError> for PaneChatCallError {
    fn from(error: PaneChatError) -> Self {
        Self {
            is_delivery_uncertain: matches!(error, PaneChatError::DeliveryUncertain { .. }),
            message: error.to_string(),
        }
    }
}

/// pane CLI가 새 대화 ID를 알렸을 때 부르는 알림. 서비스의 감시 스레드에서 불린다
pub type PaneChatConnectionSignal = Arc<dyn Fn() + Send + Sync>;

/// 걸어 둔 연결 신호 구독. 값을 버리면 멈춘다
pub type PaneChatConnectionWatch = Box<dyn Send>;

/// 화면이 쓰는 pane chat 서비스. 앱은 [`RuntimePaneChatBackend`]를, 테스트는 가짜를 넘긴다
pub trait PaneChatBackend: Send + Sync {
    fn call(&self, request: PaneChatRequest) -> PaneChatCall;

    /// 그 Task의 연결 신호를 받는다. 신호를 낼 수 없는 서비스는 `None`이고, 화면은 조회 주기로만 연결한다
    fn watch_connection(
        &self,
        _task_id: &str,
        _signal: PaneChatConnectionSignal,
    ) -> Option<PaneChatConnectionWatch> {
        None
    }
}

/// 앱 런타임의 pane chat 서비스와 AppSettings를 부르는 실제 경계. QA 모드면 고장 주입을 먼저 거친다
pub struct RuntimePaneChatBackend {
    runtime: Option<Arc<DesktopRuntime>>,
    faults: Option<Arc<FaultRegistry>>,
    executor: BackgroundExecutor,
}

impl RuntimePaneChatBackend {
    pub fn from_app(cx: &App) -> Arc<dyn PaneChatBackend> {
        let services = cx.try_global::<AppServices>();
        Arc::new(Self {
            runtime: services.and_then(|services| services.runtime().cloned()),
            faults: services.and_then(|services| services.faults().cloned()),
            executor: cx.background_executor().clone(),
        })
    }
}

impl PaneChatBackend for RuntimePaneChatBackend {
    fn call(&self, request: PaneChatRequest) -> PaneChatCall {
        let serialized_arguments = Value::Array(request.arguments.clone()).to_string();
        let fault = self
            .faults
            .as_ref()
            .map(|faults| faults.check(request.target, &serialized_arguments))
            .unwrap_or(FaultOutcome::Proceed);
        let runtime = self.runtime.clone();
        let executor = self.executor.clone();
        Box::pin(async move {
            if let FaultOutcome::Result(value) = fault {
                return Ok(value);
            }
            apply_fault(fault, &executor)
                .await
                .map_err(|failure| PaneChatCallError::from_ipc_message(&failure.reason))?;
            let runtime = runtime
                .ok_or_else(|| PaneChatCallError::new("pane chat runtime is unavailable"))?;
            dispatch(&runtime, &request)
        })
    }

    fn watch_connection(
        &self,
        task_id: &str,
        signal: PaneChatConnectionSignal,
    ) -> Option<PaneChatConnectionWatch> {
        let runtime = self.runtime.as_ref()?;
        let watch = runtime
            .pane_chat()
            .watch_pane_chat_connection(task_id, signal);
        Some(Box::new(watch))
    }
}

fn text_argument(request: &PaneChatRequest, index: usize) -> &str {
    request
        .arguments
        .get(index)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn optional_text_argument(request: &PaneChatRequest, index: usize) -> Option<&str> {
    request.arguments.get(index).and_then(Value::as_str)
}

fn argument(request: &PaneChatRequest, index: usize) -> &Value {
    request.arguments.get(index).unwrap_or(&Value::Null)
}

fn success(result: Result<(), PaneChatError>) -> Result<Value, PaneChatCallError> {
    result
        .map(|_| json!({ "success": true }))
        .map_err(Into::into)
}

fn to_value<T: serde::Serialize>(
    result: Result<T, PaneChatError>,
) -> Result<Value, PaneChatCallError> {
    let value = result?;
    serde_json::to_value(value).map_err(|error| PaneChatCallError::new(error.to_string()))
}

fn dispatch(
    runtime: &DesktopRuntime,
    request: &PaneChatRequest,
) -> Result<Value, PaneChatCallError> {
    match request.target {
        "appSettings.getAppSetting" | "appSettings.setAppSetting" => {
            dispatch_app_setting(runtime, request)
        }
        "paneChat.getPaneChatLayout"
        | "paneChat.getPaneChatAgentProvider"
        | "paneChat.getPaneChatSnapshot"
        | "paneChat.focusPaneChatTerminal"
        | "paneChat.getPaneChatModes"
        | "paneChat.setPaneChatMode"
        | "paneChat.getPaneChatTranscriptPage" => dispatch_reading(runtime, request),
        _ => dispatch_action(runtime, request),
    }
}

fn dispatch_app_setting(
    runtime: &DesktopRuntime,
    request: &PaneChatRequest,
) -> Result<Value, PaneChatCallError> {
    let settings = AppSettingsService::new(runtime.database(), runtime.shortcut_platform());
    let key = text_argument(request, 0);
    let result = match request.target {
        "appSettings.getAppSetting" => settings.get_app_setting(key).map(|value| json!(value)),
        _ => settings
            .set_app_setting(key, text_argument(request, 1))
            .map(|_| Value::Null),
    };
    result.map_err(|error| PaneChatCallError::new(error.to_string()))
}

/// 화면을 읽거나 pane 모드를 다루는 호출
fn dispatch_reading(
    runtime: &DesktopRuntime,
    request: &PaneChatRequest,
) -> Result<Value, PaneChatCallError> {
    let pane_chat = runtime.pane_chat();
    let service = pane_chat.service();
    let task_id = text_argument(request, 0);
    match request.target {
        "paneChat.getPaneChatLayout" => {
            to_value(service.get_pane_chat_layout(task_id, optional_text_argument(request, 1)))
        }
        "paneChat.getPaneChatAgentProvider" => Ok(json!(
            pane_chat.get_pane_chat_agent_provider(task_id, text_argument(request, 1))
        )),
        "paneChat.getPaneChatSnapshot" => {
            to_value(service.get_pane_chat_snapshot(task_id, optional_text_argument(request, 1)))
        }
        "paneChat.focusPaneChatTerminal" => {
            success(service.focus_pane_chat_terminal(task_id, text_argument(request, 1)))
        }
        "paneChat.getPaneChatModes" => to_value(pane_chat.get_pane_chat_modes(task_id)),
        "paneChat.setPaneChatMode" => {
            let is_enabled = argument(request, 2).as_bool().unwrap_or(false);
            pane_chat
                .set_pane_chat_mode(task_id, text_argument(request, 1), is_enabled)
                .map(|_| Value::Null)
                .map_err(Into::into)
        }
        _ => to_value(service.get_pane_chat_transcript_page(
            task_id,
            text_argument(request, 1),
            text_argument(request, 2),
        )),
    }
}

/// 대상 토큰을 받아 CLI에 입력을 넣거나 입력창 자료를 다루는 호출
fn dispatch_action(
    runtime: &DesktopRuntime,
    request: &PaneChatRequest,
) -> Result<Value, PaneChatCallError> {
    let pane_chat = runtime.pane_chat();
    let service = pane_chat.service();
    let (task_id, token) = (text_argument(request, 0), text_argument(request, 1));
    match request.target {
        "paneChat.sendPaneChatPrompt" => success(pane_chat.send_pane_chat_prompt(
            task_id,
            token,
            argument(request, 2),
            Some(argument(request, 3)),
            request.arguments.get(4).filter(|value| !value.is_null()),
        )),
        "paneChat.interruptPaneChat" => success(service.interrupt_pane_chat(task_id, token)),
        "paneChat.openPaneChatModelPicker" => {
            success(service.open_pane_chat_model_picker(task_id, token))
        }
        "paneChat.revealPaneChatCodexThread" => to_value(
            service
                .reveal_pane_chat_codex_thread(task_id, optional_text_argument(request, 1))
                .map(|outcome| json!({ "outcome": outcome })),
        ),
        "paneChat.answerPaneChatPrompt" => success(service.answer_pane_chat_prompt(
            task_id,
            token,
            text_argument(request, 2),
            argument(request, 3),
        )),
        _ => dispatch_composer(runtime, request),
    }
}

/// 입력창의 첨부·추천과 모델 설정 호출
fn dispatch_composer(
    runtime: &DesktopRuntime,
    request: &PaneChatRequest,
) -> Result<Value, PaneChatCallError> {
    let pane_chat = runtime.pane_chat();
    let service = pane_chat.service();
    let (task_id, token) = (text_argument(request, 0), text_argument(request, 1));
    match request.target {
        "paneChat.stagePaneChatAttachment" => {
            to_value(pane_chat.stage_pane_chat_attachment(task_id, token, argument(request, 2)))
        }
        "paneChat.removePaneChatAttachment" => {
            success(pane_chat.remove_pane_chat_attachment(task_id, token, argument(request, 2)))
        }
        "paneChat.getPaneChatSuggestions" => to_value(pane_chat.get_pane_chat_suggestions(
            task_id,
            token,
            optional_text_argument(request, 2),
            text_argument(request, 3),
            request.arguments.get(4).and_then(Value::as_f64),
        )),
        "paneChat.getPaneChatModelSettings" => {
            to_value(service.get_pane_chat_model_settings(task_id, token))
        }
        "paneChat.changePaneChatModelSetting" => {
            success(service.change_pane_chat_model_setting(task_id, token, argument(request, 2)))
        }
        other => Err(PaneChatCallError::new(format!(
            "unknown pane chat call: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_prefix_and_error_name_are_removed_like_describe_action_error() {
        let error = PaneChatCallError::from_ipc_message(
            "Error invoking remote method 'kanvibe:invoke': PaneChatInputError: CLI 입력란이 비어 있지 않습니다.",
        );
        assert_eq!(error.message, "CLI 입력란이 비어 있지 않습니다.");
        assert!(!error.is_delivery_uncertain);
        let uncertain = PaneChatCallError::from_ipc_message(
            "Error invoking remote method 'kanvibe:invoke': PaneChatDeliveryUncertainError: CLI 전송 결과를 확인할 수 없습니다.",
        );
        assert!(uncertain.is_delivery_uncertain);
        assert_eq!(
            PaneChatCallError::from_ipc_message("CLI target changed").message,
            "CLI target changed"
        );
    }
}
