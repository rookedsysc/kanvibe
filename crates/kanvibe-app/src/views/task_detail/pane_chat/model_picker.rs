//! 입력창 모델 버튼의 팝오버(TS `PaneChatModelPicker`). 스냅샷의 모델 설정을 곧바로 보이고 API로 새로 읽는다.
//! 고른 값은 서비스가 확인한 뒤에야 체크가 옮겨 가고, 바꾸는 중에는 조작을 잠그되 Esc는 팝오버가 받는다.

use gpui_kit::{
    AnyElement, Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_ai::pane_chat::model_settings::{
    PaneChatModelOption, PaneChatModelSettingChange, PaneChatModelSettings,
};

use super::view::PaneChatView;
use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

pub struct ModelPickerState {
    pub target_token: String,
    pub settings: Option<PaneChatModelSettings>,
    pub is_busy: bool,
    pub has_error: bool,
    /// 이 값이 바뀐 뒤 도착한 응답은 버린다
    generation: u64,
    is_changing: bool,
    pub focus_handle: FocusHandle,
    pub option_focus: Vec<FocusHandle>,
    /// 다음 그리기에서 포커스를 옮길지
    needs_focus: bool,
}

impl ModelPickerState {
    pub fn new(
        target_token: String,
        initial: Option<PaneChatModelSettings>,
        cx: &mut Context<PaneChatView>,
    ) -> Self {
        Self {
            target_token,
            settings: initial,
            is_busy: false,
            has_error: false,
            generation: 0,
            is_changing: false,
            focus_handle: cx.focus_handle(),
            option_focus: Vec::new(),
            needs_focus: true,
        }
    }

    fn current_model(&self) -> Option<&PaneChatModelOption> {
        let settings = self.settings.as_ref()?;
        settings
            .models
            .iter()
            .find(|model| Some(&model.id) == settings.model.as_ref())
    }
}

/// 사용자가 고른 바꿀 값과 그 값이 이미 지금 값인지
fn is_current_choice(
    settings: &PaneChatModelSettings,
    change: &PaneChatModelSettingChange,
) -> bool {
    match change {
        PaneChatModelSettingChange::Model { model } => settings.model.as_ref() == Some(model),
        PaneChatModelSettingChange::Effort { effort } => settings.effort.as_ref() == Some(effort),
        _ => false,
    }
}

impl PaneChatView {
    pub fn model_picker(&self) -> Option<&ModelPickerState> {
        self.model_picker.as_ref()
    }

    /// 열 때와 다시 시도할 때 모델 목록을 새로 읽는다
    pub fn load_model_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.model_picker.as_mut() else {
            return;
        };
        picker.generation += 1;
        picker.has_error = false;
        let generation = picker.generation;
        let token = picker.target_token.clone();
        let request = self
            .client
            .get_model_settings(&self.session.task_id, &token, cx);
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |view, cx| {
                let Some(picker) = view.model_picker.as_mut().filter(|picker| {
                    picker.generation == generation && picker.target_token == token
                }) else {
                    return;
                };
                match result {
                    Ok(settings) => {
                        // 처음 받은 목록이면 체크된 모델로 포커스를 옮긴다
                        picker.needs_focus |= picker.settings.is_none();
                        picker.settings = Some(settings);
                    }
                    Err(_) => picker.has_error = true,
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 고른 값을 CLI에 적용하고, 확인된 설정을 다시 읽은 뒤 대화 상태도 새로 읽는다
    pub fn change_model_setting(
        &mut self,
        change: PaneChatModelSettingChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self.model_picker.as_mut() else {
            return;
        };
        let is_current = picker
            .settings
            .as_ref()
            .is_some_and(|settings| is_current_choice(settings, &change));
        if picker.is_changing || is_current {
            return;
        }
        picker.generation += 1;
        picker.is_changing = true;
        picker.is_busy = true;
        picker.has_error = false;
        picker.needs_focus = true;
        let (generation, token) = (picker.generation, picker.target_token.clone());
        let task_id = self.session.task_id.clone();
        let request = self
            .client
            .change_model_setting(&task_id, &token, &change, cx);
        let client = self.client.clone();
        cx.spawn_in(window, async move |this, cx| {
            let changed = request.await;
            // 닫혔거나 다른 변경이 시작된 팝오버는 다시 읽지 않는다
            let is_current = this
                .read_with(cx, |view, _| view.is_model_picker_generation(generation))
                .unwrap_or(false);
            let reread = match (changed, is_current) {
                (Ok(_), true) => {
                    Some(cx.update(|_, cx| client.get_model_settings(&task_id, &token, cx)))
                }
                _ => None,
            };
            let settings = match reread {
                Some(Ok(task)) => Some(task.await),
                _ => None,
            };
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_model_change(generation, settings, window, cx)
            });
        })
        .detach();
        cx.notify();
    }

    fn is_model_picker_generation(&self, generation: u64) -> bool {
        self.model_picker
            .as_ref()
            .is_some_and(|picker| picker.generation == generation)
    }

    fn finish_model_change(
        &mut self,
        generation: u64,
        settings: Option<super::client::PaneChatResult<PaneChatModelSettings>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(picker) = self
            .model_picker
            .as_mut()
            .filter(|picker| picker.generation == generation)
        else {
            if let Some(picker) = self.model_picker.as_mut() {
                picker.is_changing = false;
            }
            return;
        };
        picker.is_changing = false;
        picker.is_busy = false;
        picker.needs_focus = true;
        let is_changed = matches!(settings, Some(Ok(_)));
        match settings {
            Some(Ok(settings)) => picker.settings = Some(settings),
            _ => picker.has_error = true,
        }
        if is_changed {
            self.refresh(window, cx);
        }
        cx.notify();
    }

    /// 팝오버를 닫는다. 키보드로 닫았으면 모델 버튼으로 포커스를 돌린다
    pub fn close_model_picker(
        &mut self,
        should_restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.model_picker.take().is_none() {
            return;
        }
        if should_restore_focus {
            window.focus(&self.composer.model_trigger_focus, cx);
        }
        cx.notify();
    }

    /// 방향키·Home·End로 모델 사이를 옮긴다. 고르기 전에는 바꾸지 않는다
    fn navigate_models(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(picker) = self.model_picker.as_ref().filter(|picker| !picker.is_busy) else {
            return false;
        };
        let count = picker.option_focus.len();
        if count == 0 {
            return false;
        }
        let index = picker
            .option_focus
            .iter()
            .position(|handle| handle.is_focused(window))
            .unwrap_or(0);
        let next = match event.keystroke.key.as_str() {
            "down" => (index + 1) % count,
            "up" => (index + count - 1) % count,
            "home" => 0,
            "end" => count - 1,
            _ => return false,
        };
        window.focus(&picker.option_focus[next], cx);
        true
    }

    /// 열린 뒤나 바꾸기가 끝난 뒤 한 번, 체크된 모델(없으면 첫 모델)에, 바꾸는 중이면 팝오버 자체에 포커스를 둔다
    fn place_model_picker_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self
            .model_picker
            .as_mut()
            .filter(|picker| picker.needs_focus)
        else {
            return;
        };
        picker.needs_focus = false;
        let Some(settings) = picker.settings.as_ref().filter(|_| !picker.is_busy) else {
            window.focus(&picker.focus_handle, cx);
            return;
        };
        while picker.option_focus.len() < settings.models.len() {
            picker.option_focus.push(cx.focus_handle());
        }
        let checked = settings
            .models
            .iter()
            .position(|model| Some(&model.id) == settings.model.as_ref())
            .unwrap_or(0);
        if let Some(handle) = picker.option_focus.get(checked) {
            window.focus(handle, cx);
        }
    }

    pub(super) fn render_model_picker(
        &mut self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.place_model_picker_focus(window, cx);
        let picker = self.model_picker.as_ref()?;
        let title = self.translator.translate("paneChat.model");
        let body = match picker.settings.as_ref() {
            None => vec![status_text(
                theme,
                self.translator.translate("paneChat.loading"),
            )],
            Some(settings) => self.render_model_settings(theme, picker, settings, cx),
        };
        let dialog = with_picker_events(dialog_frame(theme, &picker.focus_handle), cx)
            .child(self.render_model_header(theme, title, cx))
            .children(body)
            .child(model_footer(
                theme,
                self.translator.translate(match picker.is_busy {
                    true => "paneChat.savingModel",
                    false => "paneChat.modelAppliesNext",
                }),
            ))
            .children(
                picker
                    .has_error
                    .then(|| self.render_model_error(theme, picker.is_busy, cx)),
            );
        Some(dialog.into_any_element())
    }

    fn render_model_header(
        &self,
        theme: &ThemeTokens,
        title: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let close_label = self.translator.translate("paneChat.closeModel");
        div()
            .flex()
            .items_center()
            .justify_between()
            .bg(theme.background.surface)
            .px(spacing(4.0))
            .pb(spacing(1.0))
            .pt(spacing(3.0))
            .child(
                div()
                    .text_scale(text_scale::XS)
                    .css_weight(500)
                    .text_color(theme.text.secondary)
                    .child(title),
            )
            .child(
                div()
                    .id("pane-chat-model-close")
                    .relative()
                    .flex()
                    .size(px(32.0))
                    .items_center()
                    .justify_center()
                    .rounded(radius::MD)
                    .text_scale(text_scale::LG)
                    .text_color(theme.text.secondary)
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.background.page))
                    .child("×")
                    .child(qa_target_layer(QaTargetDescription {
                        role: "button",
                        name: close_label.into(),
                        text: "×".into(),
                    }))
                    .on_click(
                        cx.listener(|view, _, window, cx| {
                            view.close_model_picker(true, window, cx)
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_model_settings(
        &self,
        theme: &ThemeTokens,
        picker: &ModelPickerState,
        settings: &PaneChatModelSettings,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut menu = div().px(spacing(2.0)).pb(spacing(2.0));
        for (index, model) in settings.models.iter().enumerate() {
            let is_checked = Some(&model.id) == settings.model.as_ref();
            let focus = picker.option_focus.get(index).cloned();
            menu = menu.child(self.render_model_option(
                theme,
                model,
                is_checked,
                picker.is_busy,
                focus,
                cx,
            ));
        }
        let mut blocks = vec![menu.into_any_element()];
        if let Some(model) = picker.current_model() {
            if !model.effort_levels.is_empty() {
                blocks.push(self.render_effort_levels(theme, model, settings, picker.is_busy, cx));
            }
            if model.supports_fast {
                blocks.push(self.render_fast_mode(theme, settings, picker.is_busy, cx));
            }
        }
        blocks
    }

    fn render_model_option(
        &self,
        theme: &ThemeTokens,
        model: &PaneChatModelOption,
        is_checked: bool,
        is_busy: bool,
        focus: Option<FocusHandle>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = model.id.clone();
        let mut option = model_row(theme, model, is_checked);
        if let Some(focus) = focus {
            option = option.track_focus(&focus);
        }
        if is_busy {
            return option.opacity(0.5).into_any_element();
        }
        option
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, window, cx| {
                let change = PaneChatModelSettingChange::Model { model: id.clone() };
                view.change_model_setting(change, window, cx)
            }))
            .into_any_element()
    }

    fn render_effort_levels(
        &self,
        theme: &ThemeTokens,
        model: &PaneChatModelOption,
        settings: &PaneChatModelSettings,
        is_busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let buttons: Vec<AnyElement> = model
            .effort_levels
            .iter()
            .map(|level| {
                let is_selected = settings.effort.as_ref() == Some(&level.id);
                self.render_effort_button(theme, &level.id, &level.label, is_selected, is_busy, cx)
            })
            .collect();
        let default_label = self.translator.translate("paneChat.defaultEffort");
        let reset = labelled_button(theme, "pane-chat-effort-reset", default_label, is_busy)
            .on_click(cx.listener(|view, _, window, cx| {
                view.change_model_setting(PaneChatModelSettingChange::Reset, window, cx)
            }));
        div()
            .border_t_1()
            .border_color(theme.border.subtle)
            .px(spacing(4.0))
            .py(spacing(3.0))
            .child(
                section_heading(theme, self.translator.translate("paneChat.effort")).child(reset),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(spacing(1.0))
                    .rounded(radius::LG)
                    .bg(theme.background.page)
                    .p(spacing(1.0))
                    .children(buttons),
            )
            .into_any_element()
    }

    fn render_effort_button(
        &self,
        theme: &ThemeTokens,
        id: &str,
        label: &str,
        is_selected: bool,
        is_busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let effort = id.to_string();
        let button = effort_segment(theme, id, label, is_selected);
        if is_busy {
            return button.opacity(0.4).into_any_element();
        }
        button
            .cursor_pointer()
            .on_click(cx.listener(move |view, _, window, cx| {
                let change = PaneChatModelSettingChange::Effort {
                    effort: effort.clone(),
                };
                view.change_model_setting(change, window, cx)
            }))
            .into_any_element()
    }

    fn render_fast_mode(
        &self,
        theme: &ThemeTokens,
        settings: &PaneChatModelSettings,
        is_busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = self.translator.translate("paneChat.fastMode");
        let is_fast = settings.is_fast == Some(true);
        let is_disabled = is_busy || settings.is_fast.is_none();
        let switch = fast_switch(theme, is_fast, label.clone());
        let switch = match is_disabled {
            true => switch.opacity(0.4),
            false => switch
                .cursor_pointer()
                .on_click(cx.listener(move |view, _, window, cx| {
                    let change = PaneChatModelSettingChange::Fast { is_fast: !is_fast };
                    view.change_model_setting(change, window, cx)
                })),
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(theme.border.subtle)
            .px(spacing(4.0))
            .py(spacing(3.0))
            .child(
                div()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.secondary)
                    .child(label),
            )
            .child(switch)
            .into_any_element()
    }

    fn render_model_error(
        &self,
        theme: &ThemeTokens,
        is_busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let retry = self.translator.translate("paneChat.retry");
        let mut button = div()
            .id("pane-chat-model-retry")
            .relative()
            .flex()
            .min_h(px(36.0))
            .items_center()
            .rounded(radius::BASE)
            .px(spacing(2.0))
            .text_color(theme.text.primary)
            .child(retry.clone())
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: retry.clone().into(),
                text: retry.into(),
            }));
        if !is_busy {
            button = button
                .cursor_pointer()
                .on_click(cx.listener(|view, _, window, cx| view.load_model_settings(window, cx)));
        }
        div()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .px(spacing(4.0))
            .pb(spacing(3.0))
            .text_scale(text_scale::XS)
            .text_color(theme.status.error)
            .child(self.translator.translate("paneChat.modelError"))
            .child(button)
            .into_any_element()
    }
}

