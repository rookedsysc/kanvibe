//! 이미 세션이 있는 모든 작업(Done 포함)의 세션 타입을 한 번에 바꾸는 설정 행(TS `BulkSessionTypeChangeSettings`).
//! 바꿀 대상 확인 → 몇 개를 바꿀지 확인 → 호스트마다 세션 도구 준비 → 켜진 세션을 닫을지 확인 → 변경 순서로 진행하고
//! 단계마다 안내를 보인다. 진행 중에는 다시 적용할 수 없다.
//!
//! 작업 서비스(`KanbanService`)와 세션 도구 서비스는 앱이 오래 들고 있는 서비스라 화면이 만들지 않는다.
//! 화면은 `SessionTypeBulkChanger` 뒤에서만 부르고, 앱이 실제 구현을 `install_session_type_bulk_changer`로 건다.

use std::sync::Arc;

use futures::future::LocalBoxFuture;
use gpui_kit::{
    App, AppContext, AsyncApp, Context, Entity, Global, InteractiveElement, IntoElement,
    ParentElement, Render, Styled, Subscription, Task, Window, div,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::task::SessionType;
use serde::{Deserialize, Serialize};

use crate::components::TailwindText;
use crate::qa::QaRuntime;
use crate::services::ServiceResult;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::controls::{
    SETTING_LABEL_HEIGHT, chromium_center_offset, make_pressable, pressable, setting_label,
};
use crate::views::settings::service_call::{call_service, failure};
use crate::views::settings::session_type_select::{
    SELECT_HEIGHT, SessionTypeSelect, SessionTypeSelected,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTypeChangeTargets {
    pub task_count: usize,
    pub live_session_count: usize,
    /// 새 세션 도구를 확인해야 하는 호스트. `None`은 로컬이다
    pub ssh_hosts: Vec<Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTypeChangeResult {
    pub changed_count: usize,
    pub failed_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDependencyStatus {
    pub available: bool,
    pub tool_name: Option<String>,
    pub is_remote: bool,
    pub ssh_host: Option<String>,
    pub blocked_reason: Option<String>,
}

/// 화면이 작업 서비스에 바라는 것(TS IPC `kanban.*`, `sessionDependency.*`)
pub trait SessionTypeBulkChanger: Send + Sync {
    fn get_session_type_change_targets(
        &self,
        session_type: SessionType,
    ) -> ServiceResult<SessionTypeChangeTargets>;
    fn get_session_dependency_status(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus>;
    fn install_session_dependency(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus>;
    fn change_all_task_session_types(
        &self,
        session_type: SessionType,
        should_close_existing_sessions: bool,
    ) -> ServiceResult<SessionTypeChangeResult>;
}

struct SessionTypeBulkChangerHandle(Arc<dyn SessionTypeBulkChanger>);

impl Global for SessionTypeBulkChangerHandle {}

/// 앱이 작업 서비스를 만든 뒤 부른다
pub fn install_session_type_bulk_changer(changer: Arc<dyn SessionTypeBulkChanger>, cx: &mut App) {
    cx.set_global(SessionTypeBulkChangerHandle(changer));
}

fn session_type_bulk_changer(cx: &mut App) -> Arc<dyn SessionTypeBulkChanger> {
    if !cx.has_global::<SessionTypeBulkChangerHandle>() {
        cx.set_global(SessionTypeBulkChangerHandle(Arc::new(UnwiredBulkChanger)));
    }
    cx.global::<SessionTypeBulkChangerHandle>().0.clone()
}

/// 작업 서비스가 아직 걸리지 않은 앱. 모든 호출이 사유와 함께 실패한다
struct UnwiredBulkChanger;

const KANBAN_SERVICE_UNAVAILABLE: &str = "작업 서비스가 아직 연결되지 않았습니다";

impl SessionTypeBulkChanger for UnwiredBulkChanger {
    fn get_session_type_change_targets(
        &self,
        _: SessionType,
    ) -> ServiceResult<SessionTypeChangeTargets> {
        Err(failure(KANBAN_SERVICE_UNAVAILABLE))
    }

    fn get_session_dependency_status(
        &self,
        _: SessionType,
        _: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus> {
        Err(failure(KANBAN_SERVICE_UNAVAILABLE))
    }

    fn install_session_dependency(
        &self,
        _: SessionType,
        _: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus> {
        Err(failure(KANBAN_SERVICE_UNAVAILABLE))
    }

    fn change_all_task_session_types(
        &self,
        _: SessionType,
        _: bool,
    ) -> ServiceResult<SessionTypeChangeResult> {
        Err(failure(KANBAN_SERVICE_UNAVAILABLE))
    }
}

/// 일괄 변경이 지금 어느 단계에 있는지. 서버가 작업별 진행을 알려 주지 않으므로 확인할 수 있는 단계만 보인다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkChangeStage {
    Checking,
    Preparing { current: usize, total: usize },
    Changing { task_count: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkChangeOutcome {
    pub message: String,
    pub is_failure: bool,
    pub can_retry: bool,
}

/// 확인 대화상자. 사용자가 수락하면 `true`
pub type ConfirmPrompt = Box<dyn Fn(String) -> LocalBoxFuture<'static, bool>>;

/// 단계가 바뀔 때마다 부른다(`None`은 끝)
pub type StageReporter = Box<dyn Fn(Option<BulkChangeStage>, &mut AsyncApp)>;

/// 일괄 변경 한 번. 끝까지 가면 결과 문구를, 사용자가 중간에 그만두면 `None`을 돌려준다
pub async fn run_bulk_change(
    changer: Arc<dyn SessionTypeBulkChanger>,
    session_type: SessionType,
    locale: Locale,
    confirm: ConfirmPrompt,
    report_stage: StageReporter,
    cx: &mut AsyncApp,
) -> Option<BulkChangeOutcome> {
    report_stage(Some(BulkChangeStage::Checking), cx);
    let result =
        change_with_confirmations(&changer, session_type, locale, &confirm, &report_stage, cx)
            .await;
    report_stage(None, cx);
    let settings = Translator::new(locale, "settings");
    match result {
        Ok(outcome) => outcome,
        Err(reason) => Some(BulkChangeOutcome {
            message: reason.unwrap_or_else(|| settings.translate("bulkSessionTypeChangeFailed")),
            is_failure: true,
            can_retry: true,
        }),
    }
}

/// 실패 사유가 비어 있으면 일반 실패 문구를 쓴다(TS `error instanceof Error ? message : 기본 문구`)
type BulkChangeError = Option<String>;

fn as_bulk_error(failure: crate::services::ServiceFailure) -> BulkChangeError {
    Some(failure.reason).filter(|reason| !reason.is_empty())
}

async fn change_with_confirmations(
    changer: &Arc<dyn SessionTypeBulkChanger>,
    session_type: SessionType,
    locale: Locale,
    confirm: &ConfirmPrompt,
    report_stage: &StageReporter,
    cx: &mut AsyncApp,
) -> Result<Option<BulkChangeOutcome>, BulkChangeError> {
    let settings = Translator::new(locale, "settings");
    let targets = request_targets(changer, session_type, cx).await?;
    if targets.task_count == 0 {
        return Ok(Some(BulkChangeOutcome {
            message: settings.translate("bulkSessionTypeNoTargets"),
            is_failure: false,
            can_retry: false,
        }));
    }
    let count = targets.task_count.to_string();
    let values = [
        ("count", count.as_str()),
        ("sessionType", session_type.as_str()),
    ];
    if !confirm(settings.translate_with("bulkSessionTypeChangeConfirm", &values)).await {
        return Ok(None);
    }
    let is_ready = ensure_dependency_on_every_host(
        changer,
        session_type,
        &targets.ssh_hosts,
        locale,
        confirm,
        report_stage,
        cx,
    )
    .await?;
    if !is_ready {
        return Ok(None);
    }
    let should_close = confirm_closing_live_sessions(&settings, &targets, confirm).await;
    report_stage(
        Some(BulkChangeStage::Changing {
            task_count: targets.task_count,
        }),
        cx,
    );
    let result = request_change(changer, session_type, should_close, cx).await?;
    Ok(Some(change_result_outcome(&settings, result)))
}

/// 켜진 세션이 있을 때만 닫을지 묻는다. 취소하면 기존 세션은 백그라운드에 남는다
async fn confirm_closing_live_sessions(
    settings: &Translator,
    targets: &SessionTypeChangeTargets,
    confirm: &ConfirmPrompt,
) -> bool {
    if targets.live_session_count == 0 {
        return false;
    }
    let count = targets.live_session_count.to_string();
    confirm(settings.translate_with("bulkCloseExistingSessionsConfirm", &[("count", &count)])).await
}

/// 바꾼 수·실패 수 안내. 실패가 있으면 오류로 알리고 다시 적용할 수 있게 한다
fn change_result_outcome(
    settings: &Translator,
    result: SessionTypeChangeResult,
) -> BulkChangeOutcome {
    BulkChangeOutcome {
        message: settings.translate_with(
            "bulkSessionTypeChangeResult",
            &[
                ("changedCount", &result.changed_count.to_string()),
                ("failedCount", &result.failed_count.to_string()),
            ],
        ),
        is_failure: result.failed_count > 0,
        can_retry: result.failed_count > 0,
    }
}

async fn request_targets(
    changer: &Arc<dyn SessionTypeBulkChanger>,
    session_type: SessionType,
    cx: &mut AsyncApp,
) -> Result<SessionTypeChangeTargets, BulkChangeError> {
    let changer = changer.clone();
    let request = cx.update(|cx| {
        call_service(
            cx,
            "kanban.getSessionTypeChangeTargets",
            format!("[\"{}\"]", session_type.as_str()),
            move || changer.get_session_type_change_targets(session_type),
        )
    });
    request.await.map_err(as_bulk_error)
}

async fn request_change(
    changer: &Arc<dyn SessionTypeBulkChanger>,
    session_type: SessionType,
    should_close: bool,
    cx: &mut AsyncApp,
) -> Result<SessionTypeChangeResult, BulkChangeError> {
    let changer = changer.clone();
    let request = cx.update(|cx| {
        call_service(
            cx,
            "kanban.changeAllTaskSessionTypes",
            format!("[\"{}\",{should_close}]", session_type.as_str()),
            move || changer.change_all_task_session_types(session_type, should_close),
        )
    });
    request.await.map_err(as_bulk_error)
}

/// 호스트마다 세션 도구를 확인하고, 없으면 설치를 묻는다(TS `ensureSessionDependencyWithPrompt`)
async fn ensure_dependency_on_every_host(
    changer: &Arc<dyn SessionTypeBulkChanger>,
    session_type: SessionType,
    ssh_hosts: &[Option<String>],
    locale: Locale,
    confirm: &ConfirmPrompt,
    report_stage: &StageReporter,
    cx: &mut AsyncApp,
) -> Result<bool, BulkChangeError> {
    for (index, ssh_host) in ssh_hosts.iter().enumerate() {
        let stage = BulkChangeStage::Preparing {
            current: index + 1,
            total: ssh_hosts.len(),
        };
        report_stage(Some(stage), cx);
        if !ensure_dependency(changer, session_type, ssh_host.clone(), locale, confirm, cx).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn ensure_dependency(
    changer: &Arc<dyn SessionTypeBulkChanger>,
    session_type: SessionType,
    ssh_host: Option<String>,
    locale: Locale,
    confirm: &ConfirmPrompt,
    cx: &mut AsyncApp,
) -> Result<bool, BulkChangeError> {
    let lookup = changer.clone();
    let host = ssh_host.clone();
    let status = request_dependency(
        "sessionDependency.getSessionDependencyStatus",
        session_type,
        &ssh_host,
        move || lookup.get_session_dependency_status(session_type, host.as_deref()),
        cx,
    )
    .await?;
    let Some(tool_name) = status.tool_name.clone().filter(|_| !status.available) else {
        return Ok(true);
    };
    let common = Translator::new(locale, "common");
    if !confirm(install_prompt(&common, &status, &tool_name)).await {
        return Ok(false);
    }
    let installer = changer.clone();
    let host = ssh_host.clone();
    let installed = request_dependency(
        "sessionDependency.installSessionDependency",
        session_type,
        &ssh_host,
        move || installer.install_session_dependency(session_type, host.as_deref()),
        cx,
    )
    .await?;
    if installed.available {
        return Ok(true);
    }
    let reason = installed
        .blocked_reason
        .unwrap_or_else(|| "unknown error".to_string());
    Err(Some(common.translate_with(
        "sessionDependency.installFailed",
        &[("tool", &tool_name), ("error", &reason)],
    )))
}

async fn request_dependency(
    target: &'static str,
    session_type: SessionType,
    ssh_host: &Option<String>,
    work: impl FnOnce() -> ServiceResult<SessionDependencyStatus> + Send + 'static,
    cx: &mut AsyncApp,
) -> Result<SessionDependencyStatus, BulkChangeError> {
    let arguments = serde_json::json!([session_type.as_str(), ssh_host]).to_string();
    cx.update(|cx| call_service(cx, target, arguments, work))
        .await
        .map_err(as_bulk_error)
}

/// "{대상}에 {도구}이(가) 설치되어 있지 않습니다. 지금 설치하시겠습니까?"
fn install_prompt(
    common: &Translator,
    status: &SessionDependencyStatus,
    tool_name: &str,
) -> String {
    let target = if status.is_remote {
        let host = status
            .ssh_host
            .clone()
            .unwrap_or_else(|| "remote".to_string());
        common.translate_with("sessionDependency.remoteTarget", &[("host", &host)])
    } else {
        common.translate("sessionDependency.localTarget")
    };
    common.translate_with(
        "sessionDependency.installPrompt",
        &[("tool", tool_name), ("target", &target)],
    )
}

/// 네이티브 확인 대화상자(`window.confirm`). QA 재생은 TS 드라이버처럼 모두 수락한다
pub fn window_confirm_prompt(window: &mut Window, cx: &mut App) -> ConfirmPrompt {
    let is_qa = cx.has_global::<QaRuntime>();
    let window_handle = window.window_handle();
    let async_cx = cx.to_async();
    Box::new(move |message: String| {
        let mut async_cx = async_cx.clone();
        Box::pin(async move {
            if is_qa {
                return true;
            }
            let answer = window_handle.update(&mut async_cx, |_, window, cx| {
                window.prompt(
                    gpui_kit::PromptLevel::Info,
                    &message,
                    None,
                    &["확인", "취소"],
                    cx,
                )
            });
            match answer {
                Ok(receiver) => receiver.await == Ok(0),
                Err(_) => false,
            }
        })
    })
}

pub struct BulkSessionTypeChangeSettings {
    locale: Locale,
    changer: Arc<dyn SessionTypeBulkChanger>,
    session_type_select: Entity<SessionTypeSelect>,
    pub stage: Option<BulkChangeStage>,
    pub outcome: Option<BulkChangeOutcome>,
    _run: Option<Task<()>>,
    _subscription: Subscription,
}

impl BulkSessionTypeChangeSettings {
    pub fn new(locale: Locale, default_session_type: SessionType, cx: &mut Context<Self>) -> Self {
        let session_type_select =
            cx.new(|cx| SessionTypeSelect::new("bulk-session-type", default_session_type, cx));
        let subscription = cx
            .subscribe(&session_type_select, |_, _, _: &SessionTypeSelected, cx| {
                cx.notify()
            });
        Self {
            locale,
            changer: session_type_bulk_changer(cx),
            session_type_select,
            stage: None,
            outcome: None,
            _run: None,
            _subscription: subscription,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.stage.is_some()
    }

    /// TS는 선택한 기본 세션 타입이 바뀌어도 일괄 변경 선택은 처음 값 그대로 둔다
    pub fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_pending() {
            return;
        }
        self.outcome = None;
        let session_type = self.session_type_select.read(cx).selected();
        let confirm = window_confirm_prompt(window, cx);
        let view = cx.entity().downgrade();
        let select = self.session_type_select.clone();
        let report_stage: StageReporter = Box::new(move |stage, cx| {
            select.update(cx, |select, cx| select.set_disabled(stage.is_some(), cx));
            let _ = view.update(cx, |this, cx| {
                this.stage = stage;
                cx.notify();
            });
        });
        let (changer, locale) = (self.changer.clone(), self.locale);
        self._run = Some(cx.spawn(async move |this, cx| {
            let outcome =
                run_bulk_change(changer, session_type, locale, confirm, report_stage, cx).await;
            let _ = this.update(cx, |this, cx| {
                this.outcome = outcome;
                cx.notify();
            });
        }));
    }
}

impl Render for BulkSessionTypeChangeSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let settings = Translator::new(self.locale, "settings");
        let apply = self.render_apply_button(&theme, &settings, cx);
        div()
            .mt(spacing(4.0))
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(spacing(3.0))
                    .child(setting_label(
                        &theme,
                        settings.translate("bulkSessionTypeChange"),
                        settings.translate("bulkSessionTypeChangeDescription"),
                    ))
                    .child(
                        div()
                            .mt(chromium_center_offset(SETTING_LABEL_HEIGHT, SELECT_HEIGHT))
                            .flex()
                            .items_center()
                            .gap(spacing(2.0))
                            .child(self.session_type_select.clone())
                            .child(apply),
                    ),
            )
            .children(self.stage.map(|stage| {
                div()
                    .mt(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(describe_stage(&settings, stage))
            }))
            .children(
                self.outcome
                    .clone()
                    .map(|outcome| self.render_outcome(&theme, &settings, outcome, cx)),
            )
    }
}

impl BulkSessionTypeChangeSettings {
    /// `px-3 py-1 text-sm bg-brand-primary text-text-inverse rounded-md font-medium`
    fn render_apply_button(
        &self,
        theme: &ThemeTokens,
        settings: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let label = settings.translate("bulkSessionTypeApply");
        let hover = theme.brand.hover;
        let apply = pressable(
            "bulk-session-type-apply",
            "button",
            label.clone(),
            label.clone(),
        )
        .px(spacing(3.0))
        .py(spacing(1.0))
        .text_scale(text_scale::SM)
        .css_weight(500)
        .rounded(radius::MD)
        .bg(theme.brand.primary)
        .text_color(theme.text.inverse)
        .hover(move |style| style.bg(hover))
        .child(label);
        make_pressable(
            apply,
            theme,
            self.is_pending(),
            cx.listener(|this, _, window, cx| this.apply(window, cx)),
        )
    }

    fn render_outcome(
        &self,
        theme: &ThemeTokens,
        settings: &Translator,
        outcome: BulkChangeOutcome,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let color = if outcome.is_failure {
            theme.status.error
        } else {
            theme.text.secondary
        };
        let retry = outcome.can_retry.then(|| {
            let label = settings.translate("bulkSessionTypeRetryFailed");
            let button = pressable(
                "bulk-session-type-retry",
                "button",
                label.clone(),
                label.clone(),
            )
            .rounded(radius::SM)
            .px(spacing(1.5))
            .py(spacing(0.5))
            .css_weight(500)
            .hover(|style| style.underline())
            .child(label);
            make_pressable(
                button,
                theme,
                false,
                cx.listener(|this, _, window, cx| this.apply(window, cx)),
            )
        });
        div()
            .mt(spacing(2.0))
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(color)
            .child(outcome.message)
            .children(retry)
    }
}

pub fn describe_stage(settings: &Translator, stage: BulkChangeStage) -> String {
    match stage {
        BulkChangeStage::Checking => settings.translate("bulkSessionTypeChecking"),
        BulkChangeStage::Preparing { current, total } => settings.translate_with(
            "bulkSessionTypePreparingHost",
            &[
                ("current", &current.to_string()),
                ("total", &total.to_string()),
            ],
        ),
        BulkChangeStage::Changing { task_count } => settings.translate_with(
            "bulkSessionTypeChanging",
            &[("count", &task_count.to_string())],
        ),
    }
}
