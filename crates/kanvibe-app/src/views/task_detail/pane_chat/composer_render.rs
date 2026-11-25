//! 입력창 그리기(TS `PaneChatComposer`의 JSX). 크기 조절 테두리, 첨부 목록, 자동 완성 메뉴, 입력란, 안내, 도구 줄 순서다.

use gpui_kit::base::input::{
    Escape, IndentInline, Input, InputEditorStyle, MoveDown, MoveUp, Paste, Textarea,
};
use gpui_kit::{
    AnyElement, Context, Div, ExternalPaths, Focusable, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, canvas, div, img, px,
};
use kanvibe_domain::pane_chat::PaneChatSuggestion;

use super::composer::{COMPOSER_MIN_HEIGHT, COMPOSER_RESIZE_STEP, CompletionKey, ComposerState};
use super::drafts::{AttachmentError, AttachmentStatus, DraftAttachment};
use super::suggestions::SuggestionResults;
use super::view::PaneChatView;
use super::widgets::PaneChatButton;
use crate::components::TailwindText;
use crate::qa::targets::{
    QaTargetBehavior, QaTargetDescription, qa_target_layer, qa_target_layer_with,
};
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 재생 상태 정의가 모델 버튼을 가리키는 TS DOM 선택자
pub const MODEL_TRIGGER_SELECTOR: &str = "[data-pane-chat-model-trigger]";

/// 도구 줄 버튼(`min-h-10 shrink-0 rounded-md px-2 text-xs text-text-secondary`)
fn tool_button(
    theme: &ThemeTokens,
    id: &'static str,
    label: String,
    is_disabled: bool,
) -> PaneChatButton {
    let element = div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .min_h(px(40.0))
        .items_center()
        .rounded(radius::MD)
        .px(spacing(2.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.secondary)
        .child(aligned_text(
            label.clone(),
            text_scale::XS,
            TextFamily::Sans,
        ))
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }));
    let element = match is_disabled {
        true => element.opacity(0.4),
        false => element.hover(|style| style.bg(theme.background.page)),
    };
    PaneChatButton::new(element, is_disabled)
}

impl PaneChatView {
    /// 입력창 높이를 바꾼다. 최소 높이와 대화 화면의 60% 사이로 붙인다
    pub fn resize_composer(&mut self, height: f32, window: &Window, cx: &mut Context<Self>) {
        let maximum = self.composer.maximum_height(window);
        self.composer.input_height = height.clamp(COMPOSER_MIN_HEIGHT, maximum);
        cx.notify();
    }

    /// 크기 조절 테두리에 포커스가 있을 때의 방향키·Home·End
    pub fn handle_resize_key(
        &mut self,
        key: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let current = self.composer.input_height;
        let next = match key {
            "up" => current + COMPOSER_RESIZE_STEP,
            "down" => current - COMPOSER_RESIZE_STEP,
            "home" => COMPOSER_MIN_HEIGHT,
            "end" => self.composer.maximum_height(window),
            _ => return false,
        };
        self.resize_composer(next, window, cx);
        true
    }

