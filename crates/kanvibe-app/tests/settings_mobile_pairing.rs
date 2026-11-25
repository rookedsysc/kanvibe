//! 모바일 앱 연결 설정(TS `MobilePairingSettings`) 동작. 페어링 서버 대신 가짜 통로를 걸고 화면 상태와 보낸 요청을 본다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use kanvibe_app::qa::faults::ServiceFault;
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::qa::targets::{QaTargetRegistry, TargetQuery};
use kanvibe_app::services::{AppServices, ServiceFailure, ServiceResult};
use kanvibe_app::views::settings::mobile_pairing::{
    MobileConnectionSettings, MobilePairingBridge, MobilePairingCode, MobilePairingSettings,
    PairedMobileDevice, PairingAction, format_paired_at,
};
use kanvibe_domain::i18n::Locale;
use serde_json::json;

const FIXED_NOW_MS: i64 = 1_768_446_000_000;

#[derive(Default)]
struct BridgeState {
    current_code: Option<String>,
    devices: Vec<PairedMobileDevice>,
    is_device_list_failing: bool,
    is_issue_failing: bool,
    is_lan_update_failing: bool,
    stop_calls: usize,
    list_calls: usize,
    unpaired: Vec<String>,
    lan_updates: Vec<bool>,
}

#[derive(Default)]
struct FakeBridge {
    state: Mutex<BridgeState>,
}

fn failed(reason: &str) -> ServiceFailure {
    ServiceFailure {
        reason: reason.to_string(),
    }
}

impl MobilePairingBridge for FakeBridge {
    fn start_mobile_pairing(&self) -> ServiceResult<MobilePairingCode> {
        let mut state = self.state.lock().unwrap();
        if state.is_issue_failing {
            return Err(failed("bridge offline"));
        }
        state.current_code = Some("482913".into());
        Ok(MobilePairingCode {
            code: "482913".into(),
            expires_at: FIXED_NOW_MS + 300_000,
        })
    }

    fn stop_mobile_pairing(&self) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.stop_calls += 1;
        state.current_code = None;
        Ok(())
    }

    fn read_mobile_pairing_code(&self) -> ServiceResult<Option<String>> {
        Ok(self.state.lock().unwrap().current_code.clone())
    }

    fn list_paired_mobile_devices(&self) -> ServiceResult<Vec<PairedMobileDevice>> {
        let mut state = self.state.lock().unwrap();
        state.list_calls += 1;
        if state.is_device_list_failing {
            return Err(failed("store locked"));
        }
        Ok(state.devices.clone())
    }

    fn unpair_mobile_device(&self, device_id: &str) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        state.unpaired.push(device_id.to_string());
        state.devices.retain(|device| device.device_id != device_id);
        Ok(())
    }

    fn read_mobile_connection_settings(&self) -> ServiceResult<MobileConnectionSettings> {
        Ok(MobileConnectionSettings {
            mobile_tls_port: 47746,
            is_lan_plaintext_allowed: false,
        })
    }

    fn update_lan_plaintext_allowed(&self, is_allowed: bool) -> ServiceResult<()> {
        let mut state = self.state.lock().unwrap();
        if state.is_lan_update_failing {
            return Err(failed("disk full"));
        }
        state.lan_updates.push(is_allowed);
        Ok(())
    }
}

fn device(device_id: &str, name: &str) -> PairedMobileDevice {
    PairedMobileDevice {
        device_id: device_id.into(),
        device_name: name.into(),
        paired_at: "2026-01-14T09:00:00.000Z".into(),
    }
}

fn init_app(cx: &mut TestAppContext) {
    let launch = LaunchOptions {
        data_dir: None,
        initial_route: "/ko/settings".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: Some(QaOptions {
            fixture_root: None,
            fixed_now_ms: FIXED_NOW_MS,
            network_responses_path: None,
            faults_path: None,
            control_socket_path: None,
            platform_override: None,
            loading_holds: Default::default(),
        }),
    };
    cx.update(|cx| {
        kanvibe_app::init_app(&launch, cx).expect("app init");
    });
}

fn open_pairing(
    cx: &mut TestAppContext,
    bridge: Arc<FakeBridge>,
) -> (Entity<MobilePairingSettings>, &mut VisualTestContext) {
    let (view, cx) =
        cx.add_window_view(|_, cx| MobilePairingSettings::with_bridge(Locale::Ko, bridge, cx));
    cx.run_until_parked();
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let window_id = window.window_handle().window_id();
        cx.global_mut::<QaTargetRegistry>().begin_frame(window_id);
        window.draw(cx).clear(cx);
    });
}

