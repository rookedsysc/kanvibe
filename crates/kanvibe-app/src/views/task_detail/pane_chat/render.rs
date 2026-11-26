//! pane 채팅 화면 그리기(TS `PaneChatView`의 JSX). 머리줄, 대화 영역(대화·연결 안내·현재 CLI 화면·보낸 말풍선·작업 중 표시),
//! 선택 카드, 입력창 순서로 쌓는다. 선택 화면이 떠 있는 동안 입력창은 지우지 않고 가려 쓰던 글을 남긴다.

use std::rc::Rc;

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, MouseButton, ParentElement, Render,
    ScrollEvent, StatefulInteractiveElement, Styled, Window, canvas, div, point, px,
};
use kanvibe_ai::message_instructions::is_ai_instruction_message;
use kanvibe_ai::pane_chat::snapshot::{PaneChatTranscriptStatus, get_pane_chat_prompt_dialog_key};

use super::super::ai_provider_icon::ai_provider_icon;
use super::view::{PERF_SCREEN, PaneChatView};
use super::visible_transcript::{VisibleTranscript, VisibleTranscriptInputs};
use super::widgets::view_button;
use crate::components::TailwindText;
use crate::qa::perf_markers::mark_screen_phase;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 대화가 아래 끝에서 이만큼 안이면 새 메시지를 따라 내려간다
const NEAR_BOTTOM: f32 = 96.0;
/// `max-w-3xl`
const CONTENT_MAX_WIDTH: f32 = 768.0;

/// "이전 메시지 더 보기"를 누를 때의 스크롤 위치(TS `scrollAnchor`). 앞쪽에 붙은 페이지의 높이만큼 내려 읽던 메시지를 제자리에 둔다
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct OlderPageScrollAnchor {
    scroll_top: f32,
    max_scroll_top: f32,
    is_page_applied: bool,
    is_page_laid_out: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum OlderPageScrollStep {
    WaitForPage,
    WaitForLayout,
    RestoreTo(f32),
}

impl OlderPageScrollAnchor {
    pub(super) fn new(scroll_top: f32, max_scroll_top: f32) -> Self {
        Self {
            scroll_top,
            max_scroll_top,
            is_page_applied: false,
            is_page_laid_out: false,
        }
    }

    pub(super) fn mark_page_applied(&mut self) {
        self.is_page_applied = true;
    }

    /// 그리기마다 부른다. 페이지를 반영한 첫 그리기는 아직 배치 전이라 높이를 모르므로 다음 프레임을 기다리고,
    /// 그다음 그리기에서 늘어난 높이만큼 내린 위치를 돌려준다
    pub(super) fn next_step(&mut self, current_max_scroll_top: f32) -> OlderPageScrollStep {
        if !self.is_page_applied {
            return OlderPageScrollStep::WaitForPage;
        }
        if !std::mem::replace(&mut self.is_page_laid_out, true) {
            return OlderPageScrollStep::WaitForLayout;
        }
        let added_height = current_max_scroll_top - self.max_scroll_top;
        OlderPageScrollStep::RestoreTo(self.scroll_top + added_height)
    }
}

/// 대화를 바닥에 붙일지 다시 볼 때 비교하는 내용(TS `useLayoutEffect`의 의존 값). 스크롤만으로는 바뀌지 않는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptFollowInputs {
    message_count: usize,
    /// 마지막 메시지의 시각과 글 길이. 이어 쓰이는 답이 길어져도 따라간다
    last_message: Option<(Option<String>, usize)>,
    pending_message_count: usize,
    is_agent_working: bool,
    target_key: String,
    is_visible: bool,
    prompt_dialog_key: Option<String>,
    /// 첫 그리기에 최신 메시지만 넣었다가 전부 그리면 높이가 바뀌므로 함께 본다
    is_first_paint_limited: bool,
}

impl PaneChatView {
    pub(super) fn t(&self, key: &str) -> String {
        self.translator.translate(&format!("paneChat.{key}"))
    }

    /// 지침 표시를 껐으면 시스템·자동 지침 메시지를 뺀 대화. 스냅샷·펼친 페이지·지침 표시가 그대로면 앞서 만든 것을 다시 쓴다
    pub(super) fn visible_transcript(&self, cx: &gpui_kit::App) -> Rc<VisibleTranscript> {
        let should_show_instructions = self.preferences.read(cx).should_show_instructions;
        let inputs = VisibleTranscriptInputs {
            snapshot_revision: self.session.snapshot_revision(),
            pages_revision: self.transcript.revision(),
            should_show_instructions,
        };
        self.visible_transcript_cache.get_or_build(inputs, || {
            let snapshot = self.session.snapshot();
            let mut messages = self.transcript.messages(snapshot);
            if !should_show_instructions {
                messages.retain(|message| !is_ai_instruction_message(message));
            }
            let session_key =
                snapshot.and_then(|snapshot| snapshot.transcript.session_id.as_deref());
            VisibleTranscript::new(messages, session_key)
        })
    }

