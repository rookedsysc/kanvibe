//! 호출 그래프 패널(TS `AgentCallGraphPanel`·`useTaskAgentCallGraph`).
//!
//! 왼쪽은 계보, 오른쪽은 모든 행이 함께 쓰는 하나의 시간축이다. 막대가 겹치면 병렬, 어긋나면 순차로 읽힌다.
//! 그래프는 기록 전문을 훑어 만들어 비싸므로 열어 둔 동안에만 10초마다 다시 읽는다.

use std::time::Duration;

use gpui_kit::{
    AnyElement, Context, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div, px,
    relative,
};
use kanvibe_ai::running_agent_panes::flatten_agent_call_nodes;
use kanvibe_ai::types::{AgentCallGraph, AgentCallNode, LiveAiSession};
use kanvibe_domain::i18n::Translator;

use super::TaskDetailView;
use super::ai_provider_icon::ai_provider_icon;
use super::ai_reader::load_agent_call_graph;
use super::live_sessions::{live_session_title, mix_transparent, small_button};
use crate::components::TailwindText;
use crate::qa::options::parse_iso_utc_ms;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

const AGENT_CALL_GRAPH_POLL_INTERVAL: Duration = Duration::from_millis(10_000);
/// `w-[168px]` 시간축 폭
const TRACK_WIDTH: f32 = 168.0;
/// `.kv-graph-guide` 한 칸 폭
const GUIDE_WIDTH: f32 = 14.0;

/// 한 행 왼쪽에 그리는 계보 선 조각
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallGuide {
    /// 계속 내려오는 세로줄
    Pipe,
    /// 조상이 막내였던 빈칸
    Gap,
    /// 형제가 더 있는 꺾임
    Tee,
    /// 막내에서 끝나는 꺾임
    Elbow,
}

/// 시간축 위 막대. 비율은 0~100(%)이다
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CallBar {
    pub left_percent: f32,
    pub width_percent: f32,
    pub is_running: bool,
}

/// 그래프 한 행
#[derive(Debug, Clone, PartialEq)]
pub struct CallRow {
    pub agent_type: Option<String>,
    pub skill: Option<String>,
    pub label: String,
    pub is_running: bool,
    pub guides: Vec<CallGuide>,
    pub bar: Option<CallBar>,
}

/// 그래프 본문
#[derive(Debug, Clone, PartialEq)]
pub enum CallGraphBody {
    Loading,
    Empty,
    Rows(Vec<CallRow>),
}

/// 트리를 화면 순서대로 펴면서 각 행의 선 조각을 정한다
fn collect_rows<'a>(
    nodes: &'a [AgentCallNode],
    ancestors: &[CallGuide],
    rows: &mut Vec<(&'a AgentCallNode, Vec<CallGuide>)>,
) {
    for (index, node) in nodes.iter().enumerate() {
        let is_last_sibling = index + 1 == nodes.len();
        let mut guides = ancestors.to_vec();
        guides.push(if is_last_sibling {
            CallGuide::Elbow
        } else {
            CallGuide::Tee
        });
        rows.push((node, guides));
        let mut child_ancestors = ancestors.to_vec();
        child_ancestors.push(if is_last_sibling {
            CallGuide::Gap
        } else {
            CallGuide::Pipe
        });
        collect_rows(&node.children, &child_ancestors, rows);
    }
}

fn graph_depth(nodes: &[AgentCallNode]) -> usize {
    nodes
        .iter()
        .map(|node| 1 + graph_depth(&node.children))
        .max()
        .unwrap_or(0)
}

fn bounded_percent(ratio: f64) -> f32 {
    if ratio.is_finite() {
        (ratio * 100.0).clamp(0.0, 100.0) as f32
    } else {
        0.0
    }
}