/// 팝오버 틀(`rounded-xl border bg-bg-surface shadow-xl`, 넘치면 안에서 스크롤)
fn dialog_frame(
    theme: &ThemeTokens,
    focus_handle: &FocusHandle,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id("pane-chat-model-picker")
        .track_focus(focus_handle)
        .relative()
        .max_h_full()
        .w_full()
        .overflow_y_scroll()
        .rounded(radius::XL)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .text_color(theme.text.primary)
        .shadow(crate::components::box_shadows(&theme.shadow.lg))
        .occlude()
}

/// Esc로 닫기(모델 버튼으로 포커스를 돌린다), 방향키로 모델 사이 이동, 바깥(모델 버튼 빼고)을 누르면 닫기
fn with_picker_events(
    element: gpui_kit::Stateful<gpui_kit::Div>,
    cx: &mut Context<PaneChatView>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    element
        .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                view.close_model_picker(true, window, cx);
                cx.stop_propagation();
            } else if view.navigate_models(event, window, cx) {
                cx.stop_propagation();
            }
        }))
        .on_mouse_down_out(
            cx.listener(|view, event: &gpui_kit::MouseDownEvent, window, cx| {
                if !view.composer.is_model_trigger_at(event.position) {
                    view.close_model_picker(false, window, cx);
                }
            }),
        )
}

