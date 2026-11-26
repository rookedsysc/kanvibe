//! 글자 버튼과 아이콘 버튼. 변형별 TS 클래스는 `ButtonVariant` 각 항목에 적었다.
//! 모든 버튼은 Tab으로 갈 수 있고 키보드 포커스에서 brand 2px 링을 그린다. 비활성 버튼은 `disabled:opacity-50`이고 눌리지 않는다.

use std::rc::Rc;

use gpui_kit::{
    App, ClickEvent, Div, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels,
    RenderOnce, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::components::icon::{IconName, icon};
use crate::components::{DISABLED_OPACITY, TailwindText, focus_ring};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

const ICON_BUTTON_GROUP: &str = "kanvibe-icon-button";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// `bg-brand-primary text-text-inverse hover:bg-brand-hover`
    Primary,
    /// `border border-border-default bg-bg-surface text-text-secondary hover:border-brand-primary hover:text-text-primary`
    Outline,
    /// `bg-button-neutral text-white hover:bg-button-neutral-hover`
    Neutral,
    /// `text-status-error hover:bg-button-neutral-subtle`
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    /// `px-3 py-1.5 text-xs` (높이 28px)
    Small,
    /// `px-4 py-1.5 text-sm` (높이 32px)
    Medium,
    /// `px-4 py-2 text-sm` (높이 36px)
    Large,
}

struct VariantColors {
    background: Option<gpui_kit::Rgba>,
    border: Option<gpui_kit::Rgba>,
    text: gpui_kit::Rgba,
    hover_background: Option<gpui_kit::Rgba>,
    hover_border: Option<gpui_kit::Rgba>,
    hover_text: Option<gpui_kit::Rgba>,
}

impl ButtonVariant {
    fn colors(self, theme: &ThemeTokens) -> VariantColors {
        match self {
            Self::Primary => VariantColors {
                background: Some(theme.brand.primary),
                border: None,
                text: theme.text.inverse,
                hover_background: Some(theme.brand.hover),
                hover_border: None,
                hover_text: None,
            },
            Self::Outline => VariantColors {
                background: Some(theme.background.surface),
                border: Some(theme.border.default),
                text: theme.text.secondary,
                hover_background: None,
                hover_border: Some(theme.brand.primary),
                hover_text: Some(theme.text.primary),
            },
            Self::Neutral => VariantColors {
                background: Some(theme.button_neutral.base),
                border: None,
                text: theme.text.inverse,
                hover_background: Some(theme.button_neutral.hover),
                hover_border: None,
                hover_text: None,
            },
            Self::Danger => VariantColors {
                background: None,
                border: None,
                text: theme.status.error,
                hover_background: Some(theme.button_neutral.subtle),
                hover_border: None,
                hover_text: None,
            },
        }
    }
}

/// 변형의 색·테두리·호버·포커스·비활성 상태를 건다. 글자 버튼과 아이콘 버튼이 함께 쓴다
fn style_button(
    base: Stateful<Div>,
    variant: ButtonVariant,
    is_disabled: bool,
    on_click: Option<ClickHandler>,
    theme: &ThemeTokens,
) -> Stateful<Div> {
    let colors = variant.colors(theme);
    let ring = focus_ring(theme);
    let mut button = base
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(radius::MD)
        .text_color(colors.text);
    if let Some(background) = colors.background {
        button = button.bg(background);
    }
    if let Some(border) = colors.border {
        button = button.border_1().border_color(border);
    }
    if is_disabled {
        return button.opacity(DISABLED_OPACITY);
    }
    button = button
        .tab_index(0)
        .cursor_pointer()
        .focus_visible(move |style| style.shadow(ring))
        .hover(move |style| {
            let style = match colors.hover_background {
                Some(background) => style.bg(background),
                None => style,
            };
            let style = match colors.hover_border {
                Some(border) => style.border_color(border),
                None => style,
            };
            match colors.hover_text {
                Some(text) => style.text_color(text),
                None => style,
            }
        });
    match on_click {
        Some(handler) => button.on_click(move |event, window, cx| handler(event, window, cx)),
        None => button,
    }
}

fn qa_button(label: &SharedString, text: &str) -> impl IntoElement {
    qa_target_layer(QaTargetDescription {
        role: "button",
        name: label.clone(),
        text: text.to_string().into(),
    })
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: ButtonVariant,
    size: ButtonSize,
    is_disabled: bool,
    is_full_width: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ButtonVariant::Outline,
            size: ButtonSize::Medium,
            is_disabled: false,
            is_full_width: false,
            on_click: None,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, is_disabled: bool) -> Self {
        self.is_disabled = is_disabled;
        self
    }

    /// `w-full`
    pub fn full_width(mut self) -> Self {
        self.is_full_width = true;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let (padding_x, padding_y, scale) = match self.size {
            ButtonSize::Small => (spacing(3.0), spacing(1.5), text_scale::XS),
            ButtonSize::Medium => (spacing(4.0), spacing(1.5), text_scale::SM),
            ButtonSize::Large => (spacing(4.0), spacing(2.0), text_scale::SM),
        };
        let base = div()
            .id(self.id)
            .relative()
            .px(padding_x)
            .py(padding_y)
            .text_scale(scale)
            .child(self.label.clone())
            .child(qa_button(&self.label, &self.label));
        let base = if self.is_full_width {
            base.w_full()
        } else {
            base
        };
        style_button(base, self.variant, self.is_disabled, self.on_click, &theme)
    }
}

/// 글자 없이 아이콘만 있는 버튼. `aria-label`에 해당하는 `label`이 QA·접근성 이름이 된다.
/// 기본 모양은 `rounded-md border border-border-default bg-bg-surface p-1.5 text-text-secondary
/// hover:border-brand-primary hover:text-text-primary`다.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    icon_size: Pixels,
    variant: ButtonVariant,
    is_disabled: bool,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            icon_size: px(16.0),
            variant: ButtonVariant::Outline,
            is_disabled: false,
            on_click: None,
        }
    }

    pub fn icon_size(mut self, icon_size: Pixels) -> Self {
        self.icon_size = icon_size;
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn disabled(mut self, is_disabled: bool) -> Self {
        self.is_disabled = is_disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let colors = self.variant.colors(&theme);
        // GPUI svg는 부모 글자색을 물려받지 않아 currentColor와 호버 색을 아이콘에 직접 건다
        let mut glyph = icon(self.icon, self.icon_size).text_color(colors.text);
        if let Some(hover_text) = colors.hover_text.filter(|_| !self.is_disabled) {
            glyph = glyph.group_hover(ICON_BUTTON_GROUP, move |style| style.text_color(hover_text));
        }
        let base = div()
            .id(self.id)
            .group(ICON_BUTTON_GROUP)
            .relative()
            .p(spacing(1.5))
            .child(glyph)
            .child(qa_button(&self.label, ""));
        style_button(base, self.variant, self.is_disabled, self.on_click, &theme)
    }
}
