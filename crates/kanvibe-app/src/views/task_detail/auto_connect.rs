//! 세션 없는 프로젝트 작업을 여는 순간 기본 세션 타입으로 터미널 세션을 붙인다(TS `AutoConnectTerminalSession`).
//! 세션 도구(tmux·zellij·herdr)가 없으면 설치할지 먼저 묻고, 거절하면 오류 없이 연결 버튼만 남긴다.
//! 연결 중에는 `connecting`, 실패하면 `noTerminal`·사유·연결 버튼을 보인다. 같은 작업 연결은 한 번에 하나만 보낸다.
//! QA 모드에서는 TS IPC 이름(`sessionDependency.*`, `kanban.connectTerminalSession`)으로 고장 주입 판정을 먼저 거친다.

use std::sync::Arc;

use gpui_kit::{
    AnyElement, AnyWindowHandle, AppContext, AsyncApp, Context, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, div,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_runtime::DesktopRuntime;
use serde_json::{Value, json};

use super::TaskDetailView;
use super::data::desktop_runtime;
use crate::components::TailwindText;
use crate::qa::faults::FaultOutcome;
use crate::qa::prompts::confirm_action;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::{AppServices, apply_fault};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// TS IPC 이름
const DEPENDENCY_STATUS_SERVICE: &str = "sessionDependency.getSessionDependencyStatus";
const DEPENDENCY_INSTALL_SERVICE: &str = "sessionDependency.installSessionDependency";
const CONNECT_SERVICE: &str = "kanban.connectTerminalSession";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoConnectState {
    Connecting,
    /// 세션 도구 설치 안내를 거절했다. 실패가 아니라 오류 문구 없이 연결 버튼만 둔다
    Declined,
    /// 사유가 없으면 `connectFailed` 문구를 쓴다
    Failed(Option<String>),
}

/// 연결 시도 하나의 결과
enum ConnectOutcome {
    Connected(Box<KanbanTask>),
    Declined,
    Failed(Option<String>),
}

/// TS `SessionDependencyStatus`·`GitHubCliStatus` 가운데 화면이 쓰는 값
pub(super) struct DependencyStatus {
    pub is_available: bool,
    pub tool_name: Option<String>,
    pub is_remote: bool,
    pub ssh_host: Option<String>,
    pub blocked_reason: Option<String>,
}

/// 백그라운드에서 서비스를 부를 때 필요한 것
#[derive(Clone)]
struct ConnectRequest {
    runtime: Option<Arc<DesktopRuntime>>,
    task_id: String,
    session_type: SessionType,
    ssh_host: Option<String>,
    locale: Locale,
    window: Option<AnyWindowHandle>,
}

impl TaskDetailView {
    /// 같은 작업 연결이 진행 중이면 다시 보내지 않는다
    pub(super) fn start_auto_connect(&mut self, cx: &mut Context<Self>) {
        if self._auto_connect.is_some() {
            return;
        }
        let Some(snapshot) = self.snapshot() else {
            return;
        };
        let request = ConnectRequest {
            runtime: desktop_runtime(cx),
            task_id: snapshot.task.id.clone(),
            session_type: snapshot.default_session_type,
            ssh_host: snapshot
                .task
                .ssh_host
                .clone()
                .filter(|host| !host.is_empty()),
            locale: self.screen.route.locale,
            window: self.window_handle,
        };
        self.auto_connect = AutoConnectState::Connecting;
        self._auto_connect = Some(cx.spawn(async move |this, cx| {
            let outcome = connect_task_session(request, cx).await;
            let _ = this.update(cx, |view, cx| view.finish_auto_connect(outcome, cx));
        }));
        cx.notify();
    }

    fn finish_auto_connect(&mut self, outcome: ConnectOutcome, cx: &mut Context<Self>) {
        self._auto_connect = None;
        match outcome {
            ConnectOutcome::Connected(connected) => {
                if let Some(mut snapshot) = self.snapshot().cloned() {
                    snapshot.task = *connected;
                    self.apply_snapshot(snapshot, cx);
                }
            }
            ConnectOutcome::Declined => self.auto_connect = AutoConnectState::Declined,
            ConnectOutcome::Failed(reason) => self.auto_connect = AutoConnectState::Failed(reason),
        }
        cx.notify();
    }
}

/// 도구 확인 → (없으면 설치 확인·설치) → 세션 연결
async fn connect_task_session(request: ConnectRequest, cx: &mut AsyncApp) -> ConnectOutcome {
    match ensure_session_dependency(&request, cx).await {
        Ok(true) => {}
        Ok(false) => return ConnectOutcome::Declined,
        Err(message) => return ConnectOutcome::Failed(Some(message)),
    }
    let arguments = json!([request.task_id, request.session_type.as_str()]);
    let call = request.clone();
    let connected = call_service(cx, CONNECT_SERVICE, arguments, move || {
        let runtime = call.runtime.as_ref().ok_or_else(String::new)?;
        let task = runtime
            .kanban()
            .connect_terminal_session(&call.task_id, call.session_type)
            .map_err(|error| error.message)?;
        Ok(serde_json::to_value(task).unwrap_or(Value::Null))
    })
    .await;
    match connected {
        Ok(value) => match serde_json::from_value::<Option<KanbanTask>>(value) {
            Ok(Some(task)) => ConnectOutcome::Connected(Box::new(task)),
            _ => ConnectOutcome::Failed(None),
        },
        Err(message) => ConnectOutcome::Failed((!message.is_empty()).then_some(message)),
    }
}

/// TS `ensureSessionDependencyWithPrompt`. 준비됐으면 `Ok(true)`, 설치를 거절하면 `Ok(false)`
async fn ensure_session_dependency(
    request: &ConnectRequest,
    cx: &mut AsyncApp,
) -> Result<bool, String> {
    let status = read_dependency(request, DEPENDENCY_STATUS_SERVICE, cx).await?;
    let Some(tool_name) = status.tool_name.clone().filter(|_| !status.is_available) else {
        return Ok(true);
    };
    let common = Translator::new(request.locale, "common");
    let prompt = install_prompt(&common, &status, &tool_name);
    let Some(window) = request.window else {
        return Ok(false);
    };
    let answer = cx
        .update_window(window, |_, window, cx| {
            confirm_action(&prompt, None, window, cx)
        })
        .map_err(|error| error.to_string())?;
    if !answer.await {
        return Ok(false);
    }
    let installed = read_dependency(request, DEPENDENCY_INSTALL_SERVICE, cx).await?;
    if installed.is_available {
        return Ok(true);
    }
    let reason = installed
        .blocked_reason
        .unwrap_or_else(|| "unknown error".to_string());
    Err(common.translate_with(
        "sessionDependency.installFailed",
        &[("tool", &tool_name), ("error", &reason)],
    ))
}

async fn read_dependency(
    request: &ConnectRequest,
    service: &'static str,
    cx: &mut AsyncApp,
) -> Result<DependencyStatus, String> {
    let arguments = json!([request.session_type.as_str(), request.ssh_host]);
    let call = request.clone();
    let value = call_service(cx, service, arguments, move || {
        let runtime = call.runtime.as_ref().ok_or_else(String::new)?;
        let dependencies = runtime.session_dependency();
        let host = call.ssh_host.as_deref();
        let status = match service {
            DEPENDENCY_INSTALL_SERVICE => {
                dependencies.install_session_dependency(call.session_type, host)
            }
            _ => dependencies.get_session_dependency_status(call.session_type, host),
        };
        let status = status.map_err(|error| error.message)?;
        Ok(serde_json::to_value(status).unwrap_or(Value::Null))
    })
    .await?;
    Ok(dependency_status_from_value(&value))
}

/// 고장 주입 판정 뒤 서비스를 백그라운드에서 부른다. `Result` 고장이면 그 값을 응답으로 쓴다
pub(super) async fn call_service(
    cx: &mut AsyncApp,
    service: &'static str,
    arguments: Value,
    call: impl FnOnce() -> Result<Value, String> + Send + 'static,
) -> Result<Value, String> {
    let fault = cx.update(|cx| {
        cx.global::<AppServices>()
            .check_fault(service, &arguments.to_string())
    });
    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        if let FaultOutcome::Result(value) = fault {
            return Ok(value);
        }
        apply_fault(fault, &executor)
            .await
            .map_err(|failure| failure.reason)?;
        call()
    })
    .await
}