/// 모든 막대가 같은 자를 쓴다. 시작 시각을 못 읽은 노드는 막대를 그리지 않는다
fn call_bar(node: &AgentCallNode, graph: &AgentCallGraph) -> Option<CallBar> {
    let read_at_ms = parse_iso_utc_ms(&graph.read_at)? as f64;
    let start_ms = graph
        .started_at
        .as_deref()
        .and_then(parse_iso_utc_ms)
        .map_or(read_at_ms, |ms| ms as f64);
    let span_ms = (read_at_ms - start_ms).max(1.0);
    let started_at_ms = parse_iso_utc_ms(node.started_at.as_deref()?)? as f64;
    let is_running = node.ended_at.is_none();
    let ended_at_ms = match node.ended_at.as_deref() {
        None => read_at_ms,
        Some(ended_at) => parse_iso_utc_ms(ended_at).map_or(f64::NAN, |ms| ms as f64),
    };
    let left_percent = bounded_percent((started_at_ms - start_ms) / span_ms);
    let right_percent = bounded_percent((ended_at_ms - start_ms) / span_ms);
    Some(CallBar {
        left_percent,
        width_percent: (right_percent - left_percent).max(1.0),
        is_running,
    })
}

/// 머리말 집계 `서브 {total} · 실행중 {running} · 깊이 {depth}`
pub fn call_graph_summary(translator: &Translator, graph: Option<&AgentCallGraph>) -> String {
    let nodes = graph
        .map(|graph| flatten_agent_call_nodes(&graph.roots))
        .unwrap_or_default();
    let running = nodes.iter().filter(|node| node.ended_at.is_none()).count();
    let depth = graph.map_or(0, |graph| graph_depth(&graph.roots));
    translator.translate_with(
        "liveSessions.graph.summary",
        &[
            ("total", &nodes.len().to_string()),
            ("running", &running.to_string()),
            ("depth", &depth.to_string()),
        ],
    )
}

pub fn call_graph_body(graph: Option<&AgentCallGraph>) -> CallGraphBody {
    let Some(graph) = graph else {
        return CallGraphBody::Loading;
    };
    if graph.roots.is_empty() {
        return CallGraphBody::Empty;
    }
    let mut rows = Vec::new();
    collect_rows(&graph.roots, &[], &mut rows);
    CallGraphBody::Rows(
        rows.into_iter()
            .map(|(node, guides)| CallRow {
                agent_type: node.agent_type.clone(),
                skill: node.skill.clone(),
                label: node.task.clone().unwrap_or_else(|| node.id.clone()),
                is_running: node.ended_at.is_none(),
                guides,
                bar: call_bar(node, graph),
            })
            .collect(),
    )
}

impl TaskDetailView {
    pub(super) fn open_agent_call_graph(&mut self, session: LiveAiSession, cx: &mut Context<Self>) {
        let Some(session_id) = session.session_id.clone() else {
            return;
        };
        let provider = session.provider;
        self.live_sessions.graph_session = Some(session);
        self.live_sessions.graph = None;
        let task_id = self.task_id.clone();
        self.live_sessions.graph_poll = Some(cx.spawn(async move |this, cx| {
            loop {
                let read =
                    cx.update(|cx| load_agent_call_graph(&task_id, provider, &session_id, cx));
                if let Ok(graph) = read.await {
                    let is_alive = this.update(cx, |view, cx| {
                        view.live_sessions.graph = graph;
                        cx.notify();
                    });
                    if is_alive.is_err() {
                        return;
                    }
                }
                cx.background_executor()
                    .timer(AGENT_CALL_GRAPH_POLL_INTERVAL)
                    .await;
            }
        }));
        cx.notify();
    }

    fn close_agent_call_graph(&mut self, cx: &mut Context<Self>) {
        self.live_sessions.close_graph();
        cx.notify();
    }

