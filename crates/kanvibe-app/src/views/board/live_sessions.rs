//! 카드의 실행중 AI 세션(TS `TaskCardLiveSessions` + 보드 팝오버의 `LiveAiSessionPanel`).
//! 배지는 보드 전체가 5초마다 한 번 읽는 tmux pane 목록만 보고 그린다. 세션과 서브태스크를 담은 패널은 카드에
//! 포커스·마우스가 잠시 머무른 뒤에만 열고, 열려 있는 동안 그 작업의 세션을 2초마다 다시 읽는다.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::{
    AnyElement, Context, FocusHandle, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, deferred, div, img, px,
    relative,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::running_agent_panes::{count_by_provider, filter_panes_by_worktree};
use kanvibe_ai::types::{LiveAiSession, LiveAiSessionState, RunningAgentPane};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::KanbanTask;

use super::BoardView;
use crate::assets::provider_icon_paths;
use crate::components::{TailwindText, box_shadows};
use crate::polled_value::{PollingSubscription, start_polling};
use crate::qa::options::parse_iso_utc_ms;
use crate::qa::targets::{QaTargetBehavior, QaTargetDescription, qa_target_layer_with};
use crate::services::AppServices;
use crate::theme::{ThemeTokens, radius, spacing};
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};

/// 카드 사이를 훑고 지나갈 때마다 패널이 번쩍이지 않도록 잠깐 머무른 뒤에 연다
const LIVE_SESSION_PANEL_OPEN_DELAY: Duration = Duration::from_millis(1_200);
/// 세션 패널은 서브태스크가 뜨고 지는 것을 눈으로 따라갈 수 있어야 해서 짧게 돈다
const LIVE_SESSION_POLL_INTERVAL: Duration = Duration::from_millis(2_000);
const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;

/// 열린 세션 패널 하나. 닫으면 폴링도 함께 멈춘다
struct OpenLivePanel {
    sessions: Option<PollingSubscription<Vec<LiveAiSession>>>,
}

#[derive(Default)]
pub(super) struct LiveSessionState {
    pub running_panes: Vec<RunningAgentPane>,
    open_timers: HashMap<String, Task<()>>,
    open_panels: HashMap<String, OpenLivePanel>,
    focus_watches: HashMap<String, [Subscription; 2]>,
}

/// TS `AiProviderIcon`: 그림이 있는 provider는 색 아이콘, 나머지는 이름 앞 두 글자
pub(super) fn provider_icon(provider: AiSessionProvider, size: f32) -> AnyElement {
    let path = match provider {
        AiSessionProvider::Claude => provider_icon_paths::CLAUDE,
        AiSessionProvider::Codex => provider_icon_paths::CODEX,
        AiSessionProvider::Gemini => provider_icon_paths::GEMINI,
        AiSessionProvider::Opencode => provider_icon_paths::OPENCODE,
        _ => {
            let initials: String = provider.label().chars().take(2).collect();
            return div()
                .size(px(size))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.0))
                .css_weight(700)
                .child(initials.to_uppercase())
                .into_any_element();
        }
    };
    img(path).size(px(size)).flex_none().into_any_element()
}

/// 시험·재생 드라이버가 TS `data-testid`로 찾는 대상
fn test_id_layer(test_id: String, text: String) -> impl IntoElement {
    qa_target_layer_with(
        QaTargetDescription {
            role: "status",
            name: test_id.clone().into(),
            text: text.into(),
        },
        QaTargetBehavior {
            selector: Some(format!("[data-testid='{test_id}']").into()),
            ..QaTargetBehavior::default()
        },
    )
}

impl BoardView {
    /// 카드 포커스가 들어오고 나갈 때 패널 열기·닫기를 건다(카드마다 한 번)
    pub(super) fn watch_card_focus(
        &mut self,
        task_id: &str,
        focus_handle: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.live_sessions.focus_watches.contains_key(task_id) {
            return;
        }
        let focused_task = task_id.to_string();
        let blurred_task = task_id.to_string();
        let watches = [
            cx.on_focus(focus_handle, window, move |this, window, cx| {
                this.activate_live_panel(&focused_task, window, cx)
            }),
            cx.on_blur(focus_handle, window, move |this, _, cx| {
                this.deactivate_live_panel(&blurred_task, cx)
            }),
        ];
        self.live_sessions
            .focus_watches
            .insert(task_id.to_string(), watches);
    }