fn model_footer(theme: &ThemeTokens, text: String) -> AnyElement {
    div()
        .border_t_1()
        .border_color(theme.border.subtle)
        .px(spacing(4.0))
        .py(spacing(3.0))
        .text_scale(text_scale::XS)
        .line_height(px(20.0))
        .text_color(theme.text.secondary)
        .child(text)
        .into_any_element()
}

type StatefulDiv = gpui_kit::Stateful<gpui_kit::Div>;

/// 모델 한 줄. 지금 모델이면 브랜드 바탕에 체크를 붙인다
fn model_row(theme: &ThemeTokens, model: &PaneChatModelOption, is_checked: bool) -> StatefulDiv {
    let row = div()
        .id(gpui_kit::SharedString::from(format!(
            "pane-chat-model-{}",
            model.id
        )))
        .relative()
        .flex()
        .min_h(px(44.0))
        .w_full()
        .items_center()
        .gap(spacing(3.0))
        .rounded(radius::LG)
        .px(spacing(3.0))
        .text_scale(text_scale::SM)
        .child(
            div()
                .min_w_0()
                .flex_1()
                .truncate()
                .child(model.label.clone()),
        )
        .children(is_checked.then(|| check_icon(theme)))
        .child(qa_target_layer(QaTargetDescription {
            role: "menuitemradio",
            name: model.label.clone().into(),
            text: model.label.clone().into(),
        }));
    match is_checked {
        true => row.bg(theme.brand.subtle).text_color(theme.text.primary),
        false => row.hover(|style| style.bg(theme.background.page)),
    }
}