    fn render_resize_edge(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let label = self.t("resizeComposer");
        div()
            .id("pane-chat-composer-resize")
            .track_focus(&self.composer.separator_focus)
            .absolute()
            .top(px(-6.0))
            .left_0()
            .h(px(12.0))
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_row_resize()
            .child(
                div()
                    .h(px(4.0))
                    .w(px(32.0))
                    .rounded(radius::FULL)
                    .bg(theme.border.default),
            )
            .child(qa_target_layer(QaTargetDescription {
                role: "separator",
                name: label.into(),
                text: String::new().into(),
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    window.focus(&view.composer.separator_focus, cx);
                    view.composer.resize_drag =
                        Some((f32::from(event.position.y), view.composer.input_height));
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                if view.handle_resize_key(&event.keystroke.key, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .into_any_element()
    }

    /// 테두리를 끄는 동안의 마우스. 놓으면 멈춘다
    fn handle_resize_drag(
        &mut self,
        event: &MouseMoveEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some((start_y, start_height)) = self.composer.resize_drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.composer.resize_drag = None;
            return;
        }
        let height = start_height + start_y - f32::from(event.position.y);
        self.resize_composer(height, window, cx);
    }

    fn render_attachments(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let draft = self.composer.draft();
        if draft.attachments.is_empty() {
            return None;
        }
        let can_edit = ComposerState::can_edit(&self.session);
        let rows: Vec<AnyElement> = draft
            .attachments
            .iter()
            .map(|item| self.render_attachment(theme, item, can_edit, draft.is_sending, cx))
            .collect();
        Some(
            div()
                .flex()
                .flex_wrap()
                .gap(spacing(2.0))
                .px(spacing(2.0))
                .py(spacing(1.0))
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_attachment(
        &self,
        theme: &ThemeTokens,
        item: &DraftAttachment,
        can_edit: bool,
        is_sending: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .max_w_full()
            .items_center()
            .gap(spacing(2.0))
            .rounded(radius::LG)
            .bg(theme.background.page)
            .px(spacing(2.0))
            .py(spacing(1.0))
            .child(attachment_thumbnail(theme, item))
            .child(self.render_attachment_details(theme, item))
            .children(self.render_attachment_buttons(theme, item, can_edit, is_sending, cx))
            .into_any_element()
    }

    /// 이름, 크기(KB), 올리는 중·지우는 중 표시, 안내
    fn render_attachment_details(&self, theme: &ThemeTokens, item: &DraftAttachment) -> AnyElement {
        let status = match item.status {
            AttachmentStatus::Uploading => Some(self.t("uploading")),
            AttachmentStatus::Removing => Some(self.t("removingAttachment")),
            _ => None,
        };
        let error = self.attachment_error_text(item);
        let size_label = format!("{} KB", item.size_bytes.div_ceil(1024));
        div()
            .min_w_0()
            .text_scale(text_scale::XS)
            .child(
                div()
                    .max_w(px(176.0))
                    .truncate()
                    .text_color(theme.text.primary)
                    .child(item.name.clone()),
            )
            .child(div().text_color(theme.text.secondary).child(size_label))
            .children(status.map(|status| div().text_color(theme.text.secondary).child(status)))
            .children(error.map(|error| {
                div()
                    .max_w(px(224.0))
                    .text_color(theme.status.error)
                    .child(error)
            }))
            .into_any_element()
    }

    /// 실패한 첨부의 "다시 시도"(크기 초과는 빼고)와 제거 버튼
    fn render_attachment_buttons(
        &self,
        theme: &ThemeTokens,
        item: &DraftAttachment,
        can_edit: bool,
        is_sending: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let local_id = item.local_id;
        let can_retry = item.status == AttachmentStatus::Failed
            && item.error != Some(AttachmentError::TooLarge);
        let retry = can_retry.then(|| {
            tool_button(
                theme,
                "pane-chat-attachment-retry",
                self.t("retryAttachment"),
                !can_edit,
            )
            .on_click(
                cx.listener(move |view, _, window, cx| view.retry_attachment(local_id, window, cx)),
            )
            .into_any_element()
        });
        let is_remove_disabled =
            !can_edit || item.status == AttachmentStatus::Removing || is_sending;
        let remove = tool_button(
            theme,
            "pane-chat-attachment-remove",
            "×".to_string(),
            is_remove_disabled,
        )
        .on_click(
            cx.listener(move |view, _, window, cx| view.remove_attachment(local_id, window, cx)),
        )
        .into_any_element();
        retry.into_iter().chain(std::iter::once(remove)).collect()
    }

    fn render_suggestion_menu(
        &self,
        theme: &ThemeTokens,
        results: SuggestionResults,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.composer.suggestions.active_index;
        let options: Vec<AnyElement> = results
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| self.render_suggestion(theme, item, index, index == active, cx))
            .collect();
        div()
            .mb(spacing(2.0))
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.page)
            .p(spacing(1.0))
            .children(
                self.composer
                    .suggestions
                    .is_skill_picker_open
                    .then(|| self.render_skill_search(theme)),
            )
            .children(self.render_suggestion_notes(theme, &results, cx))
            .child(
                div()
                    .id("pane-chat-suggestions")
                    .max_h(px(192.0))
                    .overflow_y_scroll()
                    .children(options),
            )
            .children(
                results
                    .is_truncated
                    .then(|| menu_note(self.t("moreSuggestions"), theme.text.secondary)),
            )
            .into_any_element()
    }

    /// "스킬" 버튼으로 연 메뉴의 검색 입력란
    fn render_skill_search(&self, theme: &ThemeTokens) -> AnyElement {
        div()
            .mb(spacing(1.0))
            .min_h(px(44.0))
            .w_full()
            .flex()
            .items_center()
            .rounded(radius::MD)
            .bg(theme.background.surface)
            .px(spacing(3.0))
            .text_scale(text_scale::SM)
            .child(Input::new(&self.composer.skill_search))
            .into_any_element()
    }

    /// 메뉴 위 안내: 검색 중, 오류와 다시 시도, 결과 없음
    fn render_suggestion_notes(
        &self,
        theme: &ThemeTokens,
        results: &SuggestionResults,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut notes = Vec::new();
        if results.is_loading {
            notes.push(menu_note(
                self.t("loadingSuggestions"),
                theme.text.secondary,
            ));
        }
        if results.has_error {
            let retry = tool_button(
                theme,
                "pane-chat-suggestions-retry",
                self.t("retryAttachment"),
                false,
            )
            .on_click(cx.listener(|view, _, window, cx| view.retry_suggestions(window, cx)));
            notes.push(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(spacing(3.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.status.error)
                    .child(self.t("suggestionsError"))
                    .child(retry)
                    .into_any_element(),
            );
        }
        if !results.is_loading && !results.has_error && results.items.is_empty() {
            notes.push(menu_note(self.t("noSuggestions"), theme.text.secondary));
        }
        notes
    }

    fn render_suggestion(
        &self,
        theme: &ThemeTokens,
        item: &PaneChatSuggestion,
        index: usize,
        is_active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chosen = item.clone();
        let option = suggestion_row(theme, item)
            .on_mouse_move(cx.listener(move |view, _: &MouseMoveEvent, _, cx| {
                view.set_active_suggestion(index, cx)
            }))
            .on_click(
                cx.listener(move |view, _, window, cx| view.choose_suggestion(&chosen, window, cx)),
            );
        match is_active {
            true => option
                .bg(theme.background.surface)
                .text_color(theme.text.primary)
                .into_any_element(),
            false => option
                .text_color(theme.text.secondary)
                .hover(|style| style.bg(theme.background.surface))
                .into_any_element(),
        }
    }

    fn render_textarea(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let style = InputEditorStyle {
            foreground: theme.text.primary.into(),
            muted_foreground: theme.text.secondary.into(),
            background: theme.background.surface.into(),
            border: theme.border.default.into(),
            selection: theme.brand.muted.into(),
            caret: theme.text.primary.into(),
            ..InputEditorStyle::default()
        };
        self.composer
            .textarea
            .update(cx, |state, _| state.set_editor_style(style));
        let placeholder = self.t("placeholder");
        div()
            .relative()
            .w_full()
            .min_w_0()
            .h(px(self.composer.input_height))
            .px(spacing(2.0))
            .py(spacing(1.0))
            .text_scale(text_scale::SM)
            .line_height(px(24.0))
            .text_color(theme.text.primary)
            .child(Textarea::new(&self.composer.textarea))
            .child(qa_target_layer_with(
                QaTargetDescription {
                    role: "textbox",
                    name: placeholder.clone().into(),
                    text: self.composer.draft().text.into(),
                },
                QaTargetBehavior {
                    placeholder: Some(placeholder.into()),
                    ..QaTargetBehavior::default()
                },
            ))
            .into_any_element()
    }

    /// 입력창 아래 안내. 서비스·초안 오류는 빨갛게, 입력을 기다리는 안내는 흐리게 보인다
    fn render_composer_notes(&self, theme: &ThemeTokens) -> Vec<AnyElement> {
        let has_error =
            self.session.action_error.is_some() || self.composer.draft().error.is_some();
        self.composer_notes()
            .into_iter()
            .enumerate()
            .map(|(index, note)| {
                let color = if has_error && index == 0 {
                    theme.status.error
                } else {
                    theme.text.secondary
                };
                note_line(note, color)
            })
            .collect()
    }

    /// 모델 버튼의 글: 모델 이름(없으면 "모델 선택")과 추론 강도
    pub fn model_trigger_label(&self) -> String {
        let settings = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.model_settings.as_ref());
        let model = settings.and_then(|settings| {
            let current = settings.model.as_ref()?;
            let label = settings
                .models
                .iter()
                .find(|model| &model.id == current)
                .map(|model| model.label.clone());
            Some(label.unwrap_or_else(|| current.clone()))
        });
        let mut label = model.unwrap_or_else(|| self.t("model"));
        if let Some(effort) = settings.and_then(|settings| settings.effort.as_ref()) {
            label = format!("{label} · {effort}");
        }
        label
    }

    /// 모델 버튼을 누를 수 없는지. CLI 선택기를 여는 방식은 입력을 받을 수 있을 때만 연다
    pub fn is_model_trigger_disabled(&self) -> bool {
        let Some(snapshot) = self.session.snapshot() else {
            return true;
        };
        self.is_composer_disabled()
            || !snapshot.model_picker.is_supported
            || (!self.is_input_ready() && snapshot.model_picker.is_api_supported != Some(true))
    }

    /// 작업 중이고 보낼 내용이 없으면 전송 자리를 응답 중단으로 쓴다
    pub fn is_stop_in_place_of_send(&self) -> bool {
        let draft = self.composer.draft();
        self.session.is_agent_working()
            && draft.text.trim().is_empty()
            && draft.attachments.is_empty()
    }

    /// 보내기 버튼을 누를 수 없는지
    pub fn is_send_disabled(&self) -> bool {
        let draft = self.composer.draft();
        self.is_composer_disabled()
            || draft.is_sending
            || draft.has_unsettled_attachment()
            || !draft.has_content()
            || !self.is_input_ready()
    }

    fn render_model_trigger(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let label = self.model_trigger_label();
        let (model, effort) = match label.split_once(" · ") {
            Some((model, effort)) => (model.to_string(), Some(format!("· {effort}"))),
            None => (label.clone(), None),
        };
        let trigger = div()
            .id("pane-chat-model-trigger")
            .track_focus(&self.composer.model_trigger_focus)
            .relative()
            .flex()
            .flex_none()
            .max_w_full()
            .min_h(px(40.0))
            .items_center()
            .gap(px(6.0))
            .rounded(radius::MD)
            .px(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .child(
                div()
                    .truncate()
                    .child(aligned_text(model, text_scale::XS, TextFamily::Sans)),
            )
            .children(effort.map(|effort| aligned_text(effort, text_scale::XS, TextFamily::Sans)))
            .child(aligned_text("⌄", text_scale::XS, TextFamily::Sans))
            .children(model_trigger_layers(
                label,
                self.composer.model_trigger_bounds.clone(),
            ));
        match self.is_model_trigger_disabled() {
            true => trigger.opacity(0.4).into_any_element(),
            false => trigger
                .cursor_pointer()
                .hover(|style| style.bg(theme.background.page))
                .on_click(cx.listener(|view, _, window, cx| view.open_model_picker(window, cx)))
                .into_any_element(),
        }
    }

    /// 보내기 자리의 이름과 글자. 작업 중이고 보낼 내용이 없으면 응답 중단(사각형)이다
    fn send_slot_content(&self, is_stop: bool) -> (String, Option<&'static str>) {
        match (is_stop, self.composer.draft().is_sending) {
            (true, _) => (self.t("stopResponse"), None),
            (false, true) => (self.t("sending"), Some("…")),
            (false, false) => (self.t("send"), Some("↑")),
        }
    }

    fn render_send_slot(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let is_stop = self.is_stop_in_place_of_send();
        let is_disabled = match is_stop {
            true => self.is_composer_disabled(),
            false => self.is_send_disabled(),
        };
        let (label, glyph) = self.send_slot_content(is_stop);
        let button = div()
            .id("pane-chat-send")
            .relative()
            .ml_auto()
            .flex()
            .flex_none()
            .size(px(40.0))
            .items_center()
            .justify_center()
            .rounded(radius::FULL)
            .bg(theme.brand.primary)
            .text_color(gpui_kit::white())
            .child(send_glyph(glyph))
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.into(),
                text: String::new().into(),
            }));
        match is_disabled {
            true => button.opacity(0.4).into_any_element(),
            false => button
                .cursor_pointer()
                .hover(|style| style.bg(theme.brand.active))
                .on_click(cx.listener(move |view, _, window, cx| match is_stop {
                    true => view.interrupt(window, cx),
                    false => view.submit(window, cx),
                }))
                .into_any_element(),
        }
    }

    /// "파일"·"사진"·"스킬" 버튼. 대상이 확인되고 CLI가 그 기능을 받을 때만 누를 수 있다
    fn render_input_tools(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let can_edit = ComposerState::can_edit(&self.session);
        let capabilities = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.composer.as_ref());
        let can_attach = can_edit && capabilities.is_some_and(|composer| composer.can_attach);
        let can_use_skills =
            can_edit && capabilities.is_some_and(|composer| composer.can_use_skills);
        vec![
            tool_button(
                theme,
                "pane-chat-attach-files",
                self.t("attachFiles"),
                !can_attach,
            )
            .on_click(cx.listener(|view, _, window, cx| view.pick_attachments(window, cx)))
            .into_any_element(),
            tool_button(
                theme,
                "pane-chat-attach-photos",
                self.t("attachPhotos"),
                !can_attach,
            )
            .on_click(cx.listener(|view, _, window, cx| view.pick_attachments(window, cx)))
            .into_any_element(),
            tool_button(theme, "pane-chat-skills", self.t("skills"), !can_use_skills)
                .on_click(cx.listener(|view, _, window, cx| view.toggle_skill_picker(window, cx)))
                .into_any_element(),
        ]
    }

