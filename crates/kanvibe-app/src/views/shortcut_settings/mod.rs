//! `/:locale/settings/shortcuts` 단축키 설정 화면(TS `ShortcutSettingsRoute`). M7에서 이 디렉터리 담당자가 채운다.

use gpui_kit::{Context, IntoElement, Render, Window};

use crate::theme::ThemeTokens;
use crate::views::{ScreenContext, route_loading_fallback};

pub struct ShortcutSettingsView;

impl ShortcutSettingsView {
    pub fn new(_screen: ScreenContext, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for ShortcutSettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        route_loading_fallback(cx.global::<ThemeTokens>())
    }
}
