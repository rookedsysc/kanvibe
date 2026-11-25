//! `/:locale/settings` 설정 화면(TS `SettingsRoute` + `ProjectSettings variant="page"`).
//! 왼쪽 섹션 nav(240px)와 오른쪽 설정 본문으로 나뉘고, 창 폭이 1024px(`lg`)보다 좁으면 nav가 위쪽 가로 줄로 바뀐다.
//! 모든 값은 AppSettings에 TS와 같은 키·형식으로 저장한다. 저장은 화면 값을 먼저 바꾸고, 실패하면 그 항목만 되돌린 뒤
//! 그 아래에 사유와 재시도를 남긴다.
//!
//! 이 디렉터리의 `controls`·`service_call`·`route_load`·`text_field`는 단축키·AI 계정·pane 레이아웃 화면도 함께 쓴다.

pub mod bulk_session_type;
pub mod controls;
pub mod form;
pub mod local_time;
pub mod mobile_pairing;
pub mod route_load;
pub mod service_call;
pub mod session_type_select;
pub mod settings_store;
pub mod text_field;

pub mod page;

use gpui_kit::base::input::{InputEvent, InputState};
use gpui_kit::{
    App, AppContext, Context, Entity, IntoElement, Render, ScrollHandle, Subscription, Task, Window,
};
use kanvibe_domain::i18n::Translator;

use crate::app_events::{RefreshScope, subscribe_to_refresh};
use crate::services::ServiceResult;
use crate::theme::{ThemePreference, ThemeTokens};
use crate::views::ScreenContext;
use crate::views::settings::controls::set_window_title_once;
use bulk_session_type::BulkSessionTypeChangeSettings;
use form::{SettingChange, SettingKey, SettingsForm, SettingsValues, format_sync_interval_minutes};
use mobile_pairing::MobilePairingSettings;
use route_load::RouteLoadStatus;
use session_type_select::{SessionTypeSelect, SessionTypeSelected};

/// Pane 채팅 기본 모드(TS `PaneChatSettings` + `usePaneChatPreferences`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaneChatPreference {
    pub is_default_enabled: bool,
    pub is_loaded: bool,
    pub is_saving: bool,
    pub has_error: bool,
}

/// 첫 조회에 성공한 뒤 만드는 하위 요소들
struct LoadedSettings {
    form: SettingsForm,
    default_session_type: Entity<SessionTypeSelect>,
    bulk_change: Entity<BulkSessionTypeChangeSettings>,
    mobile_pairing: Entity<MobilePairingSettings>,
    interval_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

pub struct SettingsView {
    screen: ScreenContext,
    /// 창 제목을 걸었는지(TS `document.title`)
    is_title_set: bool,
    load: RouteLoadStatus,
    loaded: Option<LoadedSettings>,
    /// 창이 있어야 하위 요소를 만들 수 있어 첫 조회 결과를 다음 render까지 들고 있는다
    pending_values: Option<SettingsValues>,
    /// 되돌린 주기 값을 다음 render에서 입력란에 다시 쓴다
    pending_interval_text: bool,
    pane_chat: PaneChatPreference,
    scroll_handle: ScrollHandle,
    section_positions: page::SectionPositions,
    _load_task: Option<Task<()>>,
    _requests: Vec<Task<()>>,
    _refresh_subscription: Option<Subscription>,
}

impl SettingsView {
    pub fn new(screen: ScreenContext, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            screen,
            is_title_set: false,
            load: RouteLoadStatus::default(),
            loaded: None,
            pending_values: None,
            pending_interval_text: false,
            pane_chat: PaneChatPreference::default(),
            scroll_handle: ScrollHandle::new(),
            section_positions: page::SectionPositions::default(),
            _load_task: None,
            _requests: Vec::new(),
            _refresh_subscription: subscribe_to_refresh(
                &[RefreshScope::Settings],
                cx,
                Self::reload,
            ),
        };
        view.reload(cx);
        view
    }

    /// 화면이 그리는 설정 값과 저장 상태. 첫 조회 전에는 `None`
    pub fn form(&self) -> Option<&SettingsForm> {
        self.loaded.as_ref().map(|loaded| &loaded.form)
    }

    /// 세션 타입 일괄 변경 행. 첫 조회 전에는 없다
    pub fn bulk_session_type_change(&self) -> Option<Entity<BulkSessionTypeChangeSettings>> {
        self.loaded
            .as_ref()
            .map(|loaded| loaded.bulk_change.clone())
    }

