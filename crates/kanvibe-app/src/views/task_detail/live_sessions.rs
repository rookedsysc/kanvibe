//! 실행중 세션 패널(TS `LiveAiSessionPanel`·`useTaskLiveAiSessions`).
//!
//! 패널이 열려 있는 동안 2초마다 이 작업의 실행중 세션을 다시 읽는다. 소속은 색띠로, 계층은 선으로 나눠 그린다.
//! 터미널 창을 찾은 세션 행을 누르면 그 tmux window로 옮기고, 세션 id가 있으면 호출 그래프를 열 수 있다.

use std::time::Duration;

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Rgba, SharedString,
    StatefulInteractiveElement, Styled, Task, div, px,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::running_agent_panes::count_by_provider;
use kanvibe_ai::types::{AgentCallGraph, LiveAiSession, LiveAiSessionState};
use kanvibe_domain::i18n::Translator;

use super::TaskDetailView;
use super::ai_provider_icon::ai_provider_icon;
use super::session_queries::load_live_sessions;
use super::tabs::TabCommand;
use crate::components::{TailwindText, fade};
use crate::qa::options::parse_iso_utc_ms;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::AppServices;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 세션 패널은 서브태스크가 뜨고 지는 것을 따라갈 수 있게 짧게 돈다
const LIVE_SESSION_POLL_INTERVAL: Duration = Duration::from_millis(2_000);
const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 60 * MINUTE_MS;

/// 패널 상태. 목록과 그래프는 같은 dock 자리를 번갈아 쓴다
#[derive(Default)]
pub struct LiveSessionsState {
    pub sessions: Vec<LiveAiSession>,
    /// 호출 그래프를 펼쳐 둔 세션
    pub graph_session: Option<LiveAiSession>,
    /// 아직 읽지 못했으면 `None`
    pub graph: Option<AgentCallGraph>,
    pub(super) poll: Option<Task<()>>,
    pub(super) graph_poll: Option<Task<()>>,
}

impl LiveSessionsState {
    /// 그래프를 닫고 목록으로 돌아간다. 그래프 폴링도 멈춘다
    pub fn close_graph(&mut self) {
        self.graph_session = None;
        self.graph = None;
        self.graph_poll = None;
    }
}

/// 서브태스크 가지 한 줄
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtaskBranch {
    pub name: String,
    pub elapsed: Option<String>,
    /// 마지막 가지에서만 세로줄을 끊는다
    pub is_last: bool,
}

/// 세션 행 하나가 보이는 내용
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSessionRow {
    pub provider: AiSessionProvider,
    /// 지금 하고 있는 작업. 못 읽었으면 provider 이름
    pub title: String,
    pub is_running: bool,
    pub state_label: String,
    pub subtask_count: Option<String>,
    pub elapsed: Option<String>,
    /// 터미널 창을 찾은 세션만 누를 수 있다
    pub focus_label: Option<String>,
    /// 세션 id를 읽었을 때만 그래프 버튼을 붙인다
    pub has_graph_button: bool,
    pub subtasks: Vec<SubtaskBranch>,
}

/// 마지막 활동으로부터 지난 시간. 시각이 없거나 미래면 `None`
pub fn elapsed_label(
    translator: &Translator,
    last_active_at: Option<&str>,
    now_ms: i64,
) -> Option<String> {
    let elapsed_ms = now_ms - parse_iso_utc_ms(last_active_at?)?;
    if elapsed_ms < 0 {
        return None;
    }
    let label = if elapsed_ms >= HOUR_MS {
        let hours = (elapsed_ms / HOUR_MS).to_string();
        translator.translate_with("liveSessions.elapsedHours", &[("hours", &hours)])
    } else if elapsed_ms >= MINUTE_MS {
        let minutes = (elapsed_ms / MINUTE_MS).to_string();
        translator.translate_with("liveSessions.elapsedMinutes", &[("minutes", &minutes)])
    } else {
        let seconds = (elapsed_ms / 1_000).to_string();
        translator.translate_with("liveSessions.elapsedSeconds", &[("seconds", &seconds)])
    };
    Some(label)
}