    /// 카드에 포커스·마우스가 들어왔다. 머무르면 패널을 연다
    pub(super) fn activate_live_panel(
        &mut self,
        task_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.live_sessions;
        if state.open_timers.contains_key(task_id) || state.open_panels.contains_key(task_id) {
            return;
        }
        let opening_task = task_id.to_string();
        let timer = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(LIVE_SESSION_PANEL_OPEN_DELAY)
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.open_live_panel(opening_task, window, cx)
            });
        });
        state.open_timers.insert(task_id.to_string(), timer);
    }

    /// 카드에서 포커스·마우스가 나갔다. 기다리던 열기를 취소하고 열린 패널을 닫는다
    pub(super) fn deactivate_live_panel(&mut self, task_id: &str, cx: &mut Context<Self>) {
        self.live_sessions.open_timers.remove(task_id);
        if self.live_sessions.open_panels.remove(task_id).is_some() {
            cx.notify();
        }
    }

    pub(super) fn is_live_panel_open(&self, task_id: &str) -> bool {
        self.live_sessions.open_panels.contains_key(task_id)
    }

    fn open_live_panel(&mut self, task_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.live_sessions.open_timers.remove(&task_id);
        let sessions = self.services.clone().map(|services| {
            let polled_task = task_id.clone();
            start_polling(
                LIVE_SESSION_POLL_INTERVAL,
                move |cx| services.load_task_live_sessions(polled_task.clone(), cx),
                |_: &mut Self, _, cx| cx.notify(),
                window,
                cx,
            )
        });
        let panel = OpenLivePanel { sessions };
        self.live_sessions.open_panels.insert(task_id, panel);
        cx.notify();
    }

    /// 같은 worktree에서 도는 에이전트를 provider별 개수로 보인다. 없으면 그리지 않는다
    pub(super) fn render_running_agents(
        &self,
        theme: &ThemeTokens,
        task: &KanbanTask,
    ) -> Option<AnyElement> {
        let task_panes = filter_panes_by_worktree(
            &self.live_sessions.running_panes,
            task.worktree_path.as_deref(),
        );
        if task_panes.is_empty() {
            return None;
        }
        let counts = count_by_provider(task_panes.iter().map(|pane| pane.provider));
        let providers = counts.into_iter().map(|(provider, count)| {
            div()
                .relative()
                .flex()
                .items_center()
                .gap(px(2.0))
                .child(provider_icon(provider, 12.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .line_height(px(10.0))
                        .css_weight(600)
                        .text_color(theme.text.secondary)
                        .child(count.to_string()),
                )
                .child(test_id_layer(
                    format!("task-card-running-{}", provider.id()),
                    count.to_string(),
                ))
        });
        Some(
            div()
                .relative()
                .flex()
                .items_center()
                .gap(px(2.0))
                .children(providers)
                .child(test_id_layer(
                    "task-card-running-agents".to_string(),
                    String::new(),
                ))
                .into_any_element(),
        )
    }

    /// 열린 패널을 카드 아래로 펼친다. 실행중 에이전트도 세션도 없으면 그리지 않는다
    /// (빈 팝오버가 다음 카드를 덮고 그 위 클릭을 삼키지 않도록)
    pub(super) fn render_live_session_popover(
        &self,
        theme: &ThemeTokens,
        task: &KanbanTask,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let panel = self.live_sessions.open_panels.get(&task.id)?;
        let sessions = panel
            .sessions
            .as_ref()
            .and_then(|sessions| sessions.value(cx).cloned())
            .unwrap_or_default();
        let has_panes = !filter_panes_by_worktree(
            &self.live_sessions.running_panes,
            task.worktree_path.as_deref(),
        )
        .is_empty();
        if !has_panes && sessions.is_empty() {
            return None;
        }
        let content = self.render_live_session_panel(theme, &task.id, &sessions, cx);
        let popover = div()
            .id("task-card-live-session-popover")
            .occlude()
            .absolute()
            .left(spacing(2.0))
            .right(spacing(2.0))
            .top(relative(1.0))
            .mt(spacing(1.0))
            .overflow_hidden()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.lg))
            // 패널 안 클릭이 카드의 상세 이동으로 번지지 않게 한다
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(content)
            .child(test_id_layer(
                "task-card-live-session-popover".to_string(),
                String::new(),
            ));
        Some(deferred(popover).with_priority(1).into_any_element())
    }

    fn render_live_session_panel(
        &self,
        theme: &ThemeTokens,
        task_id: &str,
        sessions: &[LiveAiSession],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let labels = Translator::new(self.locale(), "taskDetail.liveSessions");
        if sessions.is_empty() {
            return div()
                .px(spacing(2.0))
                .py(spacing(3.0))
                .text_scale(crate::theme::text_scale::XS)
                .text_color(theme.text.muted)
                .child(labels.translate("empty"))
                .into_any_element();
        }
        let now_ms = cx.global::<AppServices>().clock.now_ms();
        let rows = sessions.iter().enumerate().map(|(index, session)| {
            let row = self.render_live_session_row(theme, &labels, task_id, session, now_ms, cx);
            div()
                .when_not_last(index + 1 < sessions.len(), theme)
                .child(row)
        });
        div()
            .child(render_tally(theme, &labels, sessions))
            .children(rows)
            .into_any_element()
    }

    fn render_live_session_row(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        task_id: &str,
        session: &LiveAiSession,
        now_ms: i64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = session
            .current_task
            .clone()
            .unwrap_or_else(|| session.provider.id().to_string());
        let summary = live_session_summary(theme, labels, session, &title, now_ms);
        let row = match session.terminal_window.clone() {
            Some(terminal_window) => self.selectable_session_row(
                theme,
                task_id,
                session,
                terminal_window,
                &title,
                summary,
                cx,
            ),
            None => div()
                .flex()
                .min_w_0()
                .flex_1()
                .items_center()
                .px(spacing(1.0))
                .py(px(2.0))
                .child(summary)
                .into_any_element(),
        };
        div()
            .px(spacing(2.0))
            .py(px(6.0))
            .child(row)
            .children(render_subtasks(theme, labels, session, now_ms))
            .into_any_element()
    }

    /// 터미널 창을 찾은 세션 행. 누르면 그 창으로 옮긴 뒤 상세로 간다
    #[allow(clippy::too_many_arguments)]
    fn selectable_session_row(
        &self,
        theme: &ThemeTokens,
        task_id: &str,
        session: &LiveAiSession,
        terminal_window: kanvibe_ai::types::LiveSessionTerminalWindow,
        title: &str,
        summary: gpui_kit::Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let task_id = task_id.to_string();
        let hover = theme.background.page;
        div()
            .id(gpui_kit::SharedString::from(format!(
                "live-session-{}-{}",
                session.provider.id(),
                terminal_window.window_id
            )))
            .relative()
            .flex()
            .min_w_0()
            .flex_1()
            .items_center()
            .rounded(radius::BASE)
            .px(spacing(1.0))
            .py(px(2.0))
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.focus_live_session(&task_id, &terminal_window.window_id, window, cx)
            }))
            .child(summary)
            .child(crate::qa::targets::qa_button_layer(title, title))
            .into_any_element()
    }

    /// 세션이 붙은 tmux window로 옮긴 뒤 작업 상세로 간다
    fn focus_live_session(
        &mut self,
        task_id: &str,
        window_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let locale = self.locale();
        let task_id = task_id.to_string();
        let origin_window = Some(window.window_handle().window_id());
        let select = self.services.clone().map(|services| {
            services.select_terminal_tab(task_id.clone(), window_id.to_string(), cx)
        });
        cx.spawn(async move |_, cx| {
            // 창을 옮기지 못했으면 상세로 가지 않는다(TS `await selectTerminalTab` 실패와 같다)
            if let Some(select) = select
                && select.await.is_err()
            {
                return;
            }
            let request = TaskNavigationRequest {
                origin_window,
                should_open_new_window: false,
            };
            let _ = cx.update(|cx| navigate_to_task_detail(&task_id, locale, request, cx));
        })
        .detach();
    }
}

