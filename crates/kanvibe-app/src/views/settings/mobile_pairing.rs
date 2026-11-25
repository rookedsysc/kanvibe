//! 모바일 앱 연결 설정(TS `MobilePairingSettings`). 코드는 화면에 떠 있는 동안에만 살아 있고 한 번 쓰면 사라지므로
//! 화면을 벗어나면 거둔다. 한 번 연결한 기기는 여기서 직접 끊기 전까지 목록에 남는다.
//!
//! 기기 목록·페어링 코드는 프로세스 안 서버와 같은 kanvibe-mobile `MobileDevices` 인스턴스(`AppServices::mobile_devices`)가 갖는다.
//! 화면은 `MobilePairingBridge` 뒤에서만 부르고, 테스트는 같은 자리에 가짜 통로를 넘긴다.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    App, Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, Task, Window,
    div, px,
};
use kanvibe_ai::javascript::parse_date_ms;
use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_mobile::devices::MobileDevices;
use kanvibe_process::hooks::endpoint::HookServerEndpoint;
use serde::{Deserialize, Serialize};

use crate::components::TailwindText;
use crate::services::{AppServices, ServiceResult};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::controls::{
    INLINE_TEXT_SM, inline_load_error, keyed_id, make_pressable, native_checkbox, pressable,
    section_heading,
};
use crate::views::settings::local_time::LocalDateTime;
use crate::views::settings::service_call::{call_service, failure};

