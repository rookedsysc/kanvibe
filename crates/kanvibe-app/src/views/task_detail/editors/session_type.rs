//! 세션 타입 편집(TS `SessionTypeEditor`). 이미 세션이 있는 작업의 세션 종류를 바꾼다.
//! 새 세션 도구가 없으면 설치를 묻고(TS `ensureSessionDependencyWithPrompt`), 기존 세션이 켜져 있으면 닫을지 묻는다.
//! 바꾸는 동안에는 select를 막고 진행 문구를 보이며, 실패는 그대로 보이고 파괴적인 변경을 자동으로 되풀이하지 않는다.
//! select는 TS의 네이티브 `<select>`처럼 누르면 아래로 선택지 목록을 펼친다.

use gpui_kit::{
    AnyElement, AsyncWindowContext, Context, Div, InteractiveElement, IntoElement, ParentElement,
    SharedString, Stateful, StatefulInteractiveElement, Styled, WeakEntity, Window, anchored,
    deferred, div, point, px,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::task::{KanbanTask, SessionType};
use serde::de::DeserializeOwned;

use crate::components::TailwindText;
use crate::components::session_dependency_prompt::ensure_session_dependency;
use crate::qa::prompts::confirm_action;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::{ServiceFailure, ServiceResult};
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::board::services::board_services;
use crate::views::settings::service_call::{call_service, failure};
use crate::views::task_detail::TaskDetailView;
use crate::views::task_detail::data::desktop_runtime;
use crate::views::task_detail::status_move::KANBAN_SERVICE_UNAVAILABLE;
use crate::views::task_detail::usage_cards::visible_text;

/// select 상자 폭(가장 긴 선택지 `terminal`과 화살표가 들어가는 네이티브 select 폭)
const SELECT_WIDTH: f32 = 84.0;
/// 펼친 목록 한 줄 높이(Chromium 네이티브 목록)
const MENU_ROW_HEIGHT: f32 = 25.0;
/// Chromium 네이티브 목록의 고른 줄 배경과 테두리
const MENU_SELECTED_BACKGROUND: u32 = 0x99c8ff;
const MENU_BORDER: u32 = 0x767676;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionTypeChange {
    pub is_pending: bool,
    pub error: Option<String>,
    pub is_menu_open: bool,
}

/// 바꾸기 흐름이 멈춘 사유. 서비스 실패는 IPC 머리말을 붙이고, 화면이 만든 사유는 그대로 보인다
enum ChangeFailure {
    Service(ServiceFailure),
    Message(String),
}

impl ChangeFailure {
    fn text(self) -> String {
        match self {
            Self::Service(failure) => failure.display_message(),
            Self::Message(message) => message,
        }
    }
}

impl From<ServiceFailure> for ChangeFailure {
    fn from(failure: ServiceFailure) -> Self {
        Self::Service(failure)
    }
}

/// 바꾸기에 필요한 값
#[derive(Clone)]
struct ChangeRequest {
    task_id: String,
    current: SessionType,
    next: SessionType,
    ssh_host: Option<String>,
    task_detail: Translator,
    locale: Locale,
}

impl TaskDetailView {
    pub(in crate::views::task_detail) fn render_session_type_editor(
        &self,
        theme: &ThemeTokens,
        current: SessionType,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let change = &self.editors.session_type;
        let status = change.is_pending.then(|| {
            visible_text(
                self.translator.translate("sessionTypeChanging"),
                text_scale::XS,
            )
            .text_color(theme.text.muted)
        });
        let error = change.error.as_ref().map(|error| {
            visible_text(error.clone(), text_scale::XS).text_color(theme.status.error)
        });
        div()
            .flex()
            .flex_col()
            .items_end()
            .gap(spacing(1.0))
            .child(self.render_session_select(theme, current, cx))
            .children(status)
            .children(error)
            .into_any_element()
    }

    /// `text-xs bg-tag-session-bg text-tag-session-text px-2 py-0.5 rounded-full`과 네이티브 화살표
    fn render_session_select(
        &self,
        theme: &ThemeTokens,
        current: SessionType,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let change = &self.editors.session_type;
        let mut select = select_box(theme, current, change.is_menu_open);
        select = match change.is_pending {
            true => select.opacity(0.5),
            false => select
                .cursor_pointer()
                .on_click(cx.listener(|view, _, _, cx| view.toggle_session_type_menu(cx))),
        };
        // 목록은 select 바로 아래 창 위층에 띄워 행 높이를 바꾸지 않는다
        let menu = change.is_menu_open.then(|| {
            div()
                .absolute()
                .top_full()
                .left_0()
                .child(self.render_session_type_menu(current, cx))
        });
        div()
            .relative()
            .child(select)
            .children(menu)
            .into_any_element()
    }

    fn render_session_type_menu(&self, current: SessionType, cx: &mut Context<Self>) -> AnyElement {
        let rows = SessionType::ALL.map(|session_type| {
            let row = div()
                .id(SharedString::from(format!(
                    "session-type-option-{}",
                    session_type.as_str()
                )))
                .relative()
                .h(px(MENU_ROW_HEIGHT))
                .flex()
                .items_center()
                .px(spacing(3.0))
                .text_size(px(13.0))
                .text_color(gpui_kit::rgb(0x000000))
                .cursor_pointer()
                .child(session_type.as_str())
                .child(qa_target_layer(QaTargetDescription {
                    role: "option",
                    name: session_type.as_str().into(),
                    text: session_type.as_str().into(),
                }))
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.choose_session_type(session_type, window, cx)
                }));
            match session_type == current {
                true => row.bg(gpui_kit::rgb(MENU_SELECTED_BACKGROUND)),
                false => row,
            }
        });
        deferred(
            anchored().offset(point(px(0.0), px(1.0))).child(
                div()
                    .id("session-type-menu")
                    .occlude()
                    .w(px(SELECT_WIDTH))
                    .border_1()
                    .border_color(gpui_kit::rgb(MENU_BORDER))
                    .bg(gpui_kit::rgb(0xffffff))
                    .children(rows),
            ),
        )
        .with_priority(1)
        .into_any_element()
    }

    fn toggle_session_type_menu(&mut self, cx: &mut Context<Self>) {
        let change = &mut self.editors.session_type;
        change.is_menu_open = !change.is_menu_open;
        cx.notify();
    }

    /// 지금과 같은 종류를 고르거나 바꾸는 중이면 아무것도 하지 않는다
    fn choose_session_type(
        &mut self,
        next: SessionType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editors.session_type.is_menu_open = false;
        cx.notify();
        let Some(task) = self.snapshot().map(|snapshot| snapshot.task.clone()) else {
            return;
        };
        let Some(current) = task.session_type else {
            return;
        };
        if self.editors.session_type.is_pending || next == current {
            return;
        }
        self.editors.session_type.is_pending = true;
        self.editors.session_type.error = None;
        let request = ChangeRequest {
            task_id: task.id.clone(),
            current,
            next,
            ssh_host: task.ssh_host.clone(),
            task_detail: self.translator,
            locale: self.screen.route.locale,
        };
        self.editors._session_type_change = Some(cx.spawn_in(window, async move |this, cx| {
            let result = change_session_type(&this, &request, cx).await;
            let _ = this.update(cx, |view, cx| view.finish_session_type_change(result, cx));
        }));
    }

    fn finish_session_type_change(
        &mut self,
        result: Result<Option<Option<KanbanTask>>, ChangeFailure>,
        cx: &mut Context<Self>,
    ) {
        let change = &mut self.editors.session_type;
        change.is_pending = false;
        match result {
            // 설치를 거절했다
            Ok(None) => {}
            Ok(Some(None)) => {
                change.error = Some(self.translator.translate("sessionTypeChangeFailed"))
            }
            Ok(Some(Some(changed))) => {
                if let Some(mut snapshot) = self.snapshot().cloned() {
                    snapshot.task = changed;
                    self.apply_snapshot(snapshot, cx);
                }
                crate::app_events::trigger_refresh(crate::app_events::RefreshScope::TaskDetail, cx);
            }
            Err(failure) => change.error = Some(failure.text()),
        }
        cx.notify();
    }
}