fn is_shown(cx: &mut VisualTestContext, query: serde_json::Value) -> bool {
    let query: TargetQuery = serde_json::from_value(query).expect("query");
    cx.update(|window, cx| {
        let window_id = window.window_handle().window_id();
        cx.global::<QaTargetRegistry>()
            .find(window_id, &query)
            .is_some()
    })
}

fn close_window(cx: &mut VisualTestContext) {
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
}

// parity: TS-components-MobilePairingSettings-bf49c9
#[gpui::test]
fn no_code_is_shown_before_issuing_one(cx: &mut TestAppContext) {
    init_app(cx);
    let (view, cx) = open_pairing(cx, Arc::new(FakeBridge::default()));
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.state.code.is_none()));
    assert!(is_shown(
        cx,
        json!({"role": "button", "name": "연결 코드 만들기", "exact": true})
    ));
}

// parity: TS-components-MobilePairingSettings-85c06d
#[gpui::test]
fn issuing_shows_the_code_to_type_in(cx: &mut TestAppContext) {
    init_app(cx);
    let (view, cx) = open_pairing(cx, Arc::new(FakeBridge::default()));
    view.update(cx, |view, cx| view.issue_code(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.state.code.as_deref(), Some("482913"));
        assert_eq!(view.state.remaining_seconds, 300);
    });
}

// parity: TS-components-MobilePairingSettings-4ad086
#[gpui::test]
fn shown_code_turns_the_button_into_clear(cx: &mut TestAppContext) {
    init_app(cx);
    let (view, cx) = open_pairing(cx, Arc::new(FakeBridge::default()));
    view.update(cx, |view, cx| view.issue_code(cx));
    cx.run_until_parked();
    draw(cx);
    assert!(is_shown(
        cx,
        json!({"role": "button", "name": "코드 지우기", "exact": true})
    ));
    assert!(!is_shown(
        cx,
        json!({"role": "button", "name": "연결 코드 만들기", "exact": true})
    ));
}

// parity: TS-components-MobilePairingSettings-990bba
#[gpui::test]
fn clearing_the_code_removes_it(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge.clone());
    view.update(cx, |view, cx| view.issue_code(cx));
    cx.run_until_parked();
    view.update(cx, |view, cx| view.clear_code(cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.state.code.is_none()));
    assert_eq!(bridge.state.lock().unwrap().stop_calls, 1);
}

// parity: TS-components-MobilePairingSettings-18443b
#[gpui::test]
fn code_discarded_by_the_desktop_disappears_before_expiry(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge.clone());
    view.update(cx, |view, cx| view.issue_code(cx));
    cx.run_until_parked();
    bridge.state.lock().unwrap().current_code = None;
    cx.executor().advance_clock(Duration::from_millis(1_100));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.state.code.is_none()));
}

// parity: TS-components-MobilePairingSettings-37289c
#[gpui::test]
fn shows_the_tls_port_and_saves_lan_plaintext(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge.clone());
    assert_eq!(
        view.read_with(cx, |view, _| view
            .state
            .connection
            .map(|c| c.mobile_tls_port)),
        Some(47746)
    );
    view.update(cx, |view, cx| view.change_lan_plaintext_allowed(true, cx));
    cx.run_until_parked();
    assert_eq!(bridge.state.lock().unwrap().lan_updates, vec![true]);
    assert_eq!(
        view.read_with(cx, |view, _| view
            .state
            .connection
            .map(|c| c.is_lan_plaintext_allowed)),
        Some(true)
    );
}

// parity: TS-components-MobilePairingSettings-10a6c0
#[gpui::test]
fn failed_lan_plaintext_save_explains_and_keeps_the_checkbox(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    bridge.state.lock().unwrap().is_lan_update_failing = true;
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge);
    view.update(cx, |view, cx| view.change_lan_plaintext_allowed(true, cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.state.failure_message.as_deref(),
            Some("LAN 평문 설정을 바꾸지 못했습니다.")
        );
        assert_eq!(
            view.state.connection.map(|c| c.is_lan_plaintext_allowed),
            Some(false)
        );
    });
}

