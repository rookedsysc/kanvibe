//! 마우스를 올리면 뜨는 설명. TS는 `title` 속성(브라우저 기본 툴팁)만 썼고 CSS 사양이 없다.
//! 그래서 앱 팝오버 토큰(`rounded-md border border-border-default bg-bg-surface shadow-md`)에 `px-2 py-1 text-xs`로 그린다.
//! 쓰는 법: `div().id(..).tooltip(text_tooltip("설명"))`

use gpui_kit::{
    AnyView, App, AppContext, Context, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, div,
};

use crate::components::{TailwindText, box_shadows};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

pub struct TextTooltip {
    text: SharedString,
}

impl Render for TextTooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<ThemeTokens>();
        div()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.md))
            .px(spacing(2.0))
            .py(spacing(1.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.primary)
            .child(self.text.clone())
    }
}

/// `.tooltip()`에 넘기는 생성 함수
pub fn text_tooltip(
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| cx.new(|_| TextTooltip { text: text.clone() }).into()
}