    /// 아직 주고받은 메시지가 없어 첫 화면 안내를 입력창과 함께 가운데에 놓는 대화인지
    pub fn is_new_conversation(&self, cx: &gpui_kit::App) -> bool {
        let Some(snapshot) = self.session.snapshot() else {
            return false;
        };
        snapshot.can_send
            && snapshot.transcript.status == PaneChatTranscriptStatus::Available
            && self.visible_transcript(cx).newest_first.is_empty()
            && self.transcript.next_cursor(Some(snapshot)).is_none()
            && self.session.pending_messages().is_empty()
            && !self.session.is_agent_working()
    }

    /// 머리줄 둘째 줄: pane 이름 없이 CLI 이름과 pane 식별값 끝 8자
    pub fn header_subtitle(&self) -> String {
        let Some(snapshot) = self.session.snapshot() else {
            return self.t("loading");
        };
        let pane_suffix = snapshot.pane_id.as_deref().map(|pane_id| {
            let start = pane_id
                .char_indices()
                .rev()
                .nth(7)
                .map(|(index, _)| index)
                .unwrap_or(0);
            pane_id[start..].to_string()
        });
        let parts: Vec<String> = [
            snapshot
                .provider
                .map(|provider| provider.label().to_string()),
            pane_suffix,
        ]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect();
        match parts.is_empty() {
            true => self.t("loading"),
            false => parts.join(" · "),
        }
    }

