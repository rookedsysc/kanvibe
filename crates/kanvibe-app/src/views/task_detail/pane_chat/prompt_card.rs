//! CLI가 답을 기다리며 띄운 선택 화면의 카드(TS `PaneChatPromptCard`). AI의 질문, 도구 실행 허용 요청, CLI 스스로의 확인을
//! 같은 모양으로 보이고, 고른 답은 서비스가 CLI 화면에 키 입력으로 전달한다. 체크만 바뀐 같은 선택 화면이면 카드를 다시 만들지 않아
//! 적던 글과 답을 보내지 못한 이유가 남고, 다른 선택 화면이 뜨면 새 카드로 시작한다.

use std::collections::BTreeMap;

use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use kanvibe_ai::pane_chat::claude_prompt::{
    PaneChatPromptKind, PaneChatPromptOption, is_blank_claude_text_entry,
};
use kanvibe_ai::pane_chat::snapshot::{
    PaneChatPrompt, PaneChatSnapshot, get_pane_chat_prompt_dialog_key,
};
use kanvibe_domain::pane_chat::{
    MAX_PANE_CHAT_PROMPT_OPTION_NUMBER, PaneChatPromptAnswer, PaneChatPromptStep,
};

use super::view::PaneChatView;
use super::widgets::{control_button, number_badge};
use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::text_field::themed_text_field;

/// 카드가 보이는 선택 화면과 직접 입력 줄마다의 입력란
#[derive(Default)]
pub struct PromptCardState {
    dialog_key: Option<String>,
    text_inputs: BTreeMap<u64, Entity<InputState>>,
    _subscriptions: Vec<Subscription>,
}

impl PromptCardState {
    /// 다른 선택 화면이 뜨면 새 카드로 시작한다. 직접 입력 줄에는 입력란을 하나씩 둔다
    pub fn sync(
        &mut self,
        snapshot: Option<&PaneChatSnapshot>,
        placeholder: SharedString,
        window: &mut Window,
        cx: &mut Context<PaneChatView>,
    ) {
        let prompt = snapshot.and_then(|snapshot| snapshot.prompt.as_ref());
        let dialog_key = prompt.map(get_pane_chat_prompt_dialog_key);
        if dialog_key != self.dialog_key {
            self.dialog_key = dialog_key;
            self.text_inputs.clear();
            self._subscriptions.clear();
        }
        let Some(prompt) = prompt else {
            return;
        };
        for option in prompt
            .screen
            .options
            .iter()
            .filter(|option| is_blank_claude_text_entry(option))
        {
            if self.text_inputs.contains_key(&option.number) {
                continue;
            }
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.clone()));
            let number = option.number;
            self._subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |view, _, event, window, cx| match event {
                    InputEvent::PressEnter { .. } => view.submit_prompt_text(number, window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ));
            self.text_inputs.insert(number, input);
        }
    }

    pub fn text_input(&self, number: u64) -> Option<&Entity<InputState>> {
        self.text_inputs.get(&number)
    }
}

fn kind_key(kind: PaneChatPromptKind) -> &'static str {
    match kind {
        PaneChatPromptKind::Question => "paneChat.promptQuestion",
        PaneChatPromptKind::Permission => "paneChat.promptPermission",
        PaneChatPromptKind::Confirmation => "paneChat.promptConfirmation",
    }
}

/// 단계 줄의 공백 덩어리는 HTML처럼 한 칸으로 보인다
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 답을 보내는 중인 단일 선택, 숫자 키 하나로 고를 수 없는 두 자리 번호는 누를 수 없다
fn is_choice_disabled(option: &PaneChatPromptOption, is_multi_select: bool, is_busy: bool) -> bool {
    (is_busy && !is_multi_select) || option.number > u64::from(MAX_PANE_CHAT_PROMPT_OPTION_NUMBER)
}

/// 카드 선택지 한 줄이 보이는 모양
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptOptionModel {
    /// 글을 적어 답하는 입력란과 제출 버튼
    TextEntry { number: u64, can_submit: bool },
    /// 이미 글이 들어 있는 직접 입력 줄. 단일 선택이면 그 글로 답하는 버튼이 있다
    EnteredText {
        number: u64,
        label: String,
        can_submit: Option<bool>,
    },
    /// 고르는 버튼. 다중 선택이면 눌림 상태를 알린다
    Choice {
        number: u64,
        label: String,
        description: String,
        is_enabled: bool,
        is_pressed: Option<bool>,
    },
}

