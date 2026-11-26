//! 채팅 표시 설정(TS `usePaneChatPreferences`). 앱 전역 설정이라 AppSettings에 두고, 설정 화면의 새로 고침 신호를 받으면 다시 읽는다.

use gpui_kit::{Context, Subscription, Task};

use super::client::PaneChatClient;
use crate::app_events::{RefreshScope, subscribe_to_refresh};

/// 새 pane을 채팅으로 열지(AppSettings `pane_chat_default_enabled`)
pub const PANE_CHAT_DEFAULT_KEY: &str = "pane_chat_default_enabled";
/// 시스템·자동 지침 메시지를 대화에 보일지(AppSettings `pane_chat_show_instructions`)
pub const PANE_CHAT_INSTRUCTIONS_KEY: &str = "pane_chat_show_instructions";

pub struct PaneChatPreferences {
    client: PaneChatClient,
    pub is_default_enabled: bool,
    pub should_show_instructions: bool,
    pub is_loaded: bool,
    pub has_error: bool,
    pub is_saving: bool,
    _load: Option<Task<()>>,
    _save: Option<Task<()>>,
    _refresh: Option<Subscription>,
}

impl PaneChatPreferences {
    pub fn new(client: PaneChatClient, cx: &mut Context<Self>) -> Self {
        let refresh = subscribe_to_refresh(&[RefreshScope::Settings], cx, |preferences, cx| {
            preferences.load(cx)
        });
        let mut preferences = Self {
            client,
            is_default_enabled: false,
            should_show_instructions: false,
            is_loaded: false,
            has_error: false,
            is_saving: false,
            _load: None,
            _save: None,
            _refresh: refresh,
        };
        preferences.load(cx);
        preferences
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let enabled = self.client.get_app_setting(PANE_CHAT_DEFAULT_KEY, cx);
        let instructions = self.client.get_app_setting(PANE_CHAT_INSTRUCTIONS_KEY, cx);
        self._load = Some(cx.spawn(async move |this, cx| {
            let enabled = enabled.await;
            let instructions = instructions.await;
            let _ = this.update(cx, |preferences, cx| {
                match (enabled, instructions) {
                    (Ok(enabled), Ok(instructions)) => {
                        preferences.is_default_enabled = enabled.as_deref() == Some("true");
                        preferences.should_show_instructions =
                            instructions.as_deref() == Some("true");
                        preferences.is_loaded = true;
                        preferences.has_error = false;
                    }
                    _ => preferences.has_error = true,
                }
                cx.notify();
            });
        }));
    }

    pub fn set_show_instructions(&mut self, should_show: bool, cx: &mut Context<Self>) {
        self.is_saving = true;
        self.has_error = false;
        let value = if should_show { "true" } else { "false" };
        let save = self
            .client
            .set_app_setting(PANE_CHAT_INSTRUCTIONS_KEY, value, cx);
        self._save = Some(cx.spawn(async move |this, cx| {
            let result = save.await;
            let _ = this.update(cx, |preferences, cx| {
                match result {
                    Ok(()) => preferences.should_show_instructions = should_show,
                    Err(_) => preferences.has_error = true,
                }
                preferences.is_saving = false;
                cx.notify();
            });
        }));
        cx.notify();
    }
}
