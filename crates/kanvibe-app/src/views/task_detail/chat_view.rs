//! AI 대화 기록 메인 화면(TS `InlineAiChatView`). 왼쪽은 세션 목록·검색·provider 필터, 오른쪽은 고른 세션의 메시지다.
//! dock의 채팅 항목으로 열 때 만들고 처음 쪽을 읽는다. 터미널로 돌아가면 버린다.

use std::collections::HashSet;

use gpui_kit::base::TextSelection;
use gpui_kit::base::input::{Input, InputEvent, InputState};
use gpui_kit::{
    AnyElement, AppContext, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, canvas, div, px, relative,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{AggregatedAiSession, AiMessageRole};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::shortcuts::key_event::match_shortcut_event;

use super::ai_provider_icon::ai_provider_icon;
use super::chat_history::{
    ChatHistoryState, DetailRequest, HistoryRequest, ROLE_FILTERS, session_key, session_preview,
    session_title, transcript_rows,
};
use super::chat_message::role_label;
use super::file_preview::FilePreviewState;
use super::message_content::MessageCopyLabels;
use super::session_queries::{load_session_detail, load_session_list};
use crate::components::scroll_area::scroll_area;
use crate::components::{TailwindText, box_shadows, fade};
use crate::locale_format::format_date;
use crate::qa::perf_markers::ScreenPaintMarks;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::AppServices;
use crate::shortcuts::shortcut_event_from_keystroke;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 성능 표식의 화면 이름
const PERF_SCREEN: &str = "history";
/// 이 폭보다 좁으면 목록과 대화를 한 번에 하나만 보인다(TS `@min-[720px]`)
const NARROW_LAYOUT_WIDTH: f32 = 720.0;
/// 고른 대화 글자를 복사하는 단축키(브라우저 기본 복사)
const COPY_SELECTION_SHORTCUT: &str = "Mod+C";

/// 화면 폭이 좁은 배치인지. 아직 재지 못했으면 넓은 배치로 그린다
pub fn is_narrow_layout(container_width: Option<f32>) -> bool {
    container_width.is_some_and(|width| width < NARROW_LAYOUT_WIDTH)
}

/// 이보다 아래에 떨어져 있으면 최신 메시지 버튼을 띄운다(TS `INLINE_CHAT_NEAR_BOTTOM_PX`)
const NEAR_BOTTOM: f32 = 96.0;
/// TS는 메시지 목록 끝의 빈 기준점을 화면 아래에 맞췄다(`scrollIntoView({block: "end"})`). 기준점 아래의 `py-6`(24px)은
/// 보이지 않고 마지막 메시지 뒤 `space-y-2.5`(10px)만 보이므로, 맨 아래에서 그 차이만큼 덜 내린다.
/// 기준 화면에서 잰 결과 Chromium은 여기서 1px을 덜 남겼다
const LATEST_ANCHOR_INSET: f32 = 24.0 - 10.0 - 1.0;

/// 목록 아래에 떨어져 있는 거리로 최신 메시지 버튼을 띄울지 정한다
pub fn should_show_jump_to_latest(distance_from_bottom: f32) -> bool {
    distance_from_bottom > NEAR_BOTTOM
}

pub struct InlineAiChatView {
    pub(super) task_id: String,
    pub(super) translator: Translator,
    pub(super) state: ChatHistoryState,
    pub(super) open_disclosures: HashSet<String>,
    pub(super) copied_messages: HashSet<String>,
    pub(super) file_preview: Option<FilePreviewState>,
    pub(super) focus_handle: FocusHandle,
    /// 작업 상세의 dock 패널이 대화 위에 떠 있는지. HTML 미리보기 WebView는 네이티브 창이라 그동안 숨긴다
    pub(super) is_covered_by_panel: bool,
    /// 지난 프레임에서 잰 화면 폭(TS 컨테이너 쿼리 `@container`)
    container_width: Option<f32>,
    search: Entity<InputState>,
    messages_scroll: ScrollHandle,
    should_scroll_to_latest: bool,
    /// 이전 메시지를 붙이기 전의 (스크롤 위치, 최대 스크롤). 붙인 뒤 읽던 위치를 되돌린다
    pending_scroll_anchor: Option<(f32, f32)>,
    /// 맨 아래로 내린 다음 프레임에 최신 기준점 위치로 맞춘다
    is_aligning_latest_anchor: bool,
    /// 최신 메시지 버튼. TS처럼 스크롤이 움직였을 때만 다시 판정한다(내용만 커지면 그대로)
    is_jump_to_latest_visible: bool,
    last_scroll_offset: Option<f32>,
    /// 성능 표식(`--perf-markers`)이 기다리는 첫 그리기. `ready`는 세션 목록, `rows`는 고른 대화의 메시지다
    paint_marks: ScreenPaintMarks,
    _history: Option<Task<()>>,
    _detail: Option<Task<()>>,
    _search_events: Subscription,
}

impl InlineAiChatView {
    pub fn new(
        task_id: String,
        translator: Translator,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.set_global(MessageCopyLabels::from_translator(&translator));
        let placeholder = translator.translate("aiSessions.searchPlaceholder");
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let search_events = cx.subscribe(&search, |view, input, event, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let query = input.read(cx).value().to_string();
                view.reload_history(&query, cx);
            }
        });
        let mut view = Self {
            task_id,
            translator,
            state: ChatHistoryState::default(),
            open_disclosures: HashSet::new(),
            copied_messages: HashSet::new(),
            file_preview: None,
            focus_handle: cx.focus_handle(),
            is_covered_by_panel: false,
            container_width: None,
            search,
            messages_scroll: ScrollHandle::new(),
            should_scroll_to_latest: false,
            pending_scroll_anchor: None,
            is_aligning_latest_anchor: false,
            is_jump_to_latest_visible: false,
            last_scroll_offset: None,
            paint_marks: ScreenPaintMarks::default(),
            _history: None,
            _detail: None,
            _search_events: search_events,
        };
        view.paint_marks.arm(PERF_SCREEN, "open");
        view.reload_history("", cx);
        view
    }

    fn reload_history(&mut self, query: &str, cx: &mut Context<Self>) {
        let request = self.state.begin_history_load(query);
        self.send_history_request(request, cx);
    }

    fn send_history_request(&mut self, request: HistoryRequest, cx: &mut Context<Self>) {
        let read = load_session_list(&self.task_id, request.list.clone(), cx);
        self._history = Some(cx.spawn(async move |this, cx| {
            let result = read.await.map_err(|_| ());
            let _ = this.update(cx, |view, cx| {
                view.state.finish_history_load(&request, result);
                if view.state.selected_key.is_none() {
                    view.file_preview = None;
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn send_detail_request(&mut self, request: Option<DetailRequest>, cx: &mut Context<Self>) {
        let Some(request) = request else {
            cx.notify();
            return;
        };
        if !request.is_older {
            self.should_scroll_to_latest = true;
            self.is_jump_to_latest_visible = false;
            self.pending_scroll_anchor = None;
        }
        let read = load_session_detail(&self.task_id, request.query.clone(), cx);
        self._detail = Some(cx.spawn(async move |this, cx| {
            let result = read.await.map_err(|_| ());
            let _ = this.update(cx, |view, cx| {
                view.state.finish_detail_load(&request, result);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_session(&mut self, key: String, cx: &mut Context<Self>) {
        self.paint_marks.arm(PERF_SCREEN, "select");
        self.file_preview = None;
        let request = self.state.select_session(key);
        self.send_detail_request(request, cx);
    }

    fn toggle_role(&mut self, role: AiMessageRole, cx: &mut Context<Self>) {
        let request = self.state.toggle_role(role);
        self.send_detail_request(request, cx);
    }

    fn toggle_provider(&mut self, provider: AiSessionProvider, cx: &mut Context<Self>) {
        self.state.toggle_provider(provider);
        cx.notify();
    }

    fn load_more_sessions(&mut self, cx: &mut Context<Self>) {
        if let Some(request) = self.state.begin_more_sessions() {
            self.send_history_request(request, cx);
        }
    }

    fn load_older_messages(&mut self, cx: &mut Context<Self>) {
        let request = self.state.begin_older_messages();
        if request.is_some() {
            let offset = f32::from(self.messages_scroll.offset().y);
            let max = f32::from(self.messages_scroll.max_offset().y);
            self.pending_scroll_anchor = Some((offset, max));
        }
        self.send_detail_request(request, cx);
    }

    fn retry_detail(&mut self, cx: &mut Context<Self>) {
        let request = self.state.begin_detail_load();
        self.send_detail_request(request, cx);
    }

    pub(super) fn toggle_disclosure(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.open_disclosures.remove(key) {
            self.open_disclosures.insert(key.to_string());
        }
        cx.notify();
    }

    fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        self.scroll_to_latest_anchor();
        self.is_jump_to_latest_visible = false;
        cx.notify();
    }

    fn scroll_to_latest_anchor(&mut self) {
        self.messages_scroll.scroll_to_bottom();
        self.is_aligning_latest_anchor = true;
    }

    /// 지난 프레임의 배치로 스크롤을 맞춘다. 첫 쪽은 맨 아래로, 이전 쪽을 붙였으면 읽던 위치로
    fn settle_scroll(&mut self, window: &mut Window) {
        if self.is_aligning_latest_anchor {
            let mut position = self.messages_scroll.offset();
            position.y = (position.y + px(LATEST_ANCHOR_INSET)).min(px(0.0));
            self.messages_scroll.set_offset(position);
            self.is_aligning_latest_anchor = false;
        }
        if self.should_scroll_to_latest
            && !self.state.is_detail_loading()
            && !self.state.messages().is_empty()
        {
            self.scroll_to_latest_anchor();
            self.should_scroll_to_latest = false;
            window.request_animation_frame();
        }
        self.observe_scroll();
        let Some((offset, max)) = self.pending_scroll_anchor else {
            return;
        };
        let current_max = f32::from(self.messages_scroll.max_offset().y);
        if !self.state.is_older_loading && current_max != max {
            let mut position = self.messages_scroll.offset();
            position.y = px(offset - (current_max - max));
            self.messages_scroll.set_offset(position);
            self.pending_scroll_anchor = None;
        }
    }

    /// TS `onScroll`: 위치가 바뀐 프레임에서만 맨 아래와의 거리를 다시 잰다
    fn observe_scroll(&mut self) {
        let offset = f32::from(self.messages_scroll.offset().y);
        if self.last_scroll_offset == Some(offset) {
            return;
        }
        self.last_scroll_offset = Some(offset);
        let distance = f32::from(self.messages_scroll.max_offset().y) + offset;
        self.is_jump_to_latest_visible = should_show_jump_to_latest(distance);
    }
}

impl Render for InlineAiChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        if self.paint_marks.is_pending() {
            let has_rows = !self.state.messages().is_empty();
            let has_sessions = !self.state.filtered_sessions().is_empty();
            self.paint_marks
                .observe(PERF_SCREEN, window, has_rows, has_sessions);
        }
        self.settle_scroll(window);
        self.prepare_document_preview(window, cx);
        let header = self.render_header(&theme, cx).into_any_element();
        let width_probe = self.width_probe(cx).into_any_element();
        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::copy_selected_text))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .flex_1()
            .overflow_hidden()
            .bg(theme.background.page)
            .child(width_probe)
            .child(header)
            .child(
                div()
                    .flex()
                    .min_h_0()
                    .flex_1()
                    .children(self.render_history_sidebar(&theme, cx))
                    .children(self.render_conversation(&theme, cx)),
            )
    }
}

impl InlineAiChatView {
    /// 화면 폭을 재서 좁은 배치로 바뀔 때만 다시 그린다
    fn width_probe(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        canvas(
            move |bounds, _, cx| {
                let width = f32::from(bounds.size.width);
                let _ = view.update(cx, |view, cx| {
                    let was_narrow = is_narrow_layout(view.container_width);
                    view.container_width = Some(width);
                    if was_narrow != is_narrow_layout(view.container_width) {
                        cx.notify();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    fn is_narrow(&self) -> bool {
        is_narrow_layout(self.container_width)
    }

    /// 대화 글자를 누르면 이 화면이 키 입력을 받아 `Mod+C`로 고른 글자를 복사할 수 있게 한다
    pub(super) fn focus_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
    }

    fn copy_selected_text(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let platform = cx.global::<AppServices>().shortcut_platform;
        let shortcut = shortcut_event_from_keystroke(&event.keystroke);
        if !self.focus_handle.is_focused(window)
            || !match_shortcut_event(&shortcut, COPY_SELECTION_SHORTCUT, platform)
        {
            return;
        }
        let text = TextSelection::selected_text(window, cx);
        if text.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        cx.stop_propagation();
    }
}

impl InlineAiChatView {
    /// `AI 채팅` 제목과 새로고침
    fn render_header(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> impl IntoElement {
        let refresh_label = self.translator.translate("aiSessions.refresh");
        let applied_query = self.state.applied_query.clone().unwrap_or_default();
        let refresh = text_button(
            theme,
            "ai-chat-refresh",
            refresh_label,
            self.state.is_history_loading,
        )
        .py(spacing(1.5))
        .on_click(cx.listener(move |view, _, _, cx| view.reload_history(&applied_query, cx)));
        let title = self.translator.translate("aiSessions.inlineChat");
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap(spacing(3.0))
            .border_b_1()
            .border_color(theme.border.subtle)
            .px(spacing(5.0))
            .py(spacing(3.0))
            .child(
                div()
                    .relative()
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(title.clone())
                    .child(qa_target_layer(QaTargetDescription {
                        role: "heading",
                        name: title.clone().into(),
                        text: title.into(),
                    })),
            )
            .child(refresh)
    }
}

impl InlineAiChatView {
    fn render_history_sidebar(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let is_narrow = self.is_narrow();
        if is_narrow && self.state.selected_session().is_some() {
            return None;
        }
        let sidebar = div().flex().flex_col().flex_none();
        let sidebar = match is_narrow {
            true => sidebar.w_full(),
            false => sidebar.w(px(288.0)),
        };
        let element = sidebar
            .border_r_1()
            .border_color(theme.border.subtle)
            .bg(fade(
                theme.background.surface,
                theme.background.surface.a * 0.35,
            ))
            .child(
                div()
                    .flex_none()
                    .px(spacing(4.0))
                    .pb(spacing(2.0))
                    .pt(spacing(4.0))
                    .child(self.render_history_heading(theme))
                    .child(self.render_search_form(theme, cx)),
            )
            .child(self.render_provider_filters(theme, cx))
            .child(
                div()
                    .id("ai-session-list")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px(spacing(2.0))
                    .pb(spacing(3.0))
                    .child(self.render_session_list(theme, cx)),
            );
        Some(element.into_any_element())
    }

    /// `대화 기록`과 세션 수(필터 중이면 `보이는 수/전체`)
    fn render_history_heading(&self, theme: &ThemeTokens) -> impl IntoElement {
        let count_label = self
            .state
            .history_count_label()
            .unwrap_or_else(|| self.translator.translate("aiSessions.noPreview"));
        div()
            .mb(spacing(3.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(spacing(2.0))
            .text_scale(text_scale::XS)
            .child(
                div()
                    .css_weight(600)
                    .text_color(theme.text.secondary)
                    .child(self.translator.translate("aiSessions.history")),
            )
            .child(div().text_color(theme.text.muted).child(count_label))
    }

    /// 검색어 입력. Enter나 `검색`으로 목록을 다시 읽는다
    fn render_search_form(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> impl IntoElement {
        let search_label = self.translator.translate("aiSessions.search");
        div()
            .flex()
            .items_center()
            .gap(spacing(1.5))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .rounded(radius::MD)
                    .border_1()
                    .border_color(theme.border.default)
                    .bg(theme.background.page)
                    .px(spacing(3.0))
                    .py(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.primary)
                    .child(Input::new(&self.search)),
            )
            .child(
                text_button(
                    theme,
                    "ai-chat-search",
                    search_label,
                    self.state.is_history_loading,
                )
                .py(spacing(2.0))
                .on_click(cx.listener(|view, _, _, cx| {
                    let query = view.search.read(cx).value().to_string();
                    view.reload_history(&query, cx);
                })),
            )
    }

    fn render_provider_filters(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_none()
            .gap(spacing(1.0))
            .px(spacing(3.0))
            .pb(spacing(2.0))
            .children(
                self.state
                    .provider_filters()
                    .into_iter()
                    .map(|(provider, count)| {
                        let is_active = self.state.active_providers.contains(&provider);
                        let name = self.translator.translate_with(
                            "aiSessions.providerFilterLabel",
                            &[("provider", provider.label())],
                        );
                        let hover = theme.background.surface;
                        let button = div()
                            .id(SharedString::from(format!(
                                "ai-session-filter-{}",
                                provider.id()
                            )))
                            .relative()
                            .flex()
                            .flex_none()
                            .min_h(px(36.0))
                            .items_center()
                            .gap(spacing(1.5))
                            .rounded(radius::MD)
                            .px(spacing(2.0))
                            .text_scale(text_scale::XS)
                            .cursor_pointer()
                            .child(ai_provider_icon(provider, 16.0))
                            .child(count.to_string())
                            .child(qa_target_layer(QaTargetDescription {
                                role: "button",
                                name: name.into(),
                                text: count.to_string().into(),
                            }))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.toggle_provider(provider, cx)
                            }));
                        match is_active {
                            true => button.bg(theme.brand.subtle).text_color(theme.text.brand),
                            false => button
                                .text_color(theme.text.secondary)
                                .hover(move |style| style.bg(hover)),
                        }
                    }),
            )
            .into_any_element()
    }

    fn render_session_list(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().flex().flex_col().gap(spacing(1.0));
        if self.state.history_error {
            list = list.child(inline_empty(
                theme,
                self.translator.translate("aiSessions.detailError"),
                true,
            ));
        }
        if self.state.is_history_loading {
            list = list.child(inline_empty(
                theme,
                self.translator.translate("aiSessions.loadingDetail"),
                true,
            ));
        }
        let Some(history) = self.state.history.as_ref() else {
            return list.into_any_element();
        };
        let sessions = self.state.filtered_sessions();
        let is_empty = sessions.is_empty();
        for session in sessions {
            list = list.child(self.render_session_row(theme, session, cx));
        }
        if is_empty && !self.state.is_history_loading {
            let key = if history.is_remote {
                "aiSessions.remoteBadge"
            } else {
                "aiSessions.noPreview"
            };
            list = list.child(inline_empty(theme, self.translator.translate(key), true));
        }
        if history.next_cursor.is_some() {
            let key = if self.state.is_history_loading {
                "aiSessions.loadingDetail"
            } else {
                "aiSessions.loadMoreSessions"
            };
            list = list.child(
                text_button(
                    theme,
                    "ai-chat-more-sessions",
                    self.translator.translate(key),
                    self.state.is_history_loading,
                )
                .w_full()
                .justify_center()
                .px(spacing(3.0))
                .py(spacing(3.0))
                .on_click(cx.listener(|view, _, _, cx| view.load_more_sessions(cx))),
            );
        }
        list.into_any_element()
    }

    fn render_session_row(
        &self,
        theme: &ThemeTokens,
        session: &AggregatedAiSession,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = session_key(session);
        let title = session_title(session);
        let is_selected = self.state.selected_key.as_deref() == Some(key.as_str());
        let hover = fade(theme.background.surface, theme.background.surface.a * 0.75);
        let accessible_name = format!("{} {title}", session.provider.label());
        let row = div()
            .id(SharedString::from(format!("ai-session-{key}")))
            .relative()
            .w_full()
            .rounded(radius::LG)
            .px(spacing(3.0))
            .py(spacing(3.0))
            .cursor_pointer()
            .child(render_session_title(theme, session, title.clone()))
            .children(session_preview(session).map(|preview| {
                div()
                    .mt(spacing(1.5))
                    .line_clamp(2)
                    .text_scale(text_scale::XS)
                    .line_height(px(20.0))
                    .text_color(theme.text.secondary)
                    .child(preview)
            }))
            .child(self.render_session_footer(theme, session))
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: accessible_name.into(),
                text: title.into(),
            }))
            .on_click(cx.listener(move |view, _, _, cx| view.select_session(key.clone(), cx)));
        match is_selected {
            true => row.bg(theme.background.surface),
            false => row.hover(move |style| style.bg(hover)),
        }
        .into_any_element()
    }

    /// 메시지 수와 마지막 갱신 날짜(`toLocaleDateString`)
    fn render_session_footer(
        &self,
        theme: &ThemeTokens,
        session: &AggregatedAiSession,
    ) -> impl IntoElement {
        let message_count = self.translator.translate_with(
            "aiSessions.messageCount",
            &[("count", &session.message_count.to_string())],
        );
        let date = session
            .updated_at
            .as_deref()
            .filter(|value| crate::qa::options::parse_iso_utc_ms(value).is_some())
            .map(format_date);
        div()
            .mt(spacing(2.0))
            .flex()
            .justify_between()
            .gap(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.muted)
            .child(message_count)
            .children(date)
    }

    fn render_conversation(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let selected = self.state.selected_session().cloned();
        if self.is_narrow() && selected.is_none() {
            return None;
        }
        let mut pane = div().relative().flex().flex_col().min_w_0().flex_1();
        if let Some(session) = selected.as_ref() {
            pane = pane
                .child(self.render_selected_header(theme, session, cx))
                .child(self.render_role_filters(theme, cx));
        }
        pane = pane.child(self.render_messages(theme, selected.as_ref(), cx));
        if selected.is_some() {
            pane = pane.child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(theme.border.subtle)
                    .px(spacing(4.0))
                    .py(spacing(3.0))
                    .text_center()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.secondary)
                    .child(self.translator.translate("aiSessions.readOnlyHistory")),
            );
            pane = pane.children(self.render_file_preview(theme, cx));
            pane = pane.children(self.render_jump_to_latest(theme, cx));
        }
        Some(pane.into_any_element())
    }

    /// 좁은 화면에서만 보이는 `←`(TS `aiSessions.backToHistory`). 누르면 대화를 닫고 목록으로 돌아간다
    fn render_back_to_history(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_narrow() {
            return None;
        }
        let label = self.translator.translate("aiSessions.backToHistory");
        let hover = theme.background.surface;
        let button = div()
            .id("ai-chat-back-to-history")
            .relative()
            .flex_none()
            .rounded(radius::MD)
            .px(spacing(2.0))
            .py(spacing(1.5))
            .text_scale(text_scale::SM)
            .text_color(theme.text.secondary)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .child("←")
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.into(),
                text: "←".into(),
            }))
            .on_click(cx.listener(|view, _, _, cx| {
                view.file_preview = None;
                view.state.clear_selection();
                cx.notify();
            }));
        Some(button.into_any_element())
    }

    fn render_selected_header(
        &self,
        theme: &ThemeTokens,
        session: &AggregatedAiSession,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = match session.provider {
            AiSessionProvider::Claude => theme.tag.claude,
            AiSessionProvider::Gemini => theme.tag.gemini,
            AiSessionProvider::Codex => theme.tag.codex,
            _ => theme.tag.neutral,
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing(3.0))
            .border_b_1()
            .border_color(theme.border.subtle)
            .px(spacing(4.0))
            .py(spacing(3.0))
            .children(self.render_back_to_history(theme, cx))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_scale(text_scale::SM)
                    .css_weight(500)
                    .text_color(theme.text.primary)
                    .child(session_title(session)),
            )
            .child(
                div()
                    .flex_none()
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(fade(colors.text, colors.text.a * 0.3))
                    .bg(colors.background)
                    .px(spacing(2.0))
                    .py(spacing(0.5))
                    .text_size(px(10.0))
                    .line_height(px(15.0))
                    .css_weight(600)
                    .text_color(colors.text)
                    .child(session.provider.label()),
            )
            .into_any_element()
    }

    fn render_role_filters(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_none()
            .gap(spacing(1.0))
            .border_b_1()
            .border_color(theme.border.subtle)
            .px(spacing(4.0))
            .py(spacing(2.0))
            .children(ROLE_FILTERS.into_iter().map(|role| {
                let label = role_label(&self.translator, role);
                let is_active = self.state.active_roles.contains(&role);
                let button = text_button(
                    theme,
                    SharedString::from(format!("ai-role-{label}")),
                    label,
                    false,
                )
                .py(spacing(1.5))
                .on_click(cx.listener(move |view, _, _, cx| view.toggle_role(role, cx)));
                match is_active {
                    true => button.bg(theme.brand.subtle).text_color(theme.text.brand),
                    false => button,
                }
            }))
            .into_any_element()
    }

    fn render_messages(
        &self,
        theme: &ThemeTokens,
        selected: Option<&AggregatedAiSession>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let content = div()
            .px(spacing(5.0))
            .py(spacing(6.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| view.focus_conversation(window, cx)),
            )
            .child(self.render_message_column(theme, selected, cx));
        // `overflow-y-auto`: 넘칠 때만 스크롤바 홈(15px)이 생겨 가운데 열이 그만큼 왼쪽으로 옮겨 간다
        let is_overflowing = f32::from(self.messages_scroll.max_offset().y) > 0.0;
        let frame = div().min_h_0().flex_1();
        match is_overflowing {
            true => frame
                .child(scroll_area(
                    "ai-session-messages",
                    &self.messages_scroll,
                    theme.border.strong.into(),
                    content,
                ))
                .into_any_element(),
            false => frame
                .child(
                    div()
                        .id("ai-session-messages")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.messages_scroll)
                        .child(content),
                )
                .into_any_element(),
        }
    }

    /// 가운데 열(`mx-auto w-full max-w-3xl space-y-2.5`)
    fn render_message_column(
        &self,
        theme: &ThemeTokens,
        selected: Option<&AggregatedAiSession>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let mut column = div()
            .mx_auto()
            .w_full()
            .max_w(px(768.0))
            .flex()
            .flex_col()
            .gap(spacing(2.5));
        match selected {
            None if self.state.history.is_some() => {
                column = column.child(inline_empty(
                    theme,
                    self.translator.translate("aiSessions.selectSession"),
                    false,
                ));
            }
            None => {}
            Some(session) => column = self.render_selected_messages(theme, session, column, cx),
        }
        column
    }

    fn render_selected_messages(
        &self,
        theme: &ThemeTokens,
        session: &AggregatedAiSession,
        mut column: gpui_kit::Div,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        if self.state.is_detail_loading() {
            return column.child(inline_empty(
                theme,
                self.translator.translate("aiSessions.loadingDetail"),
                false,
            ));
        }
        if self.state.has_detail_error() {
            return column.child(self.render_detail_error(theme, cx));
        }
        let messages = self.state.messages();
        if messages.is_empty() {
            column = column.child(inline_empty(
                theme,
                self.translator.translate("aiSessions.noPreview"),
                false,
            ));
        }
        if self.state.has_older_messages() {
            column = column.child(self.render_older_messages_button(theme, cx));
        }
        let session_key_text = session_key(session);
        for row in transcript_rows(messages, Some(&session_key_text)) {
            column = column.child(self.render_transcript_row(theme, &row, session.provider, cx));
        }
        column
    }

    /// 상세 조회 실패 안내와 다시 시도
    fn render_detail_error(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> impl IntoElement {
        let retry = text_button(
            theme,
            "ai-chat-detail-retry",
            self.translator.translate("aiSessions.retry"),
            false,
        )
        .px(spacing(3.0))
        .py(spacing(2.0))
        .text_color(theme.text.brand)
        .on_click(cx.listener(|view, _, _, cx| view.retry_detail(cx)));
        div()
            .child(inline_empty(
                theme,
                self.translator.translate("aiSessions.detailError"),
                false,
            ))
            .child(div().flex().justify_center().child(retry))
    }

    fn render_older_messages_button(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = if self.state.is_older_loading {
            "aiSessions.loadingDetail"
        } else {
            "aiSessions.loadOlderMessages"
        };
        let older = text_button(
            theme,
            "ai-chat-older",
            self.translator.translate(key),
            self.state.is_older_loading,
        )
        .border_1()
        .border_color(theme.border.subtle)
        .px(spacing(4.0))
        .py(spacing(2.0))
        .on_click(cx.listener(|view, _, _, cx| view.load_older_messages(cx)));
        div().flex().justify_center().child(older)
    }

    fn render_jump_to_latest(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_jump_to_latest_visible {
            return None;
        }
        let label = format!("{} ↓", self.translator.translate("aiSessions.jumpToLatest"));
        let button = div()
            .id("ai-chat-jump-to-latest")
            .relative()
            .rounded_full()
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .px(spacing(4.0))
            .py(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.primary)
            .shadow(box_shadows(&theme.shadow.md))
            .cursor_pointer()
            .child(label.clone())
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.clone().into(),
                text: label.into(),
            }))
            .on_click(cx.listener(|view, _, _, cx| view.jump_to_latest(cx)));
        Some(
            div()
                .absolute()
                .bottom(px(64.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(button)
                .into_any_element(),
        )
    }
}

/// 세션 행 머리: provider 아이콘과 한 줄로 자른 제목
fn render_session_title(
    theme: &ThemeTokens,
    session: &AggregatedAiSession,
    title: String,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(spacing(2.0))
        .child(
            div()
                .size(px(20.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(ai_provider_icon(session.provider, 16.0)),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .text_scale(text_scale::SM)
                .css_weight(500)
                .text_color(theme.text.primary)
                .child(title),
        )
}

/// `rounded-md px-2 text-xs text-text-secondary hover:bg-bg-surface disabled:opacity-50`
fn text_button(
    theme: &ThemeTokens,
    id: impl Into<gpui_kit::ElementId>,
    label: String,
    is_disabled: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let hover = theme.background.surface;
    let button = div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .rounded(radius::MD)
        .px(spacing(2.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.secondary)
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }));
    match is_disabled {
        true => button.opacity(0.5),
        false => button.cursor_pointer().hover(move |style| style.bg(hover)),
    }
}

/// TS `InlineAiChatEmpty`. 목록 쪽은 위아래 여백만, 대화 쪽은 가운데 정렬
fn inline_empty(theme: &ThemeTokens, text: String, is_compact: bool) -> AnyElement {
    let frame = div().flex().items_center().justify_center();
    let frame = if is_compact {
        frame.py(spacing(6.0))
    } else {
        frame.h(relative(1.0))
    };
    frame
        .child(
            div()
                .relative()
                .rounded(radius::MD)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.surface)
                .px(spacing(4.0))
                .py(spacing(3.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(text.clone())
                .child(qa_target_layer(QaTargetDescription {
                    role: "paragraph",
                    name: text.clone().into(),
                    text: text.into(),
                })),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};
    use gpui_kit::TestAppContext;
    use kanvibe_domain::i18n::Locale;

    fn open_chat_view(
        cx: &mut TestAppContext,
    ) -> (Entity<InlineAiChatView>, &mut gpui_kit::VisualTestContext) {
        let launch = LaunchOptions {
            data_dir: None,
            initial_route: "/ko".into(),
            window_size: DEFAULT_WINDOW_SIZE,
            qa: None,
        };
        cx.update(|cx| crate::init_app(&launch, cx).expect("app init"));
        let translator = Translator::new(Locale::Ko, "taskDetail");
        let (view, cx) = cx.add_window_view(|window, cx| {
            InlineAiChatView::new("task-1".into(), translator, window, cx)
        });
        cx.run_until_parked();
        (view, cx)
    }

    // parity: TS-renderer-TaskDetailRoute-74b0c3
    #[gpui_kit::test]
    fn opening_the_chat_view_loads_the_first_history_page_by_itself(cx: &mut TestAppContext) {
        let (view, cx) = open_chat_view(cx);
        view.read_with(cx, |view, _| {
            assert!(view.state.history.is_some());
            assert!(!view.state.is_history_loading);
            assert_eq!(view.state.history_count_label().as_deref(), Some("0"));
        });
    }

    // parity: TS-renderer-AiChatFilePreview-42fb0e
    #[gpui_kit::test]
    fn closing_the_file_preview_removes_it(cx: &mut TestAppContext) {
        let (view, cx) = open_chat_view(cx);
        view.update(cx, |view, cx| {
            view.open_file_preview("/repo/src/filters.ts".into(), cx)
        });
        view.read_with(cx, |view, _| assert!(view.file_preview.is_some()));
        view.update(cx, |view, cx| view.close_file_preview(cx));
        view.read_with(cx, |view, _| {
            assert!(view.file_preview.is_none());
            assert!(view.state.preview_file.is_none());
        });
    }

    #[test]
    fn narrow_layout_starts_below_720_px_and_waits_for_a_measurement() {
        assert!(!is_narrow_layout(None));
        assert!(is_narrow_layout(Some(719.0)));
        assert!(!is_narrow_layout(Some(720.0)));
    }

    // parity: TS-renderer-TaskDetailRoute-6f049a
    #[test]
    fn jump_to_latest_appears_only_when_far_from_the_bottom() {
        assert!(!should_show_jump_to_latest(0.0));
        assert!(!should_show_jump_to_latest(96.0));
        assert!(should_show_jump_to_latest(97.0));
    }
}