/// 선택 카드에 보이는 내용
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCardModel {
    pub kind_label: String,
    pub steps_line: Option<String>,
    pub detail: String,
    pub title: String,
    pub hidden_options_notice: Option<String>,
    pub options: Vec<PromptOptionModel>,
    pub error: Option<String>,
    /// 이전·다음 질문 버튼을 누를 수 있는지. 단계가 없으면 버튼이 없다
    pub are_steps_enabled: Option<bool>,
    pub is_cancel_enabled: bool,
}

fn option_number(number: u64) -> u8 {
    u8::try_from(number).unwrap_or(u8::MAX)
}

impl PaneChatView {
    pub fn prompt_text_input(&self, number: u64) -> Option<Entity<InputState>> {
        self.prompt_card.text_input(number).cloned()
    }

    /// 선택 화면이 떠 있으면 카드에 보이는 내용
    pub fn prompt_card_model(&self, cx: &gpui_kit::App) -> Option<PromptCardModel> {
        let prompt = self.session.snapshot()?.prompt.as_ref()?;
        let screen = &prompt.screen;
        let is_busy = self.session.is_busy;
        let dialog_key = get_pane_chat_prompt_dialog_key(prompt);
        let options = screen
            .options
            .iter()
            .map(|option| self.prompt_option_model(option, screen.is_multi_select, is_busy, cx))
            .collect();
        Some(PromptCardModel {
            kind_label: self.translator.translate(kind_key(screen.kind)),
            steps_line: screen.steps_line.as_deref().map(collapse_whitespace),
            detail: screen.detail.clone(),
            title: screen.title.clone(),
            hidden_options_notice: screen
                .has_hidden_options
                .then(|| self.translator.translate("paneChat.promptHiddenOptions")),
            options,
            error: self
                .session
                .prompt_error
                .as_ref()
                .filter(|error| error.dialog_key == dialog_key)
                .map(|error| error.message.clone()),
            are_steps_enabled: screen.steps_line.as_ref().map(|_| !is_busy),
            is_cancel_enabled: !is_busy,
        })
    }

    fn prompt_option_model(
        &self,
        option: &PaneChatPromptOption,
        is_multi_select: bool,
        is_busy: bool,
        cx: &gpui_kit::App,
    ) -> PromptOptionModel {
        if is_blank_claude_text_entry(option) {
            let has_text = self
                .prompt_card
                .text_input(option.number)
                .is_some_and(|input| !input.read(cx).value().trim().is_empty());
            return PromptOptionModel::TextEntry {
                number: option.number,
                can_submit: !is_busy && has_text,
            };
        }
        if option.is_text_entry {
            return PromptOptionModel::EnteredText {
                number: option.number,
                label: option.label.clone(),
                can_submit: (!is_multi_select).then_some(!is_busy),
            };
        }
        PromptOptionModel::Choice {
            number: option.number,
            label: option.label.clone(),
            description: option.description.clone(),
            is_enabled: !is_choice_disabled(option, is_multi_select, is_busy),
            is_pressed: is_multi_select.then_some(option.is_checked),
        }
    }