    fn render_tools(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let stop = tool_button(
            theme,
            "pane-chat-stop",
            self.t("stop"),
            self.is_composer_disabled(),
        )
        .on_click(cx.listener(|view, _, window, cx| view.interrupt(window, cx)));
        div()
            .mt(spacing(1.0))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(spacing(1.0))
            .children(self.render_input_tools(theme, cx))
            .child(self.render_model_trigger(theme, cx))
            .child(stop)
            .child(self.render_send_slot(theme, cx))
            .into_any_element()
    }

    /// 메뉴가 열려 있으면 방향키·Tab·Esc를 입력란보다 먼저 받는다
    fn capture_menu_key(
        &mut self,
        key: CompletionKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.handle_completion_key(key, window, cx) {
            cx.stop_propagation();
        }
    }

    pub(super) fn render_composer(
        &mut self,
        theme: &ThemeTokens,
        padding: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let menu = self
            .suggestion_results()
            .map(|results| self.render_suggestion_menu(theme, results, cx));
        let is_focused = self.composer.textarea.focus_handle(cx).is_focused(window);
        let border = match is_focused {
            true => theme.brand.primary,
            false => theme.border.default,
        };
        let composer = with_composer_keys(div().id("pane-chat-composer"), cx)
            .relative()
            .mx_auto()
            .w_full()
            .max_w(px(768.0))
            .rounded(px(12.0))
            .border_1()
            .border_color(border)
            .bg(theme.background.surface)
            .p(spacing(2.0))
            .child(self.render_resize_edge(theme, cx))
            .children(self.render_attachments(theme, cx))
            .children(menu)
            .child(self.render_textarea(theme, cx))
            .children(self.render_composer_notes(theme))
            .child(self.render_tools(theme, cx));
        div()
            .id("pane-chat-composer-form")
            .min_h_0()
            .overflow_y_scroll()
            .px(padding)
            .py(spacing(3.0))
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, window, cx| {
                view.handle_resize_drag(event, window, cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, _| view.composer.resize_drag = None),
            )
            .child(composer)
            .into_any_element()
    }
}

