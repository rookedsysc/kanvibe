//! 레이아웃 하나를 고치는 편집기(TS `PaneLayoutEditor`). 레이아웃 6종 미리보기 중 하나를 고르고 pane마다 시작 명령을 적은 뒤 저장한다.
//! 편집 중인 값은 이 편집기가 들고 있어, 화면을 다시 읽어도(성공·실패 모두) 덮이지 않는다.

use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::{
    App, Context, Div, Entity, Focusable, InteractiveElement, IntoElement, ParentElement, Render,
    Styled, Subscription, Task, Window, div, px,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::pane_layout::{PaneLayoutConfig, PaneLayoutType};

use crate::components::{TailwindText, fade};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::pane_layout::store::{PaneLayoutSaveRequest, delete_layout, save_layout};
use crate::views::settings::controls::{keyed_id, make_pressable, pressable};
use crate::views::settings::text_field::{create_text_field, themed_text_field};

/// 레이아웃별 pane 위치 라벨 키(`paneLayout.<키>`)
pub fn pane_label_keys(layout_type: PaneLayoutType) -> &'static [&'static str] {
    match layout_type {
        PaneLayoutType::Single => &["paneLeft"],
        PaneLayoutType::Horizontal2 => &["paneTop", "paneBottom"],
        PaneLayoutType::Vertical2 => &["paneLeft", "paneRight"],
        PaneLayoutType::LeftRightTb => &["paneLeft", "paneRightTop", "paneRightBottom"],
        PaneLayoutType::LeftTbRight => &["paneLeftTop", "paneLeftBottom", "paneRight"],
        PaneLayoutType::Quad => &[
            "paneTopLeft",
            "paneTopRight",
            "paneBottomLeft",
            "paneBottomRight",
        ],
    }
}

fn layout_label_key(layout_type: PaneLayoutType) -> &'static str {
    match layout_type {
        PaneLayoutType::Single => "layoutSingle",
        PaneLayoutType::Horizontal2 => "layoutHorizontal2",
        PaneLayoutType::Vertical2 => "layoutVertical2",
        PaneLayoutType::LeftRightTb => "layoutLeftRightTB",
        PaneLayoutType::LeftTbRight => "layoutLeftTBRight",
        PaneLayoutType::Quad => "layoutQuad",
    }
}

/// 미리보기 사각형(SVG `viewBox="0 0 40 30"` 좌표): x, y, 폭, 높이
fn preview_rects(layout_type: PaneLayoutType) -> &'static [(f32, f32, f32, f32)] {
    match layout_type {
        PaneLayoutType::Single => &[(2.0, 2.0, 36.0, 26.0)],
        PaneLayoutType::Horizontal2 => &[(2.0, 2.0, 36.0, 12.0), (2.0, 16.0, 36.0, 12.0)],
        PaneLayoutType::Vertical2 => &[(2.0, 2.0, 17.0, 26.0), (21.0, 2.0, 17.0, 26.0)],
        PaneLayoutType::LeftRightTb => &[
            (2.0, 2.0, 17.0, 26.0),
            (21.0, 2.0, 17.0, 12.0),
            (21.0, 16.0, 17.0, 12.0),
        ],
        PaneLayoutType::LeftTbRight => &[
            (2.0, 2.0, 17.0, 12.0),
            (2.0, 16.0, 17.0, 12.0),
            (21.0, 2.0, 17.0, 26.0),
        ],
        PaneLayoutType::Quad => &[
            (2.0, 2.0, 17.0, 12.0),
            (21.0, 2.0, 17.0, 12.0),
            (2.0, 16.0, 17.0, 12.0),
            (21.0, 16.0, 17.0, 12.0),
        ],
    }
}

/// 선택된 미리보기 채움(`var(--color-brand-primary-light, #dbeafe)`, 변수가 없어 대체값이 쓰인다)
const SELECTED_PREVIEW_FILL: u32 = 0xdbeafe;

/// 저장·삭제 결과 문구
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorMessage {
    Success(String),
    Failure(String),
}