    /// 직접 입력 줄에 적은 글로 답한다. 빈 글은 보내지 않는다
    pub fn submit_prompt_text(&mut self, number: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.prompt_card.text_input(number) else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        let prompt = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.prompt.clone());
        if text.is_empty() || self.session.is_busy {
            return;
        }
        if let Some(prompt) = prompt {
            let answer = PaneChatPromptAnswer::Text {
                number: option_number(number),
                text,
            };
            self.answer_prompt(prompt, answer, window, cx);
        }
    }

    /// 지금 떠 있는 선택 화면에 답한다(카드의 선택지·이동·취소 버튼)
    pub fn answer_shown_prompt(
        &mut self,
        answer: PaneChatPromptAnswer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(prompt) = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.prompt.clone())
        {
            self.answer_prompt(prompt, answer, window, cx);
        }
    }

    pub(super) fn render_prompt_card(
        &self,
        theme: &ThemeTokens,
        prompt: &PaneChatPrompt,
        horizontal_padding: gpui_kit::Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kind_label = self.translator.translate(kind_key(prompt.screen.kind));
        let dialog_key = get_pane_chat_prompt_dialog_key(prompt);
        let error = self
            .session
            .prompt_error
            .as_ref()
            .filter(|error| error.dialog_key == dialog_key)
            .map(|error| error.message.clone());
        let card = div()
            .flex()
            .flex_col()
            .gap(spacing(2.0))
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .p(spacing(3.0))
            .child(self.render_prompt_heading(theme, prompt, kind_label))
            .children(self.render_prompt_body(theme, prompt))
            .child(self.render_prompt_options(theme, prompt, cx))
            .children(error.map(|message| prompt_error_line(theme, message)))
            .child(self.render_prompt_actions(theme, prompt, cx));
        div()
            .id("pane-chat-prompt")
            .mx_auto()
            .flex()
            .flex_col()
            .max_h(gpui_kit::relative(0.7))
            .min_h_0()
            .w_full()
            .max_w(px(768.0 + 2.0 * f32::from(horizontal_padding)))
            .flex_shrink_0()
            .gap(spacing(2.0))
            .overflow_y_scroll()
            .px(horizontal_padding)
            .pb(spacing(3.0))
            .child(card)
            .into_any_element()
    }

    fn render_prompt_heading(
        &self,
        theme: &ThemeTokens,
        prompt: &PaneChatPrompt,
        kind_label: String,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .child(
                div()
                    .rounded(radius::BASE)
                    .bg(theme.brand.subtle)
                    .px(spacing(2.0))
                    .py(px(2.0))
                    .css_weight(500)
                    .text_color(theme.text.brand)
                    .child(kind_label),
            )
            .children(
                prompt
                    .screen
                    .steps_line
                    .as_deref()
                    .map(|steps| div().mono(400).child(collapse_whitespace(steps))),
            )
            .into_any_element()
    }

    /// 제목 위 내용(명령 등), 제목, 화면에 다 보이지 않는 선택지 안내
    fn render_prompt_body(&self, theme: &ThemeTokens, prompt: &PaneChatPrompt) -> Vec<AnyElement> {
        let mut blocks = Vec::new();
        if !prompt.screen.detail.is_empty() {
            blocks.push(
                div()
                    .max_h(px(160.0))
                    .rounded(radius::MD)
                    .bg(theme.background.page)
                    .px(spacing(3.0))
                    .py(spacing(2.0))
                    .mono(400)
                    .text_scale(text_scale::XS)
                    .line_height(px(20.0))
                    .text_color(theme.text.primary)
                    .child(prompt.screen.detail.clone())
                    .into_any_element(),
            );
        }
        blocks.push(
            div()
                .text_scale(text_scale::SM)
                .css_weight(500)
                .text_color(theme.text.primary)
                .child(prompt.screen.title.clone())
                .into_any_element(),
        );
        if prompt.screen.has_hidden_options {
            blocks.push(
                div()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.secondary)
                    .child(self.translator.translate("paneChat.promptHiddenOptions"))
                    .into_any_element(),
            );
        }
        blocks
    }

    fn render_prompt_options(
        &self,
        theme: &ThemeTokens,
        prompt: &PaneChatPrompt,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_busy = self.session.is_busy;
        let rows = prompt.screen.options.iter().map(|option| {
            if is_blank_claude_text_entry(option) {
                self.render_text_entry(theme, option, is_busy, cx)
            } else if option.is_text_entry {
                self.render_entered_text(theme, option, prompt.screen.is_multi_select, is_busy, cx)
            } else {
                self.render_prompt_option(theme, option, prompt.screen.is_multi_select, is_busy, cx)
            }
        });
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(rows.collect::<Vec<_>>())
            .into_any_element()
    }

    /// 다중 선택은 보내는 중에 누른 다른 선택지도 차례로 전달되므로 잠그지 않는다
    fn render_prompt_option(
        &self,
        theme: &ThemeTokens,
        option: &PaneChatPromptOption,
        is_multi_select: bool,
        is_busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let number = option_number(option.number);
        let button = choice_button(theme, option);
        if is_choice_disabled(option, is_multi_select, is_busy) {
            return button.opacity(0.4).into_any_element();
        }
        button
            .cursor_pointer()
            .hover(|style| style.bg(theme.background.page))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.answer_shown_prompt(PaneChatPromptAnswer::Option { number }, window, cx)
            }))
            .into_any_element()
    }

    /// 아직 글이 없는 직접 입력 줄. 글을 적어 제출하면 CLI의 그 줄에 넣고 답한다
    fn render_text_entry(
        &self,
        theme: &ThemeTokens,
        option: &PaneChatPromptOption,
        is_busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let number = option.number;
        let input = self.prompt_card.text_input(number);
        let has_text = input.is_some_and(|input| !input.read(cx).value().trim().is_empty());
        let label = self.translator.translate("paneChat.promptSubmitText");
        let submit = control_button(
            theme,
            format!("pane-chat-prompt-submit-{number}"),
            label,
            is_busy || !has_text,
        )
        .on_click(
            cx.listener(move |view, _, window, cx| view.submit_prompt_text(number, window, cx)),
        );
        div()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .px(spacing(2.0))
            .py(px(6.0))
            .child(number_badge(theme, number))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_scale(text_scale::SM)
                    .children(
                        input.map(|input| themed_text_field(input, theme, theme.text.primary, cx)),
                    ),
            )
            .child(submit)
            .into_any_element()
    }

    /// 글이 이미 들어 있는 직접 입력 줄. 다중 선택은 그 순간 체크되어 이미 답이고, 단일 선택만 그 글로 답하는 버튼을 둔다
    fn render_entered_text(
        &self,
        theme: &ThemeTokens,
        option: &PaneChatPromptOption,
        is_multi_select: bool,
        is_busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let number = option_number(option.number);
        let text = option.label.clone();
        let submit = (!is_multi_select).then(|| {
            let label = self.translator.translate("paneChat.promptSubmitText");
            control_button(
                theme,
                format!("pane-chat-prompt-entered-{number}"),
                label,
                is_busy,
            )
            .on_click(cx.listener(move |view, _, window, cx| {
                let answer = PaneChatPromptAnswer::Text {
                    number,
                    text: text.clone(),
                };
                view.answer_shown_prompt(answer, window, cx)
            }))
        });
        div()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .px(spacing(2.0))
            .py(px(6.0))
            .text_scale(text_scale::SM)
            .text_color(theme.text.primary)
            .child(number_badge(theme, option.number))
            .child(
                div().min_w_0().flex_1().child(option.label.clone()).child(
                    div()
                        .text_scale(text_scale::XS)
                        .text_color(theme.text.secondary)
                        .child(self.translator.translate("paneChat.promptTextEntered")),
                ),
            )
            .children(submit)
            .into_any_element()
    }

    fn render_prompt_actions(
        &self,
        theme: &ThemeTokens,
        prompt: &PaneChatPrompt,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_busy = self.session.is_busy;
        let cancel = control_button(
            theme,
            "pane-chat-prompt-cancel".to_string(),
            self.t("cancel"),
            is_busy,
        )
        .on_click(cx.listener(|view, _, window, cx| {
            view.answer_shown_prompt(PaneChatPromptAnswer::Cancel, window, cx)
        }));
        let terminal = control_button(
            theme,
            "pane-chat-prompt-terminal".to_string(),
            self.t("terminal"),
            false,
        )
        .on_click(cx.listener(|view, _, _, cx| view.open_terminal(cx)));
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(spacing(2.0))
            .children(match prompt.screen.steps_line.is_some() {
                true => self.render_step_buttons(theme, is_busy, cx),
                false => Vec::new(),
            })
            .child(div().ml_auto().child(cancel))
            .child(terminal)
            .into_any_element()
    }

    /// 질문이 여러 개일 때의 이전·다음 질문
    fn render_step_buttons(
        &self,
        theme: &ThemeTokens,
        is_busy: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        [
            ("promptPrevious", PaneChatPromptStep::Previous),
            ("promptNext", PaneChatPromptStep::Next),
        ]
        .into_iter()
        .map(|(key, direction)| {
            control_button(theme, format!("pane-chat-{key}"), self.t(key), is_busy)
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.answer_shown_prompt(PaneChatPromptAnswer::Step { direction }, window, cx)
                }))
                .into_any_element()
        })
        .collect()
    }
}