/// 메뉴가 열려 있을 때 방향키·Tab·Esc를 입력란보다 먼저 받고, 붙여넣은 이미지와 끌어 놓은 파일을 첨부로 받는다
fn with_composer_keys(element: Stateful<Div>, cx: &mut Context<PaneChatView>) -> Stateful<Div> {
    element
        .capture_action(cx.listener(|view, _: &MoveUp, window, cx| {
            view.capture_menu_key(CompletionKey::Up, window, cx)
        }))
        .capture_action(cx.listener(|view, _: &MoveDown, window, cx| {
            view.capture_menu_key(CompletionKey::Down, window, cx)
        }))
        .capture_action(cx.listener(|view, _: &IndentInline, window, cx| {
            view.capture_menu_key(CompletionKey::Tab, window, cx)
        }))
        .capture_action(cx.listener(|view, _: &Escape, window, cx| {
            view.capture_menu_key(CompletionKey::Escape, window, cx)
        }))
        .capture_action(cx.listener(|view, _: &Paste, window, cx| {
            if view.paste_clipboard_image(window, cx) {
                cx.stop_propagation();
            }
        }))
        .on_drop(
            cx.listener(|view, paths: &ExternalPaths, window, cx| {
                view.drop_paths(paths, window, cx)
            }),
        )
}