pub fn live_session_title(session: &LiveAiSession) -> String {
    session
        .current_task
        .clone()
        .unwrap_or_else(|| session.provider.id().to_string())
}

/// provider별 세션 수(provider 이름순)
pub fn live_session_tally(sessions: &[LiveAiSession]) -> Vec<(AiSessionProvider, usize)> {
    count_by_provider(sessions.iter().map(|session| session.provider))
}

pub fn live_session_row(
    session: &LiveAiSession,
    translator: &Translator,
    now_ms: i64,
    can_select: bool,
    can_open_graph: bool,
) -> LiveSessionRow {
    let is_running = session.state == LiveAiSessionState::Running;
    let subtask_total = session.running_subtasks.len();
    LiveSessionRow {
        provider: session.provider,
        title: live_session_title(session),
        is_running,
        state_label: translator.translate(if is_running {
            "liveSessions.running"
        } else {
            "liveSessions.idle"
        }),
        subtask_count: (subtask_total > 0).then(|| {
            translator.translate_with(
                "liveSessions.subtaskCount",
                &[("count", &subtask_total.to_string())],
            )
        }),
        elapsed: elapsed_label(translator, session.last_active_at.as_deref(), now_ms),
        focus_label: session
            .terminal_window
            .as_ref()
            .filter(|_| can_select)
            .map(|window| {
                translator.translate_with(
                    "liveSessions.focusTerminal",
                    &[("window", &window.window_name)],
                )
            }),
        has_graph_button: can_open_graph && session.session_id.is_some(),
        subtasks: session
            .running_subtasks
            .iter()
            .enumerate()
            .map(|(index, subtask)| SubtaskBranch {
                name: subtask.name.clone().unwrap_or_else(|| subtask.id.clone()),
                elapsed: elapsed_label(translator, subtask.last_active_at.as_deref(), now_ms),
                is_last: index + 1 == subtask_total,
            })
            .collect(),
    }
}

/// 세션 행 왼쪽 provider 색띠(`kv-agent-rail`). 네 provider 밖은 띠를 그리지 않는다
pub fn agent_rail_color(theme: &ThemeTokens, provider: AiSessionProvider) -> Option<Rgba> {
    match provider {
        AiSessionProvider::Claude => Some(theme.agent.claude),
        AiSessionProvider::Codex => Some(theme.agent.codex),
        AiSessionProvider::Opencode => Some(theme.agent.opencode),
        AiSessionProvider::Gemini => Some(theme.agent.gemini),
        _ => None,
    }
}

pub fn state_dot_color(theme: &ThemeTokens, is_running: bool) -> Rgba {
    if is_running {
        theme.status.success
    } else {
        theme.status.todo
    }
}

impl TaskDetailView {
    /// 패널이 보이면 폴링을 걸고, 닫히면 멈춘다. 마지막으로 읽은 목록은 남긴다
    pub(super) fn sync_live_session_polling(&mut self, is_visible: bool, cx: &mut Context<Self>) {
        match (is_visible, self.live_sessions.poll.is_some()) {
            (true, false) => {
                let task_id = self.task_id.clone();
                self.live_sessions.poll = Some(cx.spawn(async move |this, cx| {
                    loop {
                        let read = cx.update(|cx| load_live_sessions(&task_id, cx));
                        // 폴링 실패는 다음 주기에 다시 시도한다. 화면을 오류로 덮지 않는다
                        if let Ok(sessions) = read.await {
                            let is_alive = this.update(cx, |view, cx| {
                                view.live_sessions.sessions = sessions;
                                cx.notify();
                            });
                            if is_alive.is_err() {
                                return;
                            }
                        }
                        cx.background_executor()
                            .timer(LIVE_SESSION_POLL_INTERVAL)
                            .await;
                    }
                }));
            }
            (false, true) => self.live_sessions.poll = None,
            _ => {}
        }
    }