/// 편집 값과 저장 상태. 서비스 없이 바뀌므로 화면 밖에서도 검증한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneLayoutDraft {
    pub layout_type: PaneLayoutType,
    pub commands: Vec<String>,
}

impl PaneLayoutDraft {
    pub fn from_config(config: Option<&PaneLayoutConfig>) -> Self {
        let layout_type = config
            .map(|config| config.layout_type)
            .unwrap_or(PaneLayoutType::Single);
        let commands = (0..pane_label_keys(layout_type).len())
            .map(|index| {
                config
                    .and_then(|config| config.panes.get(index))
                    .map(|pane| pane.command.clone())
                    .unwrap_or_default()
            })
            .collect();
        Self {
            layout_type,
            commands,
        }
    }

    /// 레이아웃을 바꾸면 pane 수에 맞게 명령 목록을 자르거나 빈 칸으로 늘린다
    pub fn change_layout(&mut self, layout_type: PaneLayoutType) {
        self.layout_type = layout_type;
        let pane_count = pane_label_keys(layout_type).len();
        self.commands.resize(pane_count, String::new());
    }

    /// 프로젝트 설정을 지운 뒤 화면은 1분할 빈 값으로 돌아간다
    pub fn reset(&mut self) {
        self.layout_type = PaneLayoutType::Single;
        self.commands = vec![String::new()];
    }
}

pub struct PaneLayoutEditor {
    locale: Locale,
    project_id: Option<String>,
    is_global: bool,
    /// 지울 수 있는 저장된 설정 id(프로젝트 설정만)
    saved_config_id: Option<String>,
    pub draft: PaneLayoutDraft,
    inputs: Vec<Entity<InputState>>,
    pub is_pending: bool,
    pub message: Option<EditorMessage>,
    _input_subscriptions: Vec<Subscription>,
    _request: Option<Task<()>>,
}

impl PaneLayoutEditor {
    pub fn new(
        locale: Locale,
        project_id: Option<String>,
        config: Option<&PaneLayoutConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut editor = Self {
            locale,
            is_global: project_id.is_none(),
            project_id,
            saved_config_id: config.map(|config| config.id.clone()),
            draft: PaneLayoutDraft::from_config(config),
            inputs: Vec::new(),
            is_pending: false,
            message: None,
            _input_subscriptions: Vec::new(),
            _request: None,
        };
        editor.rebuild_inputs(window, cx);
        editor
    }

    fn translate(&self, key: &str) -> String {
        Translator::new(self.locale, "paneLayout").translate(key)
    }

    /// pane 입력란을 지금 명령 목록으로 다시 만든다(TS는 레이아웃이 바뀌면 입력란을 새로 붙였다)
    fn rebuild_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = self.translate("commandPlaceholder");
        self.inputs = self
            .draft
            .commands
            .iter()
            .map(|command| create_text_field(placeholder.clone(), command, window, cx))
            .collect();
        self._input_subscriptions = self
            .inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                cx.subscribe(input, move |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = input.read(cx).value().to_string();
                        if let Some(command) = this.draft.commands.get_mut(index) {
                            *command = value;
                        }
                    }
                })
            })
            .collect();
    }

    pub fn change_layout(
        &mut self,
        layout_type: PaneLayoutType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.draft.change_layout(layout_type);
        self.rebuild_inputs(window, cx);
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        self.message = None;
        self.is_pending = true;
        let request = save_layout(
            PaneLayoutSaveRequest {
                layout_type: self.draft.layout_type,
                commands: self.draft.commands.clone(),
                project_id: if self.is_global {
                    None
                } else {
                    self.project_id.clone()
                },
                is_global: self.is_global,
            },
            cx,
        );
        let (success, failure) = (self.translate("saveSuccess"), self.translate("saveError"));
        self._request = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.is_pending = false;
                this.message = Some(match result {
                    Ok(()) => EditorMessage::Success(success),
                    Err(_) => EditorMessage::Failure(failure),
                });
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(config_id) = self.saved_config_id.clone() else {
            return;
        };
        self.message = None;
        self.is_pending = true;
        let request = delete_layout(config_id, cx);
        let (success, failure) = (
            self.translate("deleteSuccess"),
            self.translate("deleteError"),
        );
        let window_handle = window.window_handle();
        self._request = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.is_pending = false;
                    match result {
                        Ok(()) => {
                            this.draft.reset();
                            this.rebuild_inputs(window, cx);
                            this.message = Some(EditorMessage::Success(success));
                        }
                        Err(_) => this.message = Some(EditorMessage::Failure(failure)),
                    }
                    cx.notify();
                });
            });
        }));
        cx.notify();
    }
}