    pub fn header_title(&self) -> String {
        self.task_title
            .clone()
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| self.t("currentPane"))
    }

    fn horizontal_padding(window: &Window) -> gpui_kit::Pixels {
        match f32::from(window.viewport_size().width) >= 640.0 {
            true => spacing(8.0),
            false => spacing(4.0),
        }
    }

    fn render_header(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let provider_icon = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.provider)
            .map(|provider| ai_provider_icon(provider, 18.0));
        let history = view_button(theme, "pane-chat-history", self.t("history"), false)
            .on_click(cx.listener(|view, _, _, cx| view.open_history(cx)));
        let terminal = view_button(theme, "pane-chat-terminal", self.t("terminal"), false)
            .on_click(cx.listener(|view, _, _, cx| view.open_terminal(cx)));
        div()
            .flex()
            .flex_none()
            .min_h(px(56.0))
            .flex_wrap()
            .items_center()
            .gap(spacing(2.0))
            .border_b_1()
            .border_color(theme.border.subtle)
            .px(spacing(4.0))
            .py(spacing(2.0))
            .children(provider_icon)
            .child(header_titles(
                theme,
                self.header_title(),
                self.header_subtitle(),
            ))
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .flex_none()
                    .gap(spacing(2.0))
                    .child(self.render_instructions_toggle(theme, cx))
                    .child(history)
                    .child(terminal),
            )
            .into_any_element()
    }

    /// "지침 표시" 체크 상자. 설정을 읽기 전과 저장하는 동안에는 바꿀 수 없다
    fn render_instructions_toggle(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preferences = self.preferences.read(cx);
        let is_checked = preferences.should_show_instructions;
        let is_disabled = !preferences.is_loaded || preferences.is_saving;
        let label = self.t("showInstructions");
        let box_mark = crate::views::settings::controls::native_checkbox(
            theme,
            "pane-chat-show-instructions-box",
            label.clone(),
            13.0,
            is_checked,
            is_disabled,
            |_, _, _| {},
        );
        let toggle = div()
            .id("pane-chat-show-instructions")
            .relative()
            .flex()
            .min_h(px(40.0))
            .items_center()
            .gap(spacing(2.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .px(spacing(3.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.primary)
            .child(box_mark)
            .child(aligned_text(label, text_scale::XS, TextFamily::Sans));
        match is_disabled {
            true => toggle.into_any_element(),
            false => toggle
                .cursor_pointer()
                .hover(|style| style.bg(theme.background.page))
                .on_click(cx.listener(|view, _, _, cx| view.toggle_show_instructions(cx)))
                .into_any_element(),
        }
    }

    fn render_alerts(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut alerts = Vec::new();
        if self.session.has_read_error {
            let retry = view_button(theme, "pane-chat-read-retry", self.t("retry"), false)
                .on_click(cx.listener(|view, _, window, cx| view.refresh(window, cx)));
            alerts.push(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(spacing(3.0))
                    .px(spacing(4.0))
                    .py(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.status.error)
                    .child(self.t("readError"))
                    .child(retry)
                    .into_any_element(),
            );
        }
        if self.preferences.read(cx).has_error {
            alerts.push(
                div()
                    .px(spacing(4.0))
                    .py(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.status.error)
                    .child(self.t("preferencesError"))
                    .into_any_element(),
            );
        }
        alerts
    }

    /// 대화가 최신 메시지를 따라가는지 스크롤 위치로 다시 정한다
    fn update_detachment(&mut self, cx: &mut Context<Self>) {
        let max = f32::from(self.transcript_scroll.max_offset().y);
        let offset = -f32::from(self.transcript_scroll.offset().y);
        let is_detached = max - offset >= NEAR_BOTTOM;
        if is_detached != self.is_detached {
            self.is_detached = is_detached;
            cx.notify();
        }
    }

    /// 성능 표식이 기다리는 첫 그리기(화면·대화 줄·보낼 수 있는 입력창)와 대상이 바뀐 뒤의 보낼 수 있는 화면을 알린다
    fn observe_paint_marks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.paint_marks.is_pending() && !self.is_retarget_mark_pending {
            return;
        }
        let can_send = self.composer.is_editable()
            && self
                .session
                .snapshot()
                .is_some_and(|snapshot| snapshot.can_send);
        let has_rows = !self.visible_transcript(cx).newest_first.is_empty();
        self.paint_marks
            .observe(PERF_SCREEN, window, has_rows, can_send);
        if can_send && std::mem::take(&mut self.is_retarget_mark_pending) {
            window.on_next_frame(|_, _| mark_screen_phase(PERF_SCREEN, "retarget"));
        }
    }

    fn transcript_follow_inputs(&self, cx: &gpui_kit::App) -> TranscriptFollowInputs {
        let transcript = self.visible_transcript(cx);
        let snapshot = self.session.snapshot();
        TranscriptFollowInputs {
            message_count: transcript.newest_first.len(),
            last_message: transcript
                .newest_first
                .last()
                .map(|row| (row.message.timestamp.clone(), row.message.text.len())),
            pending_message_count: self.session.pending_messages().len(),
            is_agent_working: self.session.is_agent_working(),
            target_key: self.target_key.clone(),
            is_visible: self.is_visible,
            prompt_dialog_key: snapshot
                .and_then(|snapshot| snapshot.prompt.as_ref())
                .map(get_pane_chat_prompt_dialog_key),
            is_first_paint_limited: self.is_first_paint_limited,
        }
    }

    /// 휠로 움직였으면 따라가기 여부를 다시 정하고, 대화 내용이 바뀐 그리기에서만 따라가는 중인 대화를 바닥으로 내린다.
    /// 스크롤이나 타이머가 일으킨 그리기에서 바닥으로 되돌리면 바닥 근처에서 천천히 올릴 수 없다
    fn follow_latest_on_content_change(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.needs_detach_check) {
            self.update_detachment(cx);
        }
        let follow_inputs = self.transcript_follow_inputs(cx);
        let has_content_changed =
            self.last_transcript_follow_inputs.as_ref() != Some(&follow_inputs);
        if has_content_changed && !self.is_detached {
            self.transcript_scroll.scroll_to_bottom();
        }
        self.last_transcript_follow_inputs = Some(follow_inputs);
    }

    /// 이전 페이지를 앞에 붙인 뒤, 늘어난 높이만큼 내려 누르기 전에 보던 메시지를 같은 자리에 둔다
    fn restore_older_page_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(anchor) = self.older_page_scroll_anchor.as_mut() else {
            return;
        };
        let current_max_scroll_top = f32::from(self.transcript_scroll.max_offset().y);
        match anchor.next_step(current_max_scroll_top) {
            OlderPageScrollStep::WaitForPage => {}
            OlderPageScrollStep::WaitForLayout => window.request_animation_frame(),
            OlderPageScrollStep::RestoreTo(scroll_top) => {
                self.older_page_scroll_anchor = None;
                let offset = self.transcript_scroll.offset();
                self.transcript_scroll
                    .set_offset(point(offset.x, px(-scroll_top)));
                self.update_detachment(cx);
            }
        }
    }

    pub fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        self.is_detached = false;
        self.transcript_scroll.scroll_to_bottom();
        cx.notify();
    }

    fn render_transcript_area(
        &mut self,
        theme: &ThemeTokens,
        padding: gpui_kit::Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_new = self.is_new_conversation(cx);
        self.restore_older_page_scroll(window, cx);
        self.follow_latest_on_content_change(cx);
        let blocks = self.render_body_blocks(theme, window, cx);
        let scroll = transcript_scroll(&self.transcript_scroll, padding, blocks, is_new).on_scroll(
            cx.listener(|view, _: &ScrollEvent, _, cx| {
                view.needs_detach_check = true;
                cx.notify();
            }),
        );
        let picker = self
            .render_model_picker(theme, window, cx)
            .map(|dialog| model_picker_slot(padding, dialog));
        let jump = self.is_detached.then(|| self.render_jump(theme, cx));
        div()
            .relative()
            .flex()
            .flex_col()
            .min_h_0()
            .flex_1()
            .child(scroll)
            .children(picker)
            .children(jump)
            .into_any_element()
    }

    fn render_jump(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let label = format!("{} ↓", self.t("jumpToLatest"));
        div()
            .absolute()
            .bottom(spacing(3.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("pane-chat-jump-latest")
                    .relative()
                    .rounded(radius::FULL)
                    .border_1()
                    .border_color(theme.border.default)
                    .bg(theme.background.surface)
                    .px(spacing(4.0))
                    .py(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.primary)
                    .cursor_pointer()
                    .child(label.clone())
                    .child(qa_target_layer(QaTargetDescription {
                        role: "button",
                        name: label.clone().into(),
                        text: label.into(),
                    }))
                    .on_click(cx.listener(|view, _, _, cx| view.jump_to_latest(cx))),
            )
            .into_any_element()
    }
}

