//! `/:locale/settings` 설정 화면(TS `SettingsRoute`/`ProjectSettings variant="page"`). M7에서 이 디렉터리 담당자가 채운다.

use gpui_kit::{Context, IntoElement, Render, Window};

use crate::theme::ThemeTokens;
use crate::views::{ScreenContext, route_loading_fallback};

pub struct SettingsView;

impl SettingsView {
    pub fn new(_screen: ScreenContext, _cx: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        route_loading_fallback(cx.global::<ThemeTokens>())
    }
}