// parity: TS-components-MobilePairingSettings-b0a517
#[gpui::test]
fn paired_devices_are_listed(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    bridge.state.lock().unwrap().devices = vec![device("d1", "Pixel 9"), device("d2", "iPhone 17")];
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge);
    draw(cx);
    let names = view.read_with(cx, |view, _| {
        view.state
            .devices
            .iter()
            .map(|device| device.device_name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(names, vec!["Pixel 9", "iPhone 17"]);
    assert!(is_shown(
        cx,
        json!({"role": "button", "name": "연결 끊기", "nth": 1})
    ));
}

// parity: TS-components-MobilePairingSettings-b5eace
#[gpui::test]
fn disconnecting_sends_the_device_id_and_rereads_the_list(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    bridge.state.lock().unwrap().devices = vec![device("d1", "Pixel 9"), device("d2", "iPhone 17")];
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge.clone());
    let list_calls_before = bridge.state.lock().unwrap().list_calls;
    view.update(cx, |view, cx| view.disconnect_device("d1".into(), cx));
    cx.run_until_parked();
    let state = bridge.state.lock().unwrap();
    assert_eq!(state.unpaired, vec!["d1".to_string()]);
    assert_eq!(state.list_calls, list_calls_before + 1);
    drop(state);
    assert_eq!(view.read_with(cx, |view, _| view.state.devices.len()), 1);
}

// parity: TS-components-MobilePairingSettings-0ca283
#[gpui::test]
fn leaving_without_a_code_does_not_call_the_desktop(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    init_app(cx);
    let (_view, cx) = open_pairing(cx, bridge.clone());
    close_window(cx);
    assert_eq!(bridge.state.lock().unwrap().stop_calls, 0);
}

// parity: TS-components-MobilePairingSettings-ad1f08
#[gpui::test]
fn leaving_with_a_code_on_screen_withdraws_it(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge.clone());
    view.update(cx, |view, cx| view.issue_code(cx));
    cx.run_until_parked();
    drop(view);
    close_window(cx);
    assert_eq!(bridge.state.lock().unwrap().stop_calls, 1);
}

// parity: TS-components-MobilePairingSettings-498f79
#[gpui::test]
fn unreadable_device_list_still_renders_the_settings(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    bridge.state.lock().unwrap().is_device_list_failing = true;
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge);
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.state.is_device_list_unreadable));
    assert!(is_shown(
        cx,
        json!({"role": "button", "name": "연결 코드 만들기", "exact": true})
    ));
    assert!(is_shown(
        cx,
        json!({"text": "연결된 기기 목록을 읽지 못했습니다."})
    ));
}

// parity: TS-components-MobilePairingSettings-e9bdab
#[gpui::test]
fn unreadable_device_list_keeps_known_devices_and_can_reread(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    bridge.state.lock().unwrap().devices = vec![device("d1", "Pixel 9")];
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge.clone());
    bridge.state.lock().unwrap().is_device_list_failing = true;
    view.update(cx, |view, cx| view.refresh_paired_devices(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.state.is_device_list_unreadable);
        assert_eq!(
            view.state.devices.len(),
            1,
            "읽지 못했다고 기기를 지우지 않는다"
        );
    });
    bridge.state.lock().unwrap().is_device_list_failing = false;
    view.update(cx, |view, cx| view.refresh_paired_devices(cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| !view.state.is_device_list_unreadable));
}

// parity: TS-components-MobilePairingSettings-32888b
#[gpui::test]
fn failed_issue_explains_what_went_wrong(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    bridge.state.lock().unwrap().is_issue_failing = true;
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge);
    view.update(cx, |view, cx| view.issue_code(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.state.code.is_none());
        assert_eq!(
            view.state.failure_message.as_deref(),
            Some("연결 코드를 만들지 못했습니다.")
        );
    });
}

// parity: TS-components-MobilePairingSettings-5df4f0
#[gpui::test]
fn other_pairing_actions_wait_while_a_code_is_being_issued(cx: &mut TestAppContext) {
    let bridge = Arc::new(FakeBridge::default());
    bridge.state.lock().unwrap().devices = vec![device("d1", "Pixel 9")];
    init_app(cx);
    let (view, cx) = open_pairing(cx, bridge.clone());
    let fault: ServiceFault = serde_json::from_value(
        json!({"target": "mobileBridge.startMobilePairing", "mode": "hang"}),
    )
    .unwrap();
    cx.update(|_, cx| cx.global::<AppServices>().faults().unwrap().add(fault));
    view.update(cx, |view, cx| {
        view.issue_code(cx);
        view.disconnect_device("d1".into(), cx);
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.state.pending_action.clone()),
        Some(PairingAction::IssueCode)
    );
    assert!(bridge.state.lock().unwrap().unpaired.is_empty());
    draw(cx);
    assert!(is_shown(
        cx,
        json!({"role": "button", "name": "코드를 만드는 중…", "exact": true})
    ));
}

/// TS `new Date(pairedAt).toLocaleString()`은 시스템 시간대로 보인다. 오프셋 없는 시각은 현지 시각이므로
/// 어느 시간대에서 돌려도 같은 벽시계 값이 나와야 한다(UTC로 읽던 구현은 UTC가 아닌 곳에서 시각이 바뀐다)
#[test]
fn paired_date_is_shown_in_local_time() {
    assert_eq!(
        format_paired_at("2026-01-14T21:05:09"),
        "1/14/2026, 9:05:09 PM"
    );
    assert_eq!(format_paired_at("not a date"), "not a date");
}