    pub fn load_status(&self) -> &RouteLoadStatus {
        &self.load
    }

    pub fn pane_chat_preference(&self) -> PaneChatPreference {
        self.pane_chat
    }

    fn translator(&self, namespace: &'static str) -> Translator {
        Translator::new(self.screen.route.locale, namespace)
    }

    /// 화면 데이터와 pane 채팅 설정을 (다시) 읽는다(TS 새로 고침 신호는 둘 다 다시 읽는다)
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.load_route_data(cx);
        self.load_pane_chat_preference(cx);
    }

    /// 이미 데이터가 있으면 실패해도 지우지 않고 갱신 실패 배너만 보인다
    fn load_route_data(&mut self, cx: &mut Context<Self>) {
        let attempt = self.load.begin(cx, |view: &mut Self| &mut view.load);
        let request = settings_store::load_settings_values(cx);
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| this.finish_load(attempt, result, cx));
        }));
    }

    /// 재시도는 화면 데이터만 다시 읽는다(TS `retryLoad`)
    pub fn retry_load(&mut self, cx: &mut Context<Self>) {
        self.load.clear_failure();
        self.load_route_data(cx);
        cx.notify();
    }

    fn finish_load(
        &mut self,
        attempt: u64,
        result: ServiceResult<SettingsValues>,
        cx: &mut Context<Self>,
    ) {
        if !self.load.is_current(attempt) {
            return;
        }
        match result {
            Ok(values) => {
                self.load.succeed();
                self.apply_loaded_values(values, cx);
            }
            Err(failure) => self.load.fail(failure.display_message()),
        }
        cx.notify();
    }

    fn apply_loaded_values(&mut self, values: SettingsValues, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.as_mut() else {
            self.pending_values = Some(values);
            return;
        };
        loaded.form.apply_refresh(&values);
        let session_type = loaded.form.values.default_session_type;
        loaded
            .default_session_type
            .update(cx, |select, cx| select.set_selected(session_type, cx));
    }

    fn load_pane_chat_preference(&mut self, cx: &mut Context<Self>) {
        let request = settings_store::load_pane_chat_default_enabled(cx);
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(is_enabled) => {
                        this.pane_chat.is_default_enabled = is_enabled;
                        this.pane_chat.is_loaded = true;
                        this.pane_chat.has_error = false;
                    }
                    Err(_) => this.pane_chat.has_error = true,
                }
                cx.notify();
            });
        }));
    }

    fn track(&mut self, task: Task<()>) {
        self._requests.retain(|request| !request.is_ready());
        self._requests.push(task);
    }
}

/// 첫 조회 결과를 하위 요소와 함께 화면에 붙인다(창이 있어야 입력 상태를 만들 수 있어 render에서 부른다)
impl SettingsView {
    fn build_loaded(
        &mut self,
        values: SettingsValues,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> LoadedSettings {
        let locale = self.screen.route.locale;
        let session_type = values.default_session_type;
        let default_session_type =
            cx.new(|cx| SessionTypeSelect::new("default-session-type", session_type, cx));
        let bulk_change = cx.new(|cx| BulkSessionTypeChangeSettings::new(locale, session_type, cx));
        let mobile_pairing = cx.new(|cx| MobilePairingSettings::new(locale, cx));
        let interval_input = text_field::create_text_field(
            "",
            &format_sync_interval_minutes(values.background_sync.interval_ms),
            window,
            cx,
        );
        let subscriptions = vec![
            cx.subscribe(
                &default_session_type,
                |this, _, event: &SessionTypeSelected, cx| {
                    this.save_setting(SettingChange::DefaultSessionType(event.0), cx);
                },
            ),
            cx.subscribe_in(&interval_input, window, Self::handle_interval_input),
        ];
        LoadedSettings {
            form: SettingsForm::new(values),
            default_session_type,
            bulk_change,
            mobile_pairing,
            interval_input,
            _subscriptions: subscriptions,
        }
    }

    fn handle_interval_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded.as_mut() else {
            return;
        };
        match event {
            InputEvent::Change => {
                let text = input.read(cx).value().to_string();
                if let Some(change) = loaded.form.edit_interval_input(&text) {
                    self.save_interval(change, cx);
                }
            }
            InputEvent::Blur => {
                loaded.form.blur_interval_input();
                let text = loaded.form.interval_input.clone();
                if input.read(cx).value().as_ref() != text {
                    input.update(cx, |input, cx| input.set_value(text, window, cx));
                }
            }
            InputEvent::PressEnter { .. } | InputEvent::Focus => {}
        }
    }
}