/// 누를 수 없으면 누르기를 받지 않는 작은 글자 버튼("기본값")
fn labelled_button(
    theme: &ThemeTokens,
    id: &'static str,
    label: String,
    is_disabled: bool,
) -> super::widgets::PaneChatButton {
    let element = div()
        .id(id)
        .relative()
        .rounded(radius::BASE)
        .px(spacing(1.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.secondary)
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }));
    super::widgets::PaneChatButton::new(element, is_disabled)
}

/// 구역 제목 줄(왼쪽 제목, 오른쪽에 붙일 버튼 자리)
fn section_heading(theme: &ThemeTokens, title: String) -> gpui_kit::Div {
    div()
        .mb(spacing(2.0))
        .flex()
        .items_center()
        .justify_between()
        .gap(spacing(2.0))
        .child(
            div()
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(title),
        )
}

/// 추론 강도 한 칸. 고른 칸은 밝은 바탕과 그림자로 떠 보인다
fn effort_segment(theme: &ThemeTokens, id: &str, label: &str, is_selected: bool) -> StatefulDiv {
    let segment = div()
        .id(gpui_kit::SharedString::from(format!(
            "pane-chat-effort-{id}"
        )))
        .relative()
        .flex()
        .flex_1()
        .min_h(px(36.0))
        .items_center()
        .justify_center()
        .rounded(radius::MD)
        .px(px(10.0))
        .text_scale(text_scale::XS)
        .text_center()
        .child(label.to_string())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.to_string().into(),
            text: label.to_string().into(),
        }));
    match is_selected {
        true => segment
            .bg(theme.background.surface)
            .css_weight(500)
            .text_color(theme.text.primary)
            .shadow(crate::components::box_shadows(&theme.shadow.sm)),
        false => segment.text_color(theme.text.secondary),
    }
}