impl Render for PaneChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        self.observe_paint_marks(window, cx);
        let padding = Self::horizontal_padding(window);
        let prompt = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.prompt.clone());
        let is_new = self.is_new_conversation(cx);
        let section_height = self.composer.section_height.clone();
        let composer = self.render_composer(&theme, padding, window, cx);
        let text_menu = self.render_text_menu(&theme, window, cx);
        div()
            .id("pane-chat")
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(Self::open_text_menu_outside_fields),
            )
            .relative()
            .flex()
            .flex_col()
            .min_h_0()
            .flex_1()
            .size_full()
            .bg(theme.background.page)
            .child(
                canvas(
                    move |bounds, _, _| section_height.set(f32::from(bounds.size.height)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(self.render_header(&theme, cx))
            .children(self.render_alerts(&theme, cx))
            .child(self.render_transcript_area(&theme, padding, window, cx))
            .children(
                prompt
                    .as_ref()
                    .map(|prompt| self.render_prompt_card(&theme, prompt, padding, cx)),
            )
            .children(prompt.is_none().then(|| {
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .min_h_0()
                    .max_h(gpui_kit::relative(0.7))
                    .child(composer)
            }))
            .children(is_new.then(|| div().flex_1()))
            .children(text_menu)
    }
}

/// 머리줄 왼쪽: 작업 제목과 CLI 이름·pane 식별값 줄
fn header_titles(theme: &ThemeTokens, title: String, subtitle: String) -> AnyElement {
    div()
        .min_w_0()
        .flex_1()
        .flex_basis(px(112.0))
        .child(
            div()
                .truncate()
                .text_scale(text_scale::SM)
                .css_weight(500)
                .text_color(theme.text.primary)
                .child(aligned_text(title, text_scale::SM, TextFamily::Sans)),
        )
        .child(
            div()
                .truncate()
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(aligned_text(subtitle, text_scale::XS, TextFamily::Sans)),
        )
        .into_any_element()
}

/// 대화 스크롤 영역. 빈 대화면 첫 화면 안내를 아래쪽(입력창 바로 위)에 붙인다
fn transcript_scroll(
    handle: &gpui_kit::ScrollHandle,
    padding: gpui_kit::Pixels,
    blocks: Vec<AnyElement>,
    is_new_conversation: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let scroll = div()
        .id("pane-chat-transcript")
        .track_scroll(handle)
        .min_h_0()
        .flex_1()
        .overflow_y_scroll()
        .px(padding)
        .py(spacing(6.0))
        .child(
            div()
                .mx_auto()
                .w_full()
                .max_w(px(CONTENT_MAX_WIDTH))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .children(blocks),
        );
    match is_new_conversation {
        true => scroll.flex().flex_col().justify_end(),
        false => scroll,
    }
}

/// 모델 팝오버 자리(`absolute inset-y-2 left-8 w-80`, 아래쪽 정렬)
fn model_picker_slot(padding: gpui_kit::Pixels, dialog: AnyElement) -> gpui_kit::Div {
    div()
        .absolute()
        .top(spacing(2.0))
        .bottom(spacing(2.0))
        .left(padding)
        .w(px(320.0))
        .flex()
        .items_end()
        .child(dialog)
}

#[cfg(test)]
mod tests {
    use super::{OlderPageScrollAnchor, OlderPageScrollStep};

    #[test]
    fn an_older_page_keeps_the_read_message_in_place_after_its_layout() {
        let mut anchor = OlderPageScrollAnchor::new(120.0, 400.0);

        assert_eq!(anchor.next_step(400.0), OlderPageScrollStep::WaitForPage);
        anchor.mark_page_applied();
        assert_eq!(anchor.next_step(400.0), OlderPageScrollStep::WaitForLayout);
        assert_eq!(
            anchor.next_step(1000.0),
            OlderPageScrollStep::RestoreTo(720.0)
        );
    }
}