/// 세션 행의 한 줄: 상태 점·provider·지금 하는 일·서브태스크 수·지난 시간
fn live_session_summary(
    theme: &ThemeTokens,
    labels: &Translator,
    session: &LiveAiSession,
    title: &str,
    now_ms: i64,
) -> gpui_kit::Div {
    let is_running = session.state == LiveAiSessionState::Running;
    let subtask_count = (!session.running_subtasks.is_empty()).then(|| {
        let count = session.running_subtasks.len().to_string();
        small_label(
            theme,
            labels.translate_with("subtaskCount", &[("count", &count)]),
        )
    });
    let elapsed = elapsed_label(labels, session.last_active_at.as_deref(), now_ms)
        .map(|label| small_label(theme, label));
    let dot_color = if is_running {
        theme.status.success
    } else {
        theme.status.todo
    };
    div()
        .flex()
        .min_w_0()
        .flex_1()
        .items_center()
        .gap(spacing(2.0))
        .child(
            div()
                .size(px(6.0))
                .flex_none()
                .rounded(radius::FULL)
                .bg(dot_color),
        )
        .child(provider_icon(session.provider, 14.0))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_scale(crate::theme::text_scale::XS)
                .text_color(theme.text.primary)
                .child(title.to_string()),
        )
        .children(subtask_count)
        .children(elapsed)
}