    /// 세션이 붙어 있는 tmux window로 옮기고 입력 포커스를 터미널로 넘긴다
    fn focus_session_terminal(
        &mut self,
        window_id: String,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        self.run_tab_command(TabCommand::Select(window_id), cx);
        self.focus_terminal(window, cx);
    }

    pub(super) fn render_live_sessions_body(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.live_sessions.graph_session.is_some() {
            return self.render_agent_call_graph(theme, cx);
        }
        let card = div()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface);
        let sessions = &self.live_sessions.sessions;
        if sessions.is_empty() {
            return card
                .child(
                    div()
                        .px(spacing(2.0))
                        .py(spacing(3.0))
                        .text_scale(text_scale::XS)
                        .text_color(theme.text.muted)
                        .child(self.translator.translate("liveSessions.empty")),
                )
                .into_any_element();
        }
        let now_ms = cx.global::<AppServices>().clock.now_ms();
        let row_count = sessions.len();
        let rows = sessions.iter().enumerate().map(|(index, session)| {
            let row = live_session_row(session, &self.translator, now_ms, true, true);
            let row_frame =
                div().child(self.render_live_session_row(theme, index, session, row, cx));
            match index + 1 < row_count {
                true => row_frame.border_b_1().border_color(theme.border.subtle),
                false => row_frame,
            }
        });
        card.child(render_tally(theme, sessions))
            .children(rows)
            .into_any_element()
    }

    fn render_live_session_row(
        &self,
        theme: &ThemeTokens,
        index: usize,
        session: &LiveAiSession,
        row: LiveSessionRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let summary = self.render_row_target(theme, index, session, &row, cx);
        let graph_button = row
            .has_graph_button
            .then(|| self.render_graph_button(theme, index, session, cx));
        let rail =
            agent_rail_color(theme, row.provider).unwrap_or(gpui_kit::transparent_black().into());
        div()
            .border_l_2()
            .border_color(rail)
            .px(spacing(2.0))
            .py(spacing(1.5))
            .child(
                div()
                    .flex()
                    .w_full()
                    .items_center()
                    .gap(spacing(1.0))
                    .child(summary)
                    .children(graph_button),
            )
            .children(row.is_running.then(|| {
                div()
                    .mt(spacing(1.0))
                    .ml(spacing(6.0))
                    .h(px(2.0))
                    .rounded_full()
                    .bg(theme.status.success)
            }))
            .children((!row.subtasks.is_empty()).then(|| render_subtasks(theme, &row.subtasks)))
            .into_any_element()
    }

    /// 행 요약. 터미널 창을 찾은 세션만 눌러 그 창으로 옮길 수 있다
    fn render_row_target(
        &self,
        theme: &ThemeTokens,
        index: usize,
        session: &LiveAiSession,
        row: &LiveSessionRow,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let target = div()
            .id(("live-session", index))
            .flex()
            .min_w_0()
            .flex_1()
            .items_center()
            .rounded(radius::SM)
            .px(spacing(1.0))
            .py(spacing(0.5))
            .child(render_row_summary(theme, row));
        let terminal_window = session
            .terminal_window
            .as_ref()
            .filter(|_| row.focus_label.is_some());
        let Some(terminal_window) = terminal_window else {
            return target;
        };
        let window_id = terminal_window.window_id.clone();
        let hover = theme.background.page;
        target
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.focus_session_terminal(window_id.clone(), window, cx)
            }))
    }

    fn render_graph_button(
        &self,
        theme: &ThemeTokens,
        index: usize,
        session: &LiveAiSession,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let graph_session = session.clone();
        let label = self.translator.translate("liveSessions.graph.open");
        small_button(theme, ("live-session-graph", index), label)
            .border_1()
            .border_color(theme.border.subtle)
            .px(spacing(1.0))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.open_agent_call_graph(graph_session.clone(), cx)
            }))
    }
}