/// 남은 시간을 1초마다 다시 그린다
const COUNTDOWN_TICK: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MobilePairingCode {
    pub code: String,
    /// 만료 시각(epoch ms)
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedMobileDevice {
    pub device_id: String,
    pub device_name: String,
    /// ISO 8601 시각
    pub paired_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileConnectionSettings {
    pub mobile_tls_port: u16,
    pub is_lan_plaintext_allowed: bool,
}

/// 화면이 페어링 서버에 바라는 것(TS IPC `mobileBridge.*`)
pub trait MobilePairingBridge: Send + Sync {
    fn start_mobile_pairing(&self) -> ServiceResult<MobilePairingCode>;
    fn stop_mobile_pairing(&self) -> ServiceResult<()>;
    /// 데스크탑이 아직 들고 있는 코드. 만료·실패 상한으로 버렸으면 `None`
    fn read_mobile_pairing_code(&self) -> ServiceResult<Option<String>>;
    fn list_paired_mobile_devices(&self) -> ServiceResult<Vec<PairedMobileDevice>>;
    fn unpair_mobile_device(&self, device_id: &str) -> ServiceResult<()>;
    fn read_mobile_connection_settings(&self) -> ServiceResult<MobileConnectionSettings>;
    fn update_lan_plaintext_allowed(&self, is_allowed: bool) -> ServiceResult<()>;
}

/// 프로세스 안 서버의 기기 계층으로 답하는 통로. 데이터 디렉터리 없이 뜬 실행이면 모든 호출이 실패한다
fn desktop_pairing_bridge(cx: &App) -> Arc<dyn MobilePairingBridge> {
    let services = cx.global::<AppServices>();
    let (Some(devices), Some(runtime)) = (services.mobile_devices(), services.runtime()) else {
        return Arc::new(UnavailablePairingBridge);
    };
    Arc::new(DesktopPairingBridge {
        devices: devices.clone(),
        database: runtime.database().clone(),
        platform: services.shortcut_platform,
        endpoint: HookServerEndpoint::new(runtime.hook_server_address().port()),
    })
}

/// 서버가 띄운 페어링과 같은 `MobileDevices` 인스턴스라, 여기서 해제한 기기의 소켓은 서버가 닫는다.
/// 포트 안내는 hook 서버 포트 기준이고 LAN 평문 허용은 AppSettings에 저장한다(TS `mobileBridgeService`)
struct DesktopPairingBridge {
    devices: Arc<MobileDevices>,
    database: Arc<Database>,
    platform: ShortcutPlatform,
    endpoint: HookServerEndpoint,
}

impl DesktopPairingBridge {
    fn settings(&self) -> AppSettingsService<'_> {
        AppSettingsService::new(&self.database, self.platform)
    }
}

impl MobilePairingBridge for DesktopPairingBridge {
    fn start_mobile_pairing(&self) -> ServiceResult<MobilePairingCode> {
        let issued = self.devices.start_mobile_pairing();
        Ok(MobilePairingCode {
            code: issued.code,
            expires_at: issued.expires_at,
        })
    }

    fn stop_mobile_pairing(&self) -> ServiceResult<()> {
        self.devices.stop_mobile_pairing();
        Ok(())
    }

    fn read_mobile_pairing_code(&self) -> ServiceResult<Option<String>> {
        Ok(self.devices.read_mobile_pairing_code())
    }

    fn list_paired_mobile_devices(&self) -> ServiceResult<Vec<PairedMobileDevice>> {
        let devices = self.devices.list_paired_mobile_devices().map_err(failure)?;
        Ok(devices
            .into_iter()
            .map(|device| PairedMobileDevice {
                device_id: device.device_id,
                device_name: device.device_name,
                paired_at: device.paired_at,
            })
            .collect())
    }

    fn unpair_mobile_device(&self, device_id: &str) -> ServiceResult<()> {
        self.devices
            .unpair_mobile_device(device_id)
            .map_err(failure)
    }

    fn read_mobile_connection_settings(&self) -> ServiceResult<MobileConnectionSettings> {
        Ok(MobileConnectionSettings {
            mobile_tls_port: self.endpoint.mobile_tls_port(),
            is_lan_plaintext_allowed: self
                .settings()
                .get_lan_plaintext_allowed()
                .map_err(failure)?,
        })
    }

    fn update_lan_plaintext_allowed(&self, is_allowed: bool) -> ServiceResult<()> {
        self.settings()
            .set_lan_plaintext_allowed(is_allowed)
            .map_err(failure)
    }
}

/// 데이터 디렉터리 없이 뜬 앱. 서버가 없으므로 모든 호출이 사유와 함께 실패한다
struct UnavailablePairingBridge;

const DATABASE_UNAVAILABLE: &str = "데이터베이스가 열려 있지 않습니다";

impl MobilePairingBridge for UnavailablePairingBridge {
    fn start_mobile_pairing(&self) -> ServiceResult<MobilePairingCode> {
        Err(failure(DATABASE_UNAVAILABLE))
    }

    fn stop_mobile_pairing(&self) -> ServiceResult<()> {
        Err(failure(DATABASE_UNAVAILABLE))
    }

    fn read_mobile_pairing_code(&self) -> ServiceResult<Option<String>> {
        Err(failure(DATABASE_UNAVAILABLE))
    }

    fn list_paired_mobile_devices(&self) -> ServiceResult<Vec<PairedMobileDevice>> {
        Err(failure(DATABASE_UNAVAILABLE))
    }

    fn unpair_mobile_device(&self, _: &str) -> ServiceResult<()> {
        Err(failure(DATABASE_UNAVAILABLE))
    }

    fn read_mobile_connection_settings(&self) -> ServiceResult<MobileConnectionSettings> {
        Err(failure(DATABASE_UNAVAILABLE))
    }

    fn update_lan_plaintext_allowed(&self, _: bool) -> ServiceResult<()> {
        Err(failure(DATABASE_UNAVAILABLE))
    }
}

/// 진행 중인 연결 설정 작업. 기기 해제는 어느 기기인지까지 들고 있어 그 행에만 진행을 보인다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingAction {
    IssueCode,
    ClearCode,
    LanPlaintext,
    Disconnect(String),
}

/// 화면이 그리는 상태. 서비스 호출 없이 결과만 받아 바뀐다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MobilePairingState {
    pub code: Option<String>,
    pub expires_at_ms: Option<i64>,
    pub remaining_seconds: i64,
    pub devices: Vec<PairedMobileDevice>,
    pub failure_message: Option<String>,
    pub connection: Option<MobileConnectionSettings>,
    pub pending_action: Option<PairingAction>,
    pub is_device_list_unreadable: bool,
    /// 화면을 떠날 때 거둘 코드가 있는지
    pub has_outstanding_code: bool,
}

impl MobilePairingState {
    /// 한 번에 하나만 보낸다. 이미 진행 중이면 `false`
    pub fn begin_action(&mut self, action: PairingAction) -> bool {
        if self.pending_action.is_some() {
            return false;
        }
        self.pending_action = Some(action);
        self.failure_message = None;
        true
    }

    pub fn finish_action(&mut self, failure_message: Option<String>) {
        self.pending_action = None;
        self.failure_message = failure_message;
    }

    pub fn show_code(&mut self, pairing: MobilePairingCode, now_ms: i64) {
        self.has_outstanding_code = true;
        self.code = Some(pairing.code);
        self.expires_at_ms = Some(pairing.expires_at);
        self.tick(now_ms);
    }