impl Render for PaneLayoutEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let mut pane_fields = Vec::new();
        for index in 0..self.inputs.len() {
            pane_fields.push(self.render_pane_field(&theme, index, cx).into_any_element());
        }
        div()
            .flex()
            .flex_col()
            .gap(spacing(4.0))
            .child(self.render_layout_choices(&theme, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing(2.0))
                    .children(pane_fields),
            )
            .child(self.render_actions(&theme, cx))
            .children(self.message.clone().map(|message| {
                let (color, text) = match message {
                    EditorMessage::Success(text) => (theme.status.success, text),
                    EditorMessage::Failure(text) => (theme.status.error, text),
                };
                div()
                    .text_scale(text_scale::XS)
                    .text_color(color)
                    .child(text)
            }))
    }
}

impl PaneLayoutEditor {
    /// `grid grid-cols-3 gap-2`, 항목 `flex flex-col items-center gap-1 p-2 rounded-lg border`,
    /// 선택 `border-brand-primary bg-brand-primary/5`, 나머지 `border-border-default hover:border-border-strong`
    fn render_layout_choices(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut rows = Vec::new();
        for row in PaneLayoutType::ALL.chunks(3) {
            let mut cells = Vec::new();
            for &layout_type in row {
                cells.push(
                    self.render_layout_choice(theme, layout_type, cx)
                        .into_any_element(),
                );
            }
            rows.push(div().flex().gap(spacing(2.0)).children(cells));
        }
        div()
            .child(
                div()
                    .mb(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(self.translate("layoutType")),
            )
            .child(div().flex().flex_col().gap(spacing(2.0)).children(rows))
    }

    fn render_layout_choice(
        &self,
        theme: &ThemeTokens,
        layout_type: PaneLayoutType,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = self.draft.layout_type == layout_type;
        let label = self.translate(layout_label_key(layout_type));
        let hover_border = theme.border.strong;
        let choice = pressable(
            keyed_id("pane-layout-choice", layout_type.as_str()),
            "button",
            label.clone(),
            label.clone(),
        )
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .gap(spacing(1.0))
        .p(spacing(2.0))
        .rounded(radius::LG)
        .border_1()
        .child(render_preview(theme, layout_type, is_selected))
        .child(
            div()
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(label),
        );
        let choice = if is_selected {
            choice
                .border_color(theme.brand.primary)
                .bg(fade(theme.brand.primary, 0.05))
        } else {
            choice
                .border_color(theme.border.default)
                .hover(move |style| style.border_color(hover_border))
        };
        make_pressable(
            choice,
            theme,
            false,
            cx.listener(move |this, _, window, cx| this.change_layout(layout_type, window, cx)),
        )
    }

    /// 라벨 `block text-xs text-text-secondary mb-1`, 입력 `w-full px-3 py-1.5 text-sm bg-bg-page border rounded-md font-mono`
    fn render_pane_field(
        &self,
        theme: &ThemeTokens,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label_key = pane_label_keys(self.draft.layout_type)
            .get(index)
            .copied()
            .unwrap_or("paneLeft");
        let field = themed_text_field(&self.inputs[index], theme, theme.text.primary, cx);
        div()
            .child(
                div()
                    .mb(spacing(1.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.secondary)
                    .child(self.translate(label_key)),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .px(spacing(3.0))
                    .py(spacing(1.5))
                    .text_scale(text_scale::SM)
                    .mono(400)
                    .bg(theme.background.page)
                    .border_1()
                    .border_color(theme.border.default)
                    .rounded(radius::MD)
                    .child(field)
                    .child(self.render_pane_field_qa_target(index, label_key, cx)),
            )
    }

    /// pane 입력란의 QA 대상. 접근성 이름은 라벨이고, QA `fill`이 입력란 값을 바꿀 수 있게 포커스와 채우기 동작을 붙인다
    fn render_pane_field_qa_target(
        &self,
        index: usize,
        label_key: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let input = self.inputs[index].clone();
        let focus_handle = input.read(cx).focus_handle(cx);
        crate::qa::targets::qa_target_layer_with(
            crate::qa::targets::QaTargetDescription {
                role: "textbox",
                name: self.translate(label_key).into(),
                text: "".into(),
            },
            crate::qa::targets::QaTargetBehavior {
                placeholder: Some(self.translate("commandPlaceholder").into()),
                selector: None,
                focus_handle: Some(focus_handle),
                fill: Some(std::rc::Rc::new(
                    move |value: &str, window: &mut Window, cx: &mut App| {
                        input.update(cx, |state, cx| {
                            state.set_value(value.to_string(), window, cx)
                        });
                    },
                )),
            },
        )
    }

    /// 프로젝트 설정을 지우고 전역 기본값으로(`text-status-error border border-status-error`)
    fn render_reset_button(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let label = self.translate("resetToGlobal");
        let hover = fade(theme.status.error, 0.1);
        let reset = pressable("pane-layout-reset", "button", label.clone(), label.clone())
            .px(spacing(4.0))
            .py(spacing(1.5))
            .text_scale(text_scale::SM)
            .text_color(theme.status.error)
            .border_1()
            .border_color(theme.status.error)
            .rounded(radius::MD)
            .hover(move |style| style.bg(hover))
            .child(label);
        make_pressable(
            reset,
            theme,
            self.is_pending,
            cx.listener(|this, _, window, cx| this.delete(window, cx)),
        )
    }

    fn render_actions(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> Div {
        let common = Translator::new(self.locale, "common");
        let save_label = if self.is_pending {
            self.translate("saving")
        } else {
            common.translate("save")
        };
        let save = pressable(
            "pane-layout-save",
            "button",
            save_label.clone(),
            save_label.clone(),
        )
        .px(spacing(4.0))
        .py(spacing(1.5))
        .text_scale(text_scale::SM)
        .bg(theme.brand.primary)
        .text_color(gpui_kit::white())
        .rounded(radius::MD)
        .child(save_label);
        let save = make_pressable(
            save,
            theme,
            self.is_pending,
            cx.listener(|this, _, _, cx| this.save(cx)),
        );
        let reset = (!self.is_global && self.saved_config_id.is_some())
            .then(|| self.render_reset_button(theme, cx));
        div()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .child(save)
            .children(reset)
    }
}

/// 레이아웃 미리보기(40×30 SVG). 선택되면 brand 2px 선과 연한 파랑 채움, 아니면 border-default 1px 선과 페이지 바탕 채움
fn render_preview(
    theme: &ThemeTokens,
    layout_type: PaneLayoutType,
    is_selected: bool,
) -> impl IntoElement {
    let (stroke_color, fill_color, stroke_width) = if is_selected {
        (
            theme.brand.primary,
            gpui_kit::rgb(SELECTED_PREVIEW_FILL),
            2.0,
        )
    } else {
        (theme.border.default, theme.background.page, 1.0)
    };
    let half = stroke_width / 2.0;
    let rects = preview_rects(layout_type)
        .iter()
        .map(move |&(x, y, width, height)| {
            div()
                .absolute()
                .left(px(x - half))
                .top(px(y - half))
                .w(px(width + stroke_width))
                .h(px(height + stroke_width))
                .rounded(px(2.0 + half))
                .border(px(stroke_width))
                .border_color(stroke_color)
                .bg(fill_color)
        });
    div()
        .relative()
        .flex_none()
        .w(px(40.0))
        .h(px(30.0))
        .children(rects)
}