/// select 상자. 목록을 펼친 동안은 `focus:ring-1 ring-brand-primary` 테두리를 보인다
fn select_box(theme: &ThemeTokens, current: SessionType, is_menu_open: bool) -> Stateful<Div> {
    let colors = theme.tag.session;
    let border = match is_menu_open {
        true => theme.brand.primary,
        false => colors.background,
    };
    div()
        .id("session-type-select")
        .relative()
        .w(px(SELECT_WIDTH))
        .flex()
        .items_center()
        .justify_between()
        .rounded(radius::FULL)
        .border_1()
        .border_color(border)
        .bg(colors.background)
        .pl(spacing(3.0) - px(1.0))
        .pr(spacing(2.0) - px(1.0))
        .py(spacing(0.5))
        .text_scale(text_scale::XS)
        .text_color(colors.text)
        .child(aligned_text(
            current.as_str(),
            text_scale::XS,
            TextFamily::Sans,
        ))
        .child(select_chevron(colors.text))
        .child(qa_target_layer(QaTargetDescription {
            role: "combobox",
            name: current.as_str().into(),
            text: current.as_str().into(),
        }))
}

/// 네이티브 select 화살표(굵은 아래 꺾쇠)
fn select_chevron(color: gpui_kit::Rgba) -> AnyElement {
    super::super::icons::detail_icon(
        super::super::icons::DetailIcon::SelectChevron,
        color,
        px(12.0),
    )
    .into_any_element()
}