/// provider별 세션 수를 목록 위에 고정한다
fn render_tally(
    theme: &ThemeTokens,
    labels: &Translator,
    sessions: &[LiveAiSession],
) -> AnyElement {
    let chips = count_by_provider(sessions.iter().map(|session| session.provider))
        .into_iter()
        .map(|(provider, count)| {
            let name = labels.translate_with(
                "providerSessionCount",
                &[("provider", provider.id()), ("count", &count.to_string())],
            );
            div()
                .relative()
                .flex()
                .items_center()
                .gap(spacing(1.0))
                .rounded(radius::BASE)
                .border_1()
                .border_color(theme.border.subtle)
                .bg(theme.background.page)
                .px(px(6.0))
                .py(px(2.0))
                .child(provider_icon(provider, 12.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .line_height(px(11.0))
                        .css_weight(600)
                        .text_color(theme.text.primary)
                        .child(count.to_string()),
                )
                .child(test_id_layer(name, count.to_string()))
        });
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .border_b_1()
        .border_color(theme.border.subtle)
        .px(spacing(2.0))
        .py(px(6.0))
        .children(chips)
        .into_any_element()
}

fn render_subtasks(
    theme: &ThemeTokens,
    labels: &Translator,
    session: &LiveAiSession,
    now_ms: i64,
) -> Option<AnyElement> {
    if session.running_subtasks.is_empty() {
        return None;
    }
    let rows = session.running_subtasks.iter().map(|subtask| {
        div()
            .flex()
            .items_center()
            .gap(spacing(1.0))
            .py(px(2.0))
            .pl(px(14.0))
            .border_l_1()
            .border_color(theme.border.default)
            .text_size(px(10.0))
            .line_height(px(14.0))
            .text_color(theme.text.secondary)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .child(subtask.name.clone().unwrap_or_else(|| subtask.id.clone())),
            )
            .children(
                elapsed_label(labels, subtask.last_active_at.as_deref(), now_ms)
                    .map(|label| small_label(theme, label)),
            )
    });
    Some(
        div()
            .mt(spacing(1.0))
            .ml(px(13.0))
            .children(rows)
            .into_any_element(),
    )
}

fn small_label(theme: &ThemeTokens, text: String) -> impl IntoElement {
    div()
        .flex_none()
        .text_size(px(10.0))
        .line_height(px(14.0))
        .text_color(theme.text.muted)
        .child(text)
}

/// 마지막 활동으로부터 지난 시간. 시각이 없거나 미래면 그리지 않는다
fn elapsed_label(labels: &Translator, last_active_at: Option<&str>, now_ms: i64) -> Option<String> {
    let elapsed_ms = now_ms - parse_iso_utc_ms(last_active_at?)?;
    if elapsed_ms < 0 {
        return None;
    }
    Some(if elapsed_ms >= HOUR_MS {
        labels.translate_with(
            "elapsedHours",
            &[("hours", &(elapsed_ms / HOUR_MS).to_string())],
        )
    } else if elapsed_ms >= MINUTE_MS {
        labels.translate_with(
            "elapsedMinutes",
            &[("minutes", &(elapsed_ms / MINUTE_MS).to_string())],
        )
    } else {
        labels.translate_with(
            "elapsedSeconds",
            &[("seconds", &(elapsed_ms / 1_000).to_string())],
        )
    })
}

trait SessionRowDivider: Styled + Sized {
    /// 마지막 행이 아니면 아래 구분선을 긋는다(`border-b last:border-b-0`)
    fn when_not_last(self, is_not_last: bool, theme: &ThemeTokens) -> Self {
        if is_not_last {
            self.border_b_1().border_color(theme.border.subtle)
        } else {
            self
        }
    }
}

impl<E: Styled> SessionRowDivider for E {}