pub(super) fn dependency_status_from_value(value: &Value) -> DependencyStatus {
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    DependencyStatus {
        is_available: value.get("available").and_then(Value::as_bool) != Some(false),
        tool_name: text("toolName"),
        is_remote: value.get("isRemote").and_then(Value::as_bool) == Some(true),
        ssh_host: text("sshHost"),
        blocked_reason: text("blockedReason"),
    }
}

pub(super) fn install_prompt(
    common: &Translator,
    status: &DependencyStatus,
    tool_name: &str,
) -> String {
    let target = match status.is_remote {
        true => {
            let host = status.ssh_host.as_deref().unwrap_or("remote");
            common.translate_with("sessionDependency.remoteTarget", &[("host", host)])
        }
        false => common.translate("sessionDependency.localTarget"),
    };
    common.translate_with(
        "sessionDependency.installPrompt",
        &[("tool", tool_name), ("target", &target)],
    )
}

impl TaskDetailView {
    pub(super) fn render_auto_connect(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let content: AnyElement = match &self.auto_connect {
            AutoConnectState::Connecting => {
                let label = self.translator.translate("connecting");
                div()
                    .relative()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.muted)
                    .child(label.clone())
                    .child(text_target("status", label))
                    .into_any_element()
            }
            AutoConnectState::Declined => self.render_connect_retry(theme, None, cx),
            AutoConnectState::Failed(reason) => {
                let message = reason
                    .clone()
                    .unwrap_or_else(|| self.translator.translate("connectFailed"));
                self.render_connect_retry(theme, Some(message), cx)
            }
        };
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::LG)
            .border_1()
            .border_dashed()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .child(content)
            .into_any_element()
    }

    /// `noTerminal`, (실패면) 사유, 연결 버튼
    fn render_connect_retry(
        &self,
        theme: &ThemeTokens,
        failure_message: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = self.translator.translate("connectTerminal");
        let no_terminal = self.translator.translate("noTerminal");
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(spacing(3.0))
            .child(
                div()
                    .relative()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.muted)
                    .child(no_terminal.clone())
                    .child(text_target("paragraph", no_terminal)),
            )
            .children(failure_message.map(|message| {
                div()
                    .relative()
                    .text_scale(text_scale::XS)
                    .text_color(theme.status.error)
                    .child(message.clone())
                    .child(text_target("alert", message))
            }))
            .child(connect_button(theme, label, cx))
            .into_any_element()
    }
}

/// `rounded-md bg-brand-primary px-4 py-1.5 text-sm font-medium text-text-inverse`
fn connect_button(
    theme: &ThemeTokens,
    label: String,
    cx: &mut Context<TaskDetailView>,
) -> impl IntoElement {
    div()
        .id("auto-connect-terminal")
        .relative()
        .rounded(radius::MD)
        .bg(theme.brand.primary)
        .px(spacing(4.0))
        .py(spacing(1.5))
        .text_scale(text_scale::SM)
        .css_weight(500)
        .text_color(theme.text.inverse)
        .cursor_pointer()
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }))
        .on_click(cx.listener(|view, _, _, cx| view.start_auto_connect(cx)))
}

fn text_target(role: &'static str, text: String) -> impl IntoElement {
    qa_target_layer(QaTargetDescription {
        role,
        name: text.clone().into(),
        text: text.into(),
    })
}