/// 바꾸기 흐름 전체. `Ok(None)`은 설치를 거절해 바꾸지 않았다는 뜻이다
async fn change_session_type(
    view: &WeakEntity<TaskDetailView>,
    request: &ChangeRequest,
    cx: &mut AsyncWindowContext,
) -> Result<Option<Option<KanbanTask>>, ChangeFailure> {
    let Some(services) = cx.update(|_, cx| board_services(cx)).ok().flatten() else {
        return Err(ChangeFailure::Message(
            KANBAN_SERVICE_UNAVAILABLE.to_string(),
        ));
    };
    let is_ready = ensure_session_dependency(
        services,
        request.next,
        request.ssh_host.clone(),
        request.locale,
        cx,
    )
    .await
    .map_err(ChangeFailure::Message)?;
    if !is_ready {
        return Ok(None);
    }
    let task_id = request.task_id.clone();
    let is_alive: bool = run_service(
        cx,
        "kanban.isTaskSessionAlive",
        serde_json::json!([task_id]),
        move |runtime| {
            runtime
                .kanban()
                .is_task_session_alive(&task_id)
                .map_err(failure)
        },
    )
    .await?;
    let should_close = match is_alive {
        true => {
            let message = request.task_detail.translate_with(
                "closeExistingSessionConfirm",
                &[("sessionType", request.current.as_str())],
            );
            confirm(view, &message, cx).await
        }
        false => false,
    };
    let (task_id, next) = (request.task_id.clone(), request.next);
    let changed: Option<KanbanTask> = run_service(
        cx,
        "kanban.changeTaskSessionType",
        serde_json::json!([task_id, next, should_close]),
        move |runtime| {
            runtime
                .kanban()
                .change_task_session_type(&task_id, next, should_close)
                .map_err(failure)
        },
    )
    .await?;
    Ok(Some(changed))
}

async fn run_service<T, F>(
    cx: &mut AsyncWindowContext,
    target: &'static str,
    arguments: serde_json::Value,
    work: F,
) -> Result<T, ChangeFailure>
where
    T: DeserializeOwned + Send + 'static,
    F: FnOnce(&kanvibe_runtime::DesktopRuntime) -> ServiceResult<T> + Send + 'static,
{
    let request = cx.update(|_, cx| {
        let runtime = desktop_runtime(cx);
        call_service(cx, target, arguments.to_string(), move || match runtime {
            Some(runtime) => work(&runtime),
            None => Err(failure(KANBAN_SERVICE_UNAVAILABLE)),
        })
    });
    match request {
        Ok(request) => Ok(request.await?),
        Err(error) => Err(ChangeFailure::Message(error.to_string())),
    }
}

/// 네이티브 확인(QA 재생은 받아들인다). 화면이 사라졌으면 받아들이지 않은 것으로 본다
async fn confirm(
    view: &WeakEntity<TaskDetailView>,
    message: &str,
    cx: &mut AsyncWindowContext,
) -> bool {
    let answer = view.update_in(cx, |_, window, cx| {
        confirm_action(message, None, window, cx)
    });
    match answer {
        Ok(answer) => answer.await,
        Err(_) => false,
    }
}