fn choice_target(option: &PaneChatPromptOption) -> impl IntoElement + use<> {
    qa_target_layer(QaTargetDescription {
        role: "button",
        name: format!("{} {} {}", option.number, option.label, option.description).into(),
        text: option.label.clone().into(),
    })
}

fn prompt_error_line(theme: &ThemeTokens, message: String) -> AnyElement {
    div()
        .text_scale(text_scale::XS)
        .text_color(theme.status.error)
        .child(message)
        .into_any_element()
}

/// 고르는 선택지 버튼의 모양. 체크된 선택지는 브랜드 테두리·바탕에 체크 표시가 붙는다
fn choice_button(
    theme: &ThemeTokens,
    option: &PaneChatPromptOption,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let button = div()
        .id(SharedString::from(format!(
            "pane-chat-prompt-option-{}",
            option.number
        )))
        .relative()
        .flex()
        .w_full()
        .items_start()
        .gap(spacing(2.0))
        .rounded(radius::MD)
        .border_1()
        .px(spacing(2.0))
        .py(px(6.0))
        .text_scale(text_scale::SM)
        .text_color(theme.text.primary)
        .child(number_badge(theme, option.number))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(option.label.clone())
                .children((!option.description.is_empty()).then(|| {
                    div()
                        .text_scale(text_scale::XS)
                        .text_color(theme.text.secondary)
                        .child(option.description.clone())
                })),
        )
        .children(
            option
                .is_checked
                .then(|| div().text_color(theme.text.brand).child("✔")),
        )
        .child(choice_target(option));
    match option.is_checked {
        true => button
            .border_color(theme.brand.primary)
            .bg(theme.brand.subtle),
        false => button
            .border_color(theme.border.default)
            .bg(theme.background.surface),
    }
}