/// `text-[10px]` 등 작은 버튼(`rounded py-0.5 hover:bg-bg-page`)
pub(super) fn small_button(
    theme: &ThemeTokens,
    id: impl Into<gpui_kit::ElementId>,
    label: String,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let hover = theme.background.page;
    div()
        .id(id)
        .relative()
        .flex_none()
        .rounded(radius::SM)
        .py(spacing(0.5))
        .text_scale(text_scale::PX_10)
        .text_color(theme.text.secondary)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: SharedString::from(label.clone()),
            text: label.into(),
        }))
}

fn render_tally(theme: &ThemeTokens, sessions: &[LiveAiSession]) -> impl IntoElement {
    let chips = live_session_tally(sessions)
        .into_iter()
        .map(|(provider, count)| {
            div()
                .flex()
                .items_center()
                .gap(spacing(1.0))
                .rounded(radius::SM)
                .border_1()
                .border_color(theme.border.subtle)
                .bg(theme.background.page)
                .px(spacing(1.5))
                .py(spacing(0.5))
                .child(ai_provider_icon(provider, 12.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .line_height(px(11.0))
                        .css_weight(600)
                        .text_color(theme.text.primary)
                        .child(count.to_string()),
                )
        });
    div()
        .flex()
        .items_center()
        .gap(spacing(1.5))
        .border_b_1()
        .border_color(theme.border.subtle)
        .px(spacing(2.0))
        .py(spacing(1.5))
        .children(chips)
}

fn render_row_summary(theme: &ThemeTokens, row: &LiveSessionRow) -> impl IntoElement {
    let muted_small = |label: String| {
        div()
            .flex_none()
            .text_scale(text_scale::PX_10)
            .text_color(theme.text.muted)
            .child(label)
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
                .rounded_full()
                .bg(state_dot_color(theme, row.is_running)),
        )
        .child(ai_provider_icon(row.provider, 14.0))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_scale(text_scale::XS)
                .text_color(theme.text.primary)
                .child(row.title.clone()),
        )
        .children(row.subtask_count.clone().map(muted_small))
        .children(row.elapsed.clone().map(muted_small))
}

/// 세션에 매달린 서브에이전트 가지(`kv-subtask-branch`): 세로선은 계속 내려오고 마지막 항목에서만 끊긴다
fn render_subtasks(theme: &ThemeTokens, subtasks: &[SubtaskBranch]) -> impl IntoElement {
    let line = theme.border.default;
    div()
        .mt(spacing(1.0))
        .ml(px(13.0))
        .children(subtasks.iter().map(|subtask| {
            let vertical = div().absolute().left_0().top_0().w(px(1.0)).bg(line);
            let vertical = if subtask.is_last {
                vertical.h(gpui_kit::relative(0.5))
            } else {
                vertical.bottom_0()
            };
            div()
                .relative()
                .flex()
                .items_center()
                .gap(spacing(1.0))
                .py(spacing(0.5))
                .pl(spacing(3.5))
                .text_scale(text_scale::PX_10)
                .text_color(theme.text.secondary)
                .child(vertical)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top(gpui_kit::relative(0.5))
                        .w(px(10.0))
                        .h(px(1.0))
                        .bg(line),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .child(subtask.name.clone()),
                )
                .children(subtask.elapsed.clone().map(|elapsed| {
                    div()
                        .flex_none()
                        .text_color(theme.text.muted)
                        .child(elapsed)
                }))
        }))
}