/// 빠른 모드 스위치(`h-6 w-10 rounded-full`, 켜지면 브랜드 바탕에 손잡이가 오른쪽)
fn fast_switch(theme: &ThemeTokens, is_fast: bool, label: String) -> StatefulDiv {
    let track = match is_fast {
        true => theme.brand.primary,
        false => theme.border.default,
    };
    div()
        .id("pane-chat-fast-mode")
        .relative()
        .h(px(24.0))
        .w(px(40.0))
        .rounded(radius::FULL)
        .bg(track)
        .child(
            div()
                .absolute()
                .top(px(4.0))
                .left(px(if is_fast { 20.0 } else { 4.0 }))
                .size(px(16.0))
                .rounded(radius::FULL)
                .bg(gpui_kit::white()),
        )
        .child(qa_target_layer(QaTargetDescription {
            role: "switch",
            name: label.into(),
            text: String::new().into(),
        }))
}

fn status_text(theme: &ThemeTokens, text: String) -> AnyElement {
    div()
        .px(spacing(4.0))
        .py(spacing(5.0))
        .text_scale(text_scale::SM)
        .text_color(theme.text.secondary)
        .child(text)
        .into_any_element()
}

/// TS `m3 8 3 3 7-7` 체크 표시
fn check_icon(theme: &ThemeTokens) -> AnyElement {
    super::icons::check_mark(theme.brand.primary, px(16.0))
}