fn attachment_thumbnail(theme: &ThemeTokens, item: &DraftAttachment) -> AnyElement {
    match item.preview.clone() {
        Some(image) => img(image)
            .size(px(56.0))
            .rounded(radius::BASE)
            .into_any_element(),
        None => div()
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .child("↗")
            .into_any_element(),
    }
}

/// 메뉴 한 줄의 모양(이름과 설명)
fn suggestion_row(theme: &ThemeTokens, item: &PaneChatSuggestion) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!(
            "pane-chat-suggestion-{}",
            item.id
        )))
        .relative()
        .flex()
        .min_h(px(44.0))
        .w_full()
        .items_center()
        .gap(spacing(3.0))
        .rounded(radius::MD)
        .px(spacing(3.0))
        .cursor_pointer()
        .child(
            div()
                .flex_none()
                .text_scale(text_scale::SM)
                .child(item.label.clone()),
        )
        .child(
            div()
                .truncate()
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(item.description.clone()),
        )
        .child(qa_target_layer(QaTargetDescription {
            role: "option",
            name: item.label.clone().into(),
            text: item.label.clone().into(),
        }))
}

/// 모델 버튼 자리를 남기는 층들. 팝오버 바깥 누르기 판정과 QA 대상(이름·TS 선택자)이다
fn model_trigger_layers(
    label: String,
    bounds: std::rc::Rc<std::cell::Cell<Option<gpui_kit::Bounds<Pixels>>>>,
) -> Vec<AnyElement> {
    vec![
        canvas(
            move |element_bounds, _, _| bounds.set(Some(element_bounds)),
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
        .into_any_element(),
        qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        })
        .into_any_element(),
        qa_target_layer_with(
            QaTargetDescription {
                role: "button",
                name: String::new().into(),
                text: String::new().into(),
            },
            QaTargetBehavior {
                selector: Some(MODEL_TRIGGER_SELECTOR.into()),
                ..QaTargetBehavior::default()
            },
        )
        .into_any_element(),
    ]
}

/// 보내기(`↑`, 보내는 중 `…`) 또는 응답 중단 사각형
fn send_glyph(glyph: Option<&str>) -> AnyElement {
    match glyph {
        Some(glyph) => div()
            .text_scale(text_scale::BASE)
            .css_weight(500)
            .child(glyph.to_string())
            .into_any_element(),
        None => div()
            .size(px(12.0))
            .rounded(radius::SM)
            .bg(gpui_kit::white())
            .into_any_element(),
    }
}

fn menu_note(text: String, color: gpui_kit::Rgba) -> AnyElement {
    div()
        .px(spacing(3.0))
        .py(spacing(2.0))
        .text_scale(text_scale::XS)
        .text_color(color)
        .child(text)
        .into_any_element()
}

fn note_line(text: String, color: gpui_kit::Rgba) -> AnyElement {
    div()
        .px(spacing(2.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .text_color(color)
        .child(text)
        .into_any_element()
}