    /// 데스크탑에서 이미 죽은 코드라 거둘 것이 없다
    pub fn forget_code(&mut self) {
        self.has_outstanding_code = false;
        self.code = None;
        self.expires_at_ms = None;
    }

    /// 남은 시간을 다시 잰다. 만료됐으면 코드를 잊고 `false`
    pub fn tick(&mut self, now_ms: i64) -> bool {
        let Some(expires_at_ms) = self.expires_at_ms else {
            return false;
        };
        let remaining_ms = (expires_at_ms - now_ms).max(0);
        self.remaining_seconds = (remaining_ms + 999) / 1000;
        if self.remaining_seconds == 0 {
            self.forget_code();
            return false;
        }
        true
    }

    /// 기기 목록을 못 읽으면 이전 목록은 두고 못 읽었다는 표시만 켠다
    pub fn apply_device_list(&mut self, result: ServiceResult<Vec<PairedMobileDevice>>) {
        match result {
            Ok(devices) => {
                self.devices = devices;
                self.is_device_list_unreadable = false;
            }
            Err(_) => self.is_device_list_unreadable = true,
        }
    }

    pub fn is_action_pending(&self) -> bool {
        self.pending_action.is_some()
    }
}

/// `new Date(iso).toLocaleString()`. 현지 시간대의 `M/D/YYYY, h:mm:ss AM`(en-US) 모양이다
pub fn format_paired_at(iso: &str) -> String {
    let Some(local) = parse_date_ms(iso).and_then(LocalDateTime::from_epoch_ms) else {
        return iso.to_string();
    };
    format!(
        "{}/{}/{}, {}",
        local.month,
        local.day,
        local.year,
        local.twelve_hour_second()
    )
}

pub struct MobilePairingSettings {
    locale: Locale,
    pub state: MobilePairingState,
    bridge: Arc<dyn MobilePairingBridge>,
    _countdown: Option<Task<()>>,
    _requests: Vec<Task<()>>,
}

impl MobilePairingSettings {
    pub fn new(locale: Locale, cx: &mut Context<Self>) -> Self {
        let bridge = desktop_pairing_bridge(cx);
        Self::with_bridge(locale, bridge, cx)
    }

    /// 페어링 통로를 정해 연다. 앱은 프로세스 안 서버 통로를, 테스트는 가짜 통로를 넘긴다
    pub fn with_bridge(
        locale: Locale,
        bridge: Arc<dyn MobilePairingBridge>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut settings = Self {
            locale,
            state: MobilePairingState::default(),
            bridge,
            _countdown: None,
            _requests: Vec::new(),
        };
        settings.refresh_paired_devices(cx);
        settings.refresh_connection_settings(cx);
        cx.on_release(|this, cx| this.withdraw_outstanding_code(cx))
            .detach();
        settings
    }

    fn settings_text(&self, key: &str) -> String {
        Translator::new(self.locale, "settings").translate(key)
    }

    fn track(&mut self, task: Task<()>) {
        self._requests.retain(|request| !request.is_ready());
        self._requests.push(task);
    }