/// 저장
impl SettingsView {
    /// 화면 값을 먼저 바꾸고 저장한다. 실패하면 그 항목만 되돌리고 실패를 남긴다
    pub fn save_setting(&mut self, change: SettingChange, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.as_mut() else {
            return;
        };
        let pending = loaded.form.begin_save(change.clone());
        if let SettingChange::Theme(preference) = change {
            apply_theme_preference(preference, cx);
        }
        let request = settings_store::save_setting_change(&pending.change, cx);
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_save(
                    pending,
                    result.map_err(|failure| failure.display_message()),
                    cx,
                )
            });
        }));
        cx.notify();
    }

    fn finish_save(
        &mut self,
        pending: form::PendingSave,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded.as_mut() else {
            return;
        };
        let key = pending.change.key();
        let is_restored = loaded.form.finish_save(pending, result);
        if is_restored {
            self.sync_restored_controls(key, cx);
        }
        cx.notify();
    }

    /// 되돌린 값을 값 사본을 따로 들고 있는 요소(테마, select, 주기 입력란)에 다시 건다
    fn sync_restored_controls(&mut self, key: SettingKey, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        match key {
            SettingKey::Theme => apply_theme_preference(loaded.form.values.theme_preference, cx),
            SettingKey::SessionType => {
                let session_type = loaded.form.values.default_session_type;
                loaded
                    .default_session_type
                    .update(cx, |select, cx| select.set_selected(session_type, cx));
            }
            SettingKey::BackgroundSync => self.pending_interval_text = true,
            _ => {}
        }
    }

    /// 주기 저장은 앞 저장이 끝난 뒤 최신 값을 차례로 보낸다
    fn save_interval(&mut self, change: SettingChange, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.as_mut() else {
            return;
        };
        let pending = loaded.form.begin_save(change);
        if let Some(next) = loaded.form.enqueue_interval_save(pending) {
            self.send_interval_save(next, cx);
        }
        cx.notify();
    }

    fn send_interval_save(&mut self, pending: form::PendingSave, cx: &mut Context<Self>) {
        let request = settings_store::save_setting_change(&pending.change, cx);
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_save(
                    pending,
                    result.map_err(|failure| failure.display_message()),
                    cx,
                );
                let next = this
                    .loaded
                    .as_mut()
                    .and_then(|loaded| loaded.form.finish_interval_save());
                if let Some(next) = next {
                    this.send_interval_save(next, cx);
                }
            });
        }));
    }

    /// 실패한 변경을 다시 보낸다
    pub fn retry_failed_save(&mut self, cx: &mut Context<Self>) {
        let failure = self
            .loaded
            .as_ref()
            .and_then(|loaded| loaded.form.failure.clone());
        if let Some(failure) = failure {
            self.save_setting(failure.change, cx);
        }
    }

    pub fn set_pane_chat_default_enabled(&mut self, is_enabled: bool, cx: &mut Context<Self>) {
        self.pane_chat.is_saving = true;
        self.pane_chat.has_error = false;
        let request = settings_store::save_pane_chat_default_enabled(is_enabled, cx);
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.pane_chat.is_saving = false;
                match result {
                    Ok(()) => this.pane_chat.is_default_enabled = is_enabled,
                    Err(_) => this.pane_chat.has_error = true,
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

/// 고른 테마를 앱 전체(열린 창 모두)에 바로 적용한다(TS `applyThemePreference` + `notifyThemePreferenceChanged`)
pub fn apply_theme_preference(preference: ThemePreference, cx: &mut App) {
    let appearance = cx.window_appearance();
    let is_system_light = matches!(
        appearance,
        gpui_kit::WindowAppearance::Light | gpui_kit::WindowAppearance::VibrantLight
    );
    cx.set_global(ThemeTokens::for_mode(preference.resolve(is_system_light)));
    cx.refresh_windows();
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        set_window_title_once(&mut self.is_title_set, "Settings", window);
        if let Some(values) = self.pending_values.take() {
            let loaded = self.build_loaded(values, window, cx);
            self.loaded = Some(loaded);
        }
        if std::mem::take(&mut self.pending_interval_text) {
            self.sync_interval_text(window, cx);
        }
        page::render_settings_page(self, window, cx)
    }
}

impl SettingsView {
    fn sync_interval_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let text = loaded.form.interval_input.clone();
        loaded
            .interval_input
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }
}
