//! 한 줄 입력(HTML `<input type="text">`). 편집은 gpui-base의 무장식 입력 상태가 맡고, 색은 앱 테마 토큰으로 칠한다.
//! TS 입력은 모두 `bg-bg-page` 바탕이라 편집기 바탕도 그 색으로 둔다. 자리 표시 글자는 Tailwind preflight처럼
//! 글자색의 50%다.

use gpui_kit::base::input::{Input, InputEditorStyle, InputState};
use gpui_kit::{App, AppContext, Context, Entity, IntoElement, SharedString, Window};

use crate::components::fade;
use crate::theme::ThemeTokens;

/// 입력 상태를 만든다. 값이 바뀌면 `InputEvent::Change`가 나온다
pub fn create_text_field<V: 'static>(
    placeholder: impl Into<SharedString>,
    initial_value: &str,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Entity<InputState> {
    let placeholder = placeholder.into();
    let initial_value = SharedString::from(initial_value.to_string());
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(initial_value)
    })
}

/// 테마 색을 입힌 입력 요소. 바깥 테두리·여백은 호출부가 감싸서 그린다
pub fn themed_text_field(
    state: &Entity<InputState>,
    theme: &ThemeTokens,
    text_color: gpui_kit::Rgba,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let style = InputEditorStyle {
        foreground: text_color.into(),
        muted_foreground: fade(text_color, 0.5).into(),
        background: theme.background.page.into(),
        border: theme.border.default.into(),
        selection: theme.brand.muted.into(),
        caret: text_color.into(),
        ..InputEditorStyle::default()
    };
    state.update(cx, |state, _| state.set_editor_style(style));
    Input::new(state)
}