    pub fn refresh_paired_devices(&mut self, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        let request = call_service(
            cx,
            "mobileBridge.listPairedMobileDevices",
            "[]".into(),
            move || bridge.list_paired_mobile_devices(),
        );
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.state.apply_device_list(result);
                cx.notify();
            });
        }));
    }

    /// 포트 안내와 평문 설정은 부가 정보이므로 못 읽으면 그 부분만 숨긴다
    fn refresh_connection_settings(&mut self, cx: &mut Context<Self>) {
        let bridge = self.bridge.clone();
        let request = call_service(
            cx,
            "mobileBridge.readMobileConnectionSettings",
            "[]".into(),
            move || bridge.read_mobile_connection_settings(),
        );
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.state.connection = result.ok();
                cx.notify();
            });
        }));
    }

    /// 화면을 떠날 때 띄워 둔 코드를 거둔다. 실패를 보일 곳이 없어 결과는 버린다
    fn withdraw_outstanding_code(&mut self, cx: &mut App) {
        if !std::mem::take(&mut self.state.has_outstanding_code) {
            return;
        }
        let bridge = self.bridge.clone();
        call_service(
            cx,
            "mobileBridge.stopMobilePairing",
            "[]".into(),
            move || bridge.stop_mobile_pairing(),
        )
        .detach();
    }

    pub fn issue_code(&mut self, cx: &mut Context<Self>) {
        if !self.state.begin_action(PairingAction::IssueCode) {
            return;
        }
        let bridge = self.bridge.clone();
        let request = call_service(
            cx,
            "mobileBridge.startMobilePairing",
            "[]".into(),
            move || bridge.start_mobile_pairing(),
        );
        let failure_text = self.settings_text("mobilePairingIssueCodeFailed");
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(pairing) => {
                        let now_ms = cx.global::<AppServices>().clock.now_ms();
                        this.state.show_code(pairing, now_ms);
                        this.start_countdown(cx);
                        this.refresh_paired_devices(cx);
                        this.state.finish_action(None);
                    }
                    Err(_) => this.state.finish_action(Some(failure_text)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn clear_code(&mut self, cx: &mut Context<Self>) {
        if !self.state.begin_action(PairingAction::ClearCode) {
            return;
        }
        let bridge = self.bridge.clone();
        let request = call_service(
            cx,
            "mobileBridge.stopMobilePairing",
            "[]".into(),
            move || bridge.stop_mobile_pairing(),
        );
        let failure_text = self.settings_text("mobilePairingClearCodeFailed");
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                if result.is_ok() {
                    this.state.forget_code();
                    this._countdown = None;
                }
                this.state.finish_action(result.err().map(|_| failure_text));
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn disconnect_device(&mut self, device_id: String, cx: &mut Context<Self>) {
        if !self
            .state
            .begin_action(PairingAction::Disconnect(device_id.clone()))
        {
            return;
        }
        let bridge = self.bridge.clone();
        let arguments = serde_json::json!([device_id]).to_string();
        let request = call_service(
            cx,
            "mobileBridge.unpairMobileDevice",
            arguments,
            move || bridge.unpair_mobile_device(&device_id),
        );
        let failure_text = self.settings_text("mobilePairingDisconnectFailed");
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                if result.is_ok() {
                    this.refresh_paired_devices(cx);
                }
                this.state.finish_action(result.err().map(|_| failure_text));
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn change_lan_plaintext_allowed(&mut self, is_allowed: bool, cx: &mut Context<Self>) {
        if !self.state.begin_action(PairingAction::LanPlaintext) {
            return;
        }
        let bridge = self.bridge.clone();
        let request = call_service(
            cx,
            "mobileBridge.updateLanPlaintextAllowed",
            format!("[{is_allowed}]"),
            move || bridge.update_lan_plaintext_allowed(is_allowed),
        );
        let failure_text = self.settings_text("mobileLanPlaintextUpdateFailed");
        self.track(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                if result.is_ok()
                    && let Some(connection) = this.state.connection.as_mut()
                {
                    connection.is_lan_plaintext_allowed = is_allowed;
                }
                this.state.finish_action(result.err().map(|_| failure_text));
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 1초마다 남은 시간을 다시 재고, 데스크탑이 코드를 버렸는지 확인한다
    fn start_countdown(&mut self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        self._countdown = Some(cx.spawn(async move |this, cx| {
            loop {
                executor.timer(COUNTDOWN_TICK).await;
                let should_continue = this
                    .update(cx, |this, cx| this.tick_countdown(cx))
                    .unwrap_or(false);
                if !should_continue {
                    break;
                }
            }
        }));
    }

    fn tick_countdown(&mut self, cx: &mut Context<Self>) -> bool {
        let now_ms = cx.global::<AppServices>().clock.now_ms();
        let is_alive = self.state.tick(now_ms);
        cx.notify();
        if !is_alive {
            return false;
        }
        let bridge = self.bridge.clone();
        let request = call_service(
            cx,
            "mobileBridge.readMobilePairingCode",
            "[]".into(),
            move || bridge.read_mobile_pairing_code(),
        );
        self.track(cx.spawn(async move |this, cx| {
            if let Ok(None) = request.await {
                let _ = this.update(cx, |this, cx| {
                    this.state.forget_code();
                    this._countdown = None;
                    cx.notify();
                });
            }
        }));
        true
    }
}

impl Render for MobilePairingSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let settings = Translator::new(self.locale, "settings");
        let common = Translator::new(self.locale, "common");
        div()
            .p(spacing(4.0))
            .border_b_1()
            .border_color(theme.border.default)
            .child(section_heading(
                &theme,
                settings.translate("mobilePairingSection"),
            ))
            .child(self.render_header(&theme, &settings, cx))
            .children(self.render_code(&theme, &settings))
            .children(self.state.failure_message.clone().map(|message| {
                div()
                    .mt(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.status.error)
                    .child(message)
            }))
            .children(self.state.is_device_list_unreadable.then(|| {
                inline_load_error(
                    &theme,
                    "mobile-devices-retry",
                    settings.translate("mobilePairingDevicesLoadFailed"),
                    common.translate("retry"),
                    cx.listener(|this, _, _, cx| this.refresh_paired_devices(cx)),
                )
                .mt(spacing(3.0))
            }))
            .children(self.render_devices(&theme, &settings, cx))
            .children(self.render_lan_plaintext(&theme, &settings, cx))
    }
}

impl MobilePairingSettings {
    fn render_header(
        &self,
        theme: &ThemeTokens,
        settings: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let port_line = self.state.connection.map(|connection| {
            div().mt(px(2.0)).child(settings.translate_with(
                "mobilePairingPort",
                &[("port", &connection.mobile_tls_port.to_string())],
            ))
        });
        div()
            .flex()
            .items_start()
            .justify_between()
            .gap(spacing(4.0))
            .child(
                div()
                    .min_w_0()
                    .child(
                        div()
                            .text_scale(INLINE_TEXT_SM)
                            .text_color(theme.text.primary)
                            .child(settings.translate("mobilePairing")),
                    )
                    .child(
                        div()
                            .text_scale(text_scale::XS)
                            .text_color(theme.text.muted)
                            .child(
                                div()
                                    .mt(px(2.0))
                                    .child(settings.translate("mobilePairingDescription")),
                            )
                            .children(port_line),
                    ),
            )
            .child(self.render_code_button(theme, settings, cx))
    }

    /// 코드가 없으면 brand "연결 코드 만들기", 있으면 neutral "코드 지우기"(`shrink-0 px-3 py-1.5 text-sm rounded-md`)
    fn render_code_button(
        &self,
        theme: &ThemeTokens,
        settings: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pending = self.state.pending_action.as_ref();
        let is_code_shown = self.state.code.is_some();
        let (label, background, hover) = if is_code_shown {
            let label = if pending == Some(&PairingAction::ClearCode) {
                "mobilePairingClearingCode"
            } else {
                "mobilePairingClearCode"
            };
            (label, theme.button_neutral.base, theme.button_neutral.hover)
        } else {
            let label = if pending == Some(&PairingAction::IssueCode) {
                "mobilePairingIssuingCode"
            } else {
                "mobilePairingIssueCode"
            };
            (label, theme.brand.primary, theme.brand.hover)
        };
        let label = settings.translate(label);
        let button = pressable("mobile-code-button", "button", label.clone(), label.clone())
            .flex_none()
            .px(spacing(3.0))
            .py(spacing(1.5))
            .text_scale(text_scale::SM)
            .rounded(radius::MD)
            .bg(background)
            .text_color(gpui_kit::white())
            .hover(move |style| style.bg(hover))
            .child(label);
        make_pressable(
            button,
            theme,
            self.state.is_action_pending(),
            cx.listener(move |this, _, _, cx| {
                if is_code_shown {
                    this.clear_code(cx);
                } else {
                    this.issue_code(cx);
                }
            }),
        )
    }

    /// `mt-3 p-3 rounded-md bg-bg-page border flex items-center justify-between`, 코드는 `text-2xl font-mono tracking-[0.3em]`
    fn render_code(&self, theme: &ThemeTokens, settings: &Translator) -> Option<impl IntoElement> {
        let code = self.state.code.clone()?;
        Some(
            div()
                .mt(spacing(3.0))
                .p(spacing(3.0))
                .flex()
                .items_center()
                .justify_between()
                .rounded(radius::MD)
                .bg(theme.background.page)
                .border_1()
                .border_color(theme.border.default)
                .child(render_tracked_code(theme, code))
                .child(
                    div()
                        .text_scale(text_scale::XS)
                        .text_color(theme.text.muted)
                        .child(settings.translate_with(
                            "mobilePairingCodeExpiresIn",
                            &[("seconds", &self.state.remaining_seconds.to_string())],
                        )),
                ),
        )
    }

    fn render_devices(
        &self,
        theme: &ThemeTokens,
        settings: &Translator,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if self.state.devices.is_empty() {
            return None;
        }
        let mut rows = Vec::new();
        for device in &self.state.devices {
            rows.push(
                self.render_device(theme, settings, device, cx)
                    .into_any_element(),
            );
        }
        Some(
            div()
                .mt(spacing(3.0))
                .flex()
                .flex_col()
                .gap(spacing(1.0))
                .children(rows),
        )
    }

    fn render_device(
        &self,
        theme: &ThemeTokens,
        settings: &Translator,
        device: &PairedMobileDevice,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let paired_at = settings.translate_with(
            "mobilePairingPairedAt",
            &[("date", &format_paired_at(&device.paired_at))],
        );
        div()
            .flex()
            .items_center()
            .justify_between()
            .px(spacing(3.0))
            .py(spacing(2.0))
            .rounded(radius::MD)
            .bg(theme.background.page)
            .border_1()
            .border_color(theme.border.default)
            .child(
                div()
                    .child(
                        div()
                            .text_scale(text_scale::SM)
                            .text_color(theme.text.primary)
                            .child(device.device_name.clone()),
                    )
                    .child(
                        div()
                            .mt(px(2.0))
                            .text_scale(text_scale::XS)
                            .text_color(theme.text.muted)
                            .child(paired_at),
                    ),
            )
            .child(self.render_disconnect_button(theme, settings, device, cx))
    }

    /// `px-2 py-1 text-xs rounded-md text-status-error hover:bg-button-neutral-subtle`
    fn render_disconnect_button(
        &self,
        theme: &ThemeTokens,
        settings: &Translator,
        device: &PairedMobileDevice,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let is_disconnecting =
            self.state.pending_action == Some(PairingAction::Disconnect(device.device_id.clone()));
        let label_key = if is_disconnecting {
            "mobilePairingDisconnecting"
        } else {
            "mobilePairingDisconnect"
        };
        let label = settings.translate(label_key);
        let hover = theme.button_neutral.subtle;
        let device_id = device.device_id.clone();
        let button = pressable(
            keyed_id("mobile-disconnect", &device.device_id),
            "button",
            label.clone(),
            label.clone(),
        )
        .px(spacing(2.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .rounded(radius::MD)
        .text_color(theme.status.error)
        .hover(move |style| style.bg(hover))
        .child(label);
        make_pressable(
            button,
            theme,
            self.state.is_action_pending(),
            cx.listener(move |this, _, _, cx| this.disconnect_device(device_id.clone(), cx)),
        )
    }

    /// `mt-3 flex items-start justify-between gap-3` + 기본 체크박스(`mt-1`)
    fn render_lan_plaintext(
        &self,
        theme: &ThemeTokens,
        settings: &Translator,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let connection = self.state.connection?;
        let label = settings.translate("mobileLanPlaintextAllow");
        let is_allowed = connection.is_lan_plaintext_allowed;
        Some(
            div()
                .mt(spacing(3.0))
                .flex()
                .items_start()
                .justify_between()
                .gap(spacing(3.0))
                .child(
                    div()
                        .min_w_0()
                        .child(
                            div()
                                .text_scale(INLINE_TEXT_SM)
                                .text_color(theme.text.primary)
                                .child(label.clone()),
                        )
                        .child(
                            div()
                                .mt(px(2.0))
                                .text_scale(text_scale::XS)
                                .text_color(theme.text.muted)
                                .child(settings.translate("mobileLanPlaintextDescription")),
                        ),
                )
                .child(
                    native_checkbox(
                        theme,
                        "mobile-lan-plaintext",
                        label,
                        13.0,
                        is_allowed,
                        self.state.is_action_pending(),
                        cx.listener(move |this, _, _, cx| {
                            this.change_lan_plaintext_allowed(!is_allowed, cx)
                        }),
                    )
                    .mt(spacing(1.0)),
                ),
        )
    }
}

/// `text-2xl font-mono tracking-[0.3em]`. GPUI에는 자간이 없어 글자마다 0.3em(7.2px) 뒤 여백을 둔다.
/// 재생 드라이버는 코드 글자 그대로(`482913`) 찾는다
fn render_tracked_code(theme: &ThemeTokens, code: String) -> impl IntoElement {
    let letter_spacing = px(24.0 * 0.3);
    div()
        .relative()
        .flex()
        .text_scale(text_scale::XXL)
        .mono(400)
        .text_color(theme.text.primary)
        .children(
            code.chars()
                .map(|character| div().mr(letter_spacing).child(character.to_string())),
        )
        .child(crate::qa::targets::qa_target_layer(
            crate::qa::targets::QaTargetDescription {
                role: "text",
                name: code.clone().into(),
                text: code.into(),
            },
        ))
}