/// `color-mix(in srgb, color X%, transparent)`
pub(super) fn mix_transparent(color: Rgba, ratio: f32) -> Rgba {
    fade(color, color.a * ratio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_ai::types::{LiveAiSubtask, LiveSessionTerminalWindow};
    use kanvibe_domain::i18n::Locale;

    const NOW_MS: i64 = 1_768_445_998_000; // 2026-01-15T02:59:58Z

    fn translator() -> Translator {
        Translator::new(Locale::Ko, "taskDetail")
    }

    fn session(provider: AiSessionProvider, state: LiveAiSessionState) -> LiveAiSession {
        LiveAiSession {
            provider,
            session_id: Some("session-1".into()),
            current_task: Some("보드 필터 구현".into()),
            state,
            last_active_at: Some("2026-01-15T02:59:56.000Z".into()),
            running_subtasks: Vec::new(),
            terminal_window: None,
        }
    }

    fn subtask(id: &str, name: Option<&str>) -> LiveAiSubtask {
        LiveAiSubtask {
            id: id.into(),
            name: name.map(str::to_string),
            last_active_at: Some("2026-01-15T02:59:55.000Z".into()),
        }
    }

    fn row(session: &LiveAiSession) -> LiveSessionRow {
        live_session_row(session, &translator(), NOW_MS, true, true)
    }

    // parity: TS-renderer-LiveAiSessionPanel-0421a2
    #[test]
    fn running_and_idle_sessions_get_different_state_dots() {
        let theme = ThemeTokens::for_mode(crate::theme::ThemeMode::Dark);
        let running = row(&session(
            AiSessionProvider::Claude,
            LiveAiSessionState::Running,
        ));
        let idle = row(&session(AiSessionProvider::Codex, LiveAiSessionState::Idle));
        assert_eq!(running.state_label, "실행중");
        assert_eq!(idle.state_label, "유휴");
        assert_ne!(
            state_dot_color(&theme, running.is_running),
            state_dot_color(&theme, idle.is_running)
        );
    }

    // parity: TS-renderer-LiveAiSessionPanel-cd66d3
    #[test]
    fn shows_running_subtask_count_and_names() {
        let mut claude = session(AiSessionProvider::Claude, LiveAiSessionState::Running);
        claude.running_subtasks = vec![subtask("a", Some("코드 탐색")), subtask("b", None)];
        let row = row(&claude);
        assert_eq!(row.subtask_count.as_deref(), Some("subtask 2개"));
        let names: Vec<&str> = row
            .subtasks
            .iter()
            .map(|branch| branch.name.as_str())
            .collect();
        assert_eq!(names, ["코드 탐색", "b"]);
    }

    // parity: TS-renderer-LiveAiSessionPanel-a0e6f9
    #[test]
    fn tally_counts_sessions_per_provider_in_name_order() {
        let sessions = [
            session(AiSessionProvider::Codex, LiveAiSessionState::Running),
            session(AiSessionProvider::Claude, LiveAiSessionState::Running),
            session(AiSessionProvider::Claude, LiveAiSessionState::Idle),
        ];
        assert_eq!(
            live_session_tally(&sessions),
            [
                (AiSessionProvider::Claude, 2),
                (AiSessionProvider::Codex, 1)
            ]
        );
    }

    // parity: TS-renderer-LiveAiSessionPanel-bb1c3b
    #[test]
    fn session_rows_carry_the_provider_rail_color() {
        let theme = ThemeTokens::for_mode(crate::theme::ThemeMode::Dark);
        assert_eq!(
            agent_rail_color(&theme, AiSessionProvider::Claude),
            Some(theme.agent.claude)
        );
        assert_eq!(
            agent_rail_color(&theme, AiSessionProvider::Codex),
            Some(theme.agent.codex)
        );
        assert_ne!(theme.agent.claude, theme.agent.codex);
    }

    // parity: TS-renderer-LiveAiSessionPanel-b6833b
    #[test]
    fn shows_time_since_last_activity() {
        let translator = translator();
        let at = |offset_ms: i64| {
            let total = (NOW_MS - offset_ms) / 1_000;
            let (minutes, seconds) = ((total / 60) % 60, total % 60);
            let hours = (total / 3_600) % 24;
            format!("2026-01-15T{hours:02}:{minutes:02}:{seconds:02}.000Z")
        };
        let label = |offset_ms| elapsed_label(&translator, Some(&at(offset_ms)), NOW_MS);
        assert_eq!(label(5_000).as_deref(), Some("5초 전"));
        assert_eq!(label(3 * MINUTE_MS).as_deref(), Some("3분 전"));
        assert_eq!(label(2 * HOUR_MS).as_deref(), Some("2시간 전"));
        assert_eq!(elapsed_label(&translator, None, NOW_MS), None);
        assert_eq!(label(-5_000), None);
    }

    // parity: TS-renderer-LiveAiSessionPanel-590973
    #[test]
    fn only_sessions_with_a_terminal_window_are_clickable() {
        let mut attached = session(AiSessionProvider::Claude, LiveAiSessionState::Running);
        attached.terminal_window = Some(LiveSessionTerminalWindow {
            session_name: "kanvibe".into(),
            window_id: "@3".into(),
            window_name: "feat".into(),
        });
        let detached = session(AiSessionProvider::Codex, LiveAiSessionState::Running);
        assert_eq!(
            row(&attached).focus_label.as_deref(),
            Some("feat 터미널로 이동")
        );
        assert_eq!(row(&detached).focus_label, None);
        let without_handler = live_session_row(&attached, &translator(), NOW_MS, false, true);
        assert_eq!(without_handler.focus_label, None);
    }

    // parity: TS-renderer-LiveAiSessionPanel-3f192f
    #[test]
    fn shows_the_current_task_instead_of_the_provider() {
        assert_eq!(
            row(&session(
                AiSessionProvider::Claude,
                LiveAiSessionState::Running
            ))
            .title,
            "보드 필터 구현"
        );
    }

    // parity: TS-renderer-LiveAiSessionPanel-d6177d
    #[test]
    fn falls_back_to_the_provider_name_without_a_task() {
        let mut unknown = session(AiSessionProvider::Codex, LiveAiSessionState::Running);
        unknown.current_task = None;
        assert_eq!(row(&unknown).title, "codex");
    }

    // parity: TS-renderer-LiveAiSessionPanel-1fe7a4
    #[test]
    fn subtasks_hang_as_branches_and_only_the_last_closes() {
        let mut claude = session(AiSessionProvider::Claude, LiveAiSessionState::Running);
        claude.running_subtasks = vec![subtask("a", None), subtask("b", None), subtask("c", None)];
        let last_flags: Vec<bool> = row(&claude)
            .subtasks
            .iter()
            .map(|branch| branch.is_last)
            .collect();
        assert_eq!(last_flags, [false, false, true]);
    }

    // parity: TS-renderer-LiveAiSessionPanel-95b549
    #[test]
    fn a_single_subtask_is_the_last_branch() {
        let mut claude = session(AiSessionProvider::Claude, LiveAiSessionState::Running);
        claude.running_subtasks = vec![subtask("only", None)];
        assert!(row(&claude).subtasks[0].is_last);
    }

    // parity: TS-renderer-LiveAiSessionPanel-bacfa0
    #[test]
    fn progress_is_drawn_only_for_running_sessions() {
        assert!(
            row(&session(
                AiSessionProvider::Claude,
                LiveAiSessionState::Running
            ))
            .is_running
        );
        assert!(
            !row(&session(
                AiSessionProvider::Claude,
                LiveAiSessionState::Idle
            ))
            .is_running
        );
    }

    // parity: TS-renderer-LiveAiSessionPanel-8f9512
    #[test]
    fn graph_button_is_attached_when_the_graph_can_open() {
        let claude = session(AiSessionProvider::Claude, LiveAiSessionState::Running);
        assert!(row(&claude).has_graph_button);
        assert!(!live_session_row(&claude, &translator(), NOW_MS, true, false).has_graph_button);
    }

    // parity: TS-renderer-LiveAiSessionPanel-fde54b
    #[test]
    fn no_graph_button_without_a_session_id() {
        let mut claude = session(AiSessionProvider::Claude, LiveAiSessionState::Running);
        claude.session_id = None;
        assert!(!row(&claude).has_graph_button);
    }

    // parity: TS-renderer-LiveAiSessionPanel-37dcb2
    #[test]
    fn empty_panel_says_no_session_is_running() {
        assert_eq!(
            translator().translate("liveSessions.empty"),
            "실행중인 AI 세션이 없습니다."
        );
        assert!(live_session_tally(&[]).is_empty());
    }
}