    pub(super) fn render_agent_call_graph(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(session) = self.live_sessions.graph_session.as_ref() else {
            return div().into_any_element();
        };
        let graph = self.live_sessions.graph.as_ref();
        let notice = |key: &str| {
            div()
                .px(spacing(2.0))
                .py(spacing(3.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(self.translator.translate(key))
                .into_any_element()
        };
        let body = match call_graph_body(graph) {
            CallGraphBody::Loading => notice("liveSessions.graph.loading"),
            CallGraphBody::Empty => notice("liveSessions.graph.empty"),
            CallGraphBody::Rows(rows) => render_timeline(theme, &self.translator, &rows),
        };
        div()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .child(self.render_graph_header(theme, session, graph, cx))
            .child(body)
            .into_any_element()
    }

    /// `← 목록`, provider, 세션 작업, 집계
    fn render_graph_header(
        &self,
        theme: &ThemeTokens,
        session: &LiveAiSession,
        graph: Option<&AgentCallGraph>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let back = small_button(
            theme,
            "agent-call-graph-back",
            self.translator.translate("liveSessions.graph.back"),
        )
        .px(spacing(1.5))
        .text_size(px(11.0))
        .line_height(px(16.5))
        .on_click(cx.listener(|view, _, _, cx| view.close_agent_call_graph(cx)));
        div()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .border_b_1()
            .border_color(theme.border.subtle)
            .px(spacing(2.0))
            .py(spacing(1.5))
            .child(back)
            .child(ai_provider_icon(session.provider, 14.0))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.primary)
                    .child(live_session_title(session)),
            )
            .child(
                div()
                    .flex_none()
                    .text_scale(text_scale::PX_10)
                    .text_color(theme.text.muted)
                    .child(call_graph_summary(&self.translator, graph)),
            )
    }
}

fn tiny_text(theme: &ThemeTokens, label: String) -> gpui_kit::Div {
    div()
        .text_size(px(9.0))
        .line_height(px(13.5))
        .text_color(theme.text.muted)
        .child(label)
}

fn render_timeline(theme: &ThemeTokens, translator: &Translator, rows: &[CallRow]) -> AnyElement {
    let scale = div()
        .flex()
        .items_center()
        .gap(spacing(3.0))
        .pb(spacing(1.0))
        .child(
            tiny_text(
                theme,
                translator
                    .translate("liveSessions.graph.lineage")
                    .to_uppercase(),
            )
            .min_w_0()
            .flex_1(),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .w(px(TRACK_WIDTH))
                .justify_between()
                .child(tiny_text(
                    theme,
                    translator.translate("liveSessions.graph.start"),
                ))
                .child(tiny_text(
                    theme,
                    translator.translate("liveSessions.graph.now"),
                )),
        );
    div()
        .px(spacing(2.0))
        .py(spacing(1.5))
        .child(scale)
        .children(rows.iter().map(|row| render_call_row(theme, row)))
        .into_any_element()
}

fn render_guide(theme: &ThemeTokens, guide: CallGuide) -> impl IntoElement {
    let line = theme.border.default;
    let frame = div().relative().w(px(GUIDE_WIDTH)).flex_none();
    let vertical = div().absolute().left(px(6.0)).top_0().w(px(1.0)).bg(line);
    let horizontal = div()
        .absolute()
        .left(px(6.0))
        .top(relative(0.5))
        .w(px(8.0))
        .h(px(1.0))
        .bg(line);
    match guide {
        CallGuide::Gap => frame,
        CallGuide::Pipe => frame.child(vertical.bottom_0()),
        CallGuide::Tee => frame.child(vertical.bottom_0()).child(horizontal),
        CallGuide::Elbow => frame.child(vertical.h(relative(0.5))).child(horizontal),
    }
}

fn render_call_row(theme: &ThemeTokens, row: &CallRow) -> impl IntoElement {
    let content = render_call_label(theme, row);
    div()
        .flex()
        .min_h(px(24.0))
        .items_center()
        .gap(spacing(3.0))
        .child(
            div()
                .flex()
                .min_w_0()
                .flex_1()
                .self_stretch()
                .children(row.guides.iter().map(|guide| render_guide(theme, *guide)))
                .child(content),
        )
        .child(render_track(theme, row.bar))
}

/// 에이전트 종류·스킬 배지와 작업 이름. 끝난 가지는 흐리게
fn render_call_label(theme: &ThemeTokens, row: &CallRow) -> impl IntoElement {
    let badge = || {
        div()
            .flex_none()
            .rounded(radius::SM)
            .border_1()
            .border_color(theme.border.subtle)
            .px(spacing(1.0))
            .text_size(px(9.0))
            .line_height(px(13.5))
    };
    let label_color = if row.is_running {
        theme.text.primary
    } else {
        theme.text.secondary
    };
    div()
        .flex()
        .min_w_0()
        .flex_1()
        .items_center()
        .gap(spacing(1.5))
        .children(row.agent_type.as_ref().map(|agent_type| {
            badge()
                .text_color(theme.text.secondary)
                .child(agent_type.clone())
        }))
        .children(
            row.skill
                .as_ref()
                .map(|skill| badge().text_color(theme.text.muted).child(skill.clone())),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_size(px(11.0))
                .line_height(px(16.5))
                .text_color(label_color)
                .child(row.label.clone()),
        )
}

/// 시간축(`kv-graph-track`): 오른쪽 테두리는 "지금", 25%마다 옅은 눈금
fn render_track(theme: &ThemeTokens, bar: Option<CallBar>) -> impl IntoElement {
    let ticks = (0..4).map(|index| {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(TRACK_WIDTH * index as f32 / 4.0))
            .w(px(1.0))
            .bg(theme.border.subtle)
    });
    let bar = bar.map(|bar| {
        let color = if bar.is_running {
            theme.status.success
        } else {
            mix_transparent(theme.status.success, 0.3)
        };
        div()
            .absolute()
            .top(px(5.0))
            .h(px(6.0))
            .left(relative(bar.left_percent / 100.0))
            .w(relative(bar.width_percent / 100.0))
            .rounded_full()
            .bg(color)
    });
    div()
        .relative()
        .flex_none()
        .h(px(16.0))
        .w(px(TRACK_WIDTH))
        .border_r_1()
        .border_color(theme.border.strong)
        .children(ticks)
        .children(bar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_ai::providers::AiSessionProvider;
    use kanvibe_domain::i18n::Locale;

    fn node(
        id: &str,
        agent_type: Option<&str>,
        task: Option<&str>,
        started_at: Option<&str>,
        ended_at: Option<&str>,
    ) -> AgentCallNode {
        AgentCallNode {
            id: id.into(),
            agent_type: agent_type.map(str::to_string),
            skill: None,
            task: task.map(str::to_string),
            started_at: started_at.map(str::to_string),
            ended_at: ended_at.map(str::to_string),
            children: Vec::new(),
        }
    }

    fn graph(roots: Vec<AgentCallNode>) -> AgentCallGraph {
        AgentCallGraph {
            provider: AiSessionProvider::Claude,
            session_id: "session-a".into(),
            started_at: Some("2026-08-15T00:00:00.000Z".into()),
            read_at: "2026-08-15T00:12:00.000Z".into(),
            roots,
        }
    }

    /// 실행중인 `판정 로직 리뷰`가 밑에 하나를 더 띄웠고, `테스트 작성`은 따로 끝난 12분짜리 세션
    fn nested_graph() -> AgentCallGraph {
        let mut review = node(
            "a1",
            Some("general-purpose"),
            Some("판정 로직 리뷰"),
            Some("2026-08-15T00:05:00.000Z"),
            None,
        );
        review.children.push(node(
            "a2",
            Some("Explore"),
            Some("머지 이력 조회"),
            Some("2026-08-15T00:08:00.000Z"),
            Some("2026-08-15T00:10:00.000Z"),
        ));
        let tests = node(
            "a3",
            Some("Explore"),
            Some("테스트 작성"),
            Some("2026-08-15T00:01:00.000Z"),
            Some("2026-08-15T00:03:00.000Z"),
        );
        graph(vec![review, tests])
    }

    fn rows(graph: &AgentCallGraph) -> Vec<CallRow> {
        match call_graph_body(Some(graph)) {
            CallGraphBody::Rows(rows) => rows,
            other => panic!("rows expected, got {other:?}"),
        }
    }

    fn translator() -> Translator {
        Translator::new(Locale::Ko, "taskDetail")
    }

    // parity: TS-renderer-AgentCallGraphPanel-a48184
    #[test]
    fn flattens_nested_subagents_in_lineage_order() {
        let labels: Vec<(Option<String>, String)> = rows(&nested_graph())
            .into_iter()
            .map(|row| (row.agent_type, row.label))
            .collect();
        assert_eq!(
            labels,
            [
                (Some("general-purpose".into()), "판정 로직 리뷰".into()),
                (Some("Explore".into()), "머지 이력 조회".into()),
                (Some("Explore".into()), "테스트 작성".into()),
            ]
        );
    }

    // parity: TS-renderer-AgentCallGraphPanel-4afece
    #[test]
    fn guides_follow_depth_and_stop_at_the_last_sibling() {
        let guides: Vec<Vec<CallGuide>> = rows(&nested_graph())
            .into_iter()
            .map(|row| row.guides)
            .collect();
        assert_eq!(
            guides,
            [
                vec![CallGuide::Tee],
                vec![CallGuide::Pipe, CallGuide::Elbow],
                vec![CallGuide::Elbow],
            ]
        );
    }

    // parity: TS-renderer-AgentCallGraphPanel-404c0f
    #[test]
    fn bars_share_one_timeline_and_only_running_ones_reach_now() {
        let bars: Vec<CallBar> = rows(&nested_graph())
            .into_iter()
            .filter_map(|row| row.bar)
            .collect();
        let states: Vec<bool> = bars.iter().map(|bar| bar.is_running).collect();
        assert_eq!(states, [true, false, false]);
        assert_eq!(bars[0].left_percent.round(), 42.0);
        assert_eq!(bars[0].width_percent.round(), 58.0);
    }

    // parity: TS-renderer-AgentCallGraphPanel-88d7c4
    #[test]
    fn nodes_without_a_start_time_get_no_bar() {
        let rows = rows(&graph(vec![node(
            "a1",
            None,
            Some("시작 시각 없음"),
            None,
            None,
        )]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].bar, None);
    }

    // parity: TS-renderer-AgentCallGraphPanel-704e11
    #[test]
    fn header_counts_subagents_running_and_depth() {
        assert_eq!(
            call_graph_summary(&translator(), Some(&nested_graph())),
            "서브 3 · 실행중 1 · 깊이 2"
        );
    }

    // parity: TS-renderer-AgentCallGraphPanel-b056a4
    #[test]
    fn graph_without_subagents_shows_the_empty_notice() {
        assert_eq!(
            call_graph_body(Some(&graph(Vec::new()))),
            CallGraphBody::Empty
        );
        assert_eq!(
            translator().translate("liveSessions.graph.empty"),
            "이 세션은 서브에이전트를 띄우지 않았습니다."
        );
    }

    // parity: TS-renderer-AgentCallGraphPanel-a843b3
    #[test]
    fn back_button_returns_to_the_session_list() {
        let mut state = super::super::live_sessions::LiveSessionsState {
            graph_session: Some(LiveAiSession {
                provider: AiSessionProvider::Claude,
                session_id: Some("session-a".into()),
                current_task: None,
                state: kanvibe_ai::types::LiveAiSessionState::Running,
                last_active_at: None,
                running_subtasks: Vec::new(),
                terminal_window: None,
            }),
            graph: Some(nested_graph()),
            ..Default::default()
        };
        assert_eq!(translator().translate("liveSessions.graph.back"), "← 목록");
        state.close_graph();
        assert!(state.graph_session.is_none());
        assert!(state.graph.is_none());
    }

    // parity: TS-renderer-AgentCallGraphPanel-73b5e2
    #[test]
    fn graph_not_read_yet_says_it_is_being_built() {
        assert_eq!(call_graph_body(None), CallGraphBody::Loading);
        assert_eq!(
            call_graph_summary(&translator(), None),
            "서브 0 · 실행중 0 · 깊이 0"
        );
    }
}
