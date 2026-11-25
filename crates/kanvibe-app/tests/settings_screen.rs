//! 설정 화면(TS `SettingsRoute`·`ProjectSettings`·`SessionTypeOptions`) 동작. 실제 `kanvibe.db`(임시 디렉터리)에
//! 저장되는 값과 화면이 그린 글자(QA 대상)로 확인한다.

use std::time::Duration;

use gpui_kit::gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, WeakEntity};
use kanvibe_app::app_events::{RefreshScope, trigger_refresh};
use kanvibe_app::qa::faults::ServiceFault;
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::qa::targets::{QaTargetRegistry, TargetQuery};
use kanvibe_app::routes::{RouteResolution, resolve_route_path};
use kanvibe_app::services::AppServices;
use kanvibe_app::theme::{ThemeMode, ThemePreference, ThemeTokens, spacing};
use kanvibe_app::views::settings::SettingsView;
use kanvibe_app::views::settings::form::{
    BackgroundSyncPreferences, NotificationPreferences, SettingChange, SettingKey, SettingsForm,
    SettingsValues, toggle_notification_status,
};
use kanvibe_app::views::settings::page::aside_top_padding;
use kanvibe_app::views::settings::session_type_select::SessionTypeSelect;
use kanvibe_app::views::{ScreenContext, WindowNavigator};
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::task::SessionType;
use serde_json::json;

const FIXED_NOW_MS: i64 = 1_768_446_000_000;

fn init_app_with_database(cx: &mut TestAppContext) -> tempfile::TempDir {
    let data_dir = tempfile::tempdir().expect("temp dir");
    let launch = LaunchOptions {
        data_dir: Some(data_dir.path().to_path_buf()),
        initial_route: "/ko/settings".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: Some(QaOptions {
            fixture_root: None,
            fixed_now_ms: FIXED_NOW_MS,
            network_responses_path: None,
            faults_path: None,
            control_socket_path: None,
            platform_override: None,
        }),
    };
    cx.update(|cx| kanvibe_app::init_app(&launch, cx).expect("app init"));
    data_dir
}

fn add_fault(cx: &mut VisualTestContext, fault: serde_json::Value) {
    let fault: ServiceFault = serde_json::from_value(fault).expect("fault");
    cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .expect("QA faults")
            .add(fault)
    });
}

fn clear_fault(cx: &mut VisualTestContext, target: &str) {
    cx.update(|_, cx| {
        cx.global::<AppServices>()
            .faults()
            .expect("QA faults")
            .clear(target)
    });
}

fn screen(path: &str) -> ScreenContext {
    let RouteResolution::Render(route) = resolve_route_path(path) else {
        panic!("{path} must render");
    };
    ScreenContext {
        route,
        navigator: WindowNavigator::new(WeakEntity::new_invalid()),
    }
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

fn stored_setting(cx: &mut VisualTestContext, key: &str) -> Option<String> {
    cx.update(|_, cx| {
        let services = cx.global::<AppServices>();
        let database = services.database().expect("database").clone();
        AppSettingsService::new(&database, services.shortcut_platform)
            .get_app_setting(key)
            .expect("read setting")
    })
}

fn open_settings(cx: &mut TestAppContext) -> (Entity<SettingsView>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, cx| SettingsView::new(screen("/ko/settings"), cx));
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn sample_values() -> SettingsValues {
    SettingsValues {
        theme_preference: ThemePreference::System,
        is_sidebar_default_collapsed: false,
        default_session_type: SessionType::Herdr,
        is_vim_mode_enabled: true,
        notifications: NotificationPreferences {
            is_enabled: true,
            enabled_statuses: vec!["progress".into(), "pending".into(), "review".into()],
        },
        background_sync: BackgroundSyncPreferences {
            is_enabled: true,
            interval_ms: 300_000,
        },
    }
}

// parity: TS-renderer-SettingsRoute-f40f8f
#[gpui::test]
fn slow_settings_load_shows_the_delay_notice_without_filling_defaults(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    cx.update(|cx| {
        cx.global::<AppServices>().faults().unwrap().add(
            serde_json::from_value(json!({"target": "appSettings.*", "mode": "hang"})).unwrap(),
        )
    });
    let (view, cx) = open_settings(cx);
    cx.executor().advance_clock(Duration::from_millis(5_100));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(view.load_status().is_load_delayed);
        assert!(view.form().is_none(), "기본값으로 채우지 않는다");
    });
}

// parity: TS-renderer-SettingsRoute-b25a84
#[gpui::test]
fn failed_settings_load_shows_reason_and_retry_reloads(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = cx.add_window_view(|_, cx| {
        cx.global::<AppServices>().faults().unwrap().add(
            serde_json::from_value(
                json!({"target": "appSettings.*", "mode": "fail", "message": "database is locked"}),
            )
            .unwrap(),
        );
        SettingsView::new(screen("/ko/settings"), cx)
    });
    cx.run_until_parked();
    draw(cx);
    assert!(is_shown(
        cx,
        json!({"text": "불러오지 못했습니다: Error invoking remote method 'kanvibe:invoke': Error: database is locked"})
    ));
    assert!(is_shown(cx, json!({"role": "button", "name": "다시 시도"})));
    assert!(view.read_with(cx, |view, _| view.form().is_none()));

    clear_fault(cx, "appSettings.*");
    view.update(cx, |view, cx| view.retry_load(cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.form().is_some()));
    assert!(is_shown(cx, json!({"text": "사이드바 기본 접기"})));
}

// parity: TS-renderer-SettingsRoute-28ac2c
#[gpui::test]
fn failed_refresh_keeps_loaded_settings_and_shows_the_banner(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    add_fault(
        cx,
        json!({"target": "appSettings.*", "mode": "fail", "message": "database is locked"}),
    );
    // 보드 갱신 같은 새로 고침 신호가 오면 다시 읽는다(TS `useRefreshSignal(["all", "settings"])`)
    cx.update(|_, cx| trigger_refresh(RefreshScope::Settings, cx));
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.form().is_some()));
    assert!(is_shown(
        cx,
        json!({"text": "최신 데이터를 불러오지 못해 이전 데이터를 표시합니다: Error invoking remote method 'kanvibe:invoke': Error: database is locked"})
    ));
    assert!(is_shown(cx, json!({"text": "사이드바 기본 접기"})));
}

// parity: TS-components-ProjectSettings-4085ee
#[gpui::test]
fn changing_the_default_session_type_saves_it(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    view.update(cx, |view, cx| {
        view.save_setting(SettingChange::DefaultSessionType(SessionType::Tmux), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        stored_setting(cx, "default_session_type").as_deref(),
        Some("tmux")
    );
}

// parity: TS-components-ProjectSettings-ebd688
#[gpui::test]
fn choosing_a_theme_applies_it_at_once_and_stores_it(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    view.update(cx, |view, cx| {
        view.save_setting(SettingChange::Theme(ThemePreference::Light), cx)
    });
    assert_eq!(
        cx.update(|_, cx| cx.global::<ThemeTokens>().mode),
        ThemeMode::Light
    );
    cx.run_until_parked();
    assert_eq!(
        stored_setting(cx, "theme_preference").as_deref(),
        Some("light")
    );
}

// parity: TS-components-ProjectSettings-604ea7
#[gpui::test]
fn vim_mode_toggle_updates_the_screen_and_storage(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    view.update(cx, |view, cx| {
        view.save_setting(SettingChange::VimModeEnabled(false), cx)
    });
    assert!(view.read_with(cx, |view, _| {
        !view.form().unwrap().values.is_vim_mode_enabled
    }));
    cx.run_until_parked();
    assert_eq!(
        stored_setting(cx, "vim_mode_enabled").as_deref(),
        Some("false")
    );
}

// parity: TS-components-ProjectSettings-ca9abb
#[test]
fn mac_desktop_moves_the_board_link_below_the_titlebar_buttons() {
    assert_eq!(aside_top_padding(true, true), spacing(16.0));
    assert_eq!(aside_top_padding(true, false), spacing(16.0));
    assert_eq!(aside_top_padding(false, true), spacing(6.0));
    assert_eq!(aside_top_padding(false, false), spacing(4.0));
}

// parity: TS-components-ProjectSettings-4b376c
#[gpui::test]
fn settings_page_does_not_render_project_scan_or_project_list(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (_view, cx) = open_settings(cx);
    assert!(is_shown(cx, json!({"text": "사이드바 기본 접기"})));
    assert!(!is_shown(cx, json!({"text": "프로젝트 스캔"})));
    assert!(!is_shown(cx, json!({"text": "등록된 프로젝트"})));
}

// parity: TS-components-ProjectSettings-337f5c
#[gpui::test]
fn notification_toggle_reflects_the_new_value_before_saving_finishes(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    add_fault(
        cx,
        json!({"target": "appSettings.setNotificationEnabled", "mode": "hang"}),
    );
    view.update(cx, |view, cx| {
        view.save_setting(SettingChange::NotificationEnabled(false), cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let form = view.form().unwrap();
        assert!(!form.values.notifications.is_enabled);
        assert!(form.is_saving(SettingKey::Notifications));
    });
}

// parity: TS-components-ProjectSettings-45b072
#[gpui::test]
fn notification_status_chip_saves_immediately(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    let current = view.read_with(cx, |view, _| {
        view.form()
            .unwrap()
            .values
            .notifications
            .enabled_statuses
            .clone()
    });
    let next = toggle_notification_status(&current, "pending");
    view.update(cx, |view, cx| {
        view.save_setting(SettingChange::NotificationStatuses(next.clone()), cx)
    });
    cx.run_until_parked();
    let stored = stored_setting(cx, "notification_statuses").expect("stored statuses");
    assert_eq!(serde_json::from_str::<Vec<String>>(&stored).unwrap(), next);
    assert!(!next.contains(&"pending".to_string()));
}

// parity: TS-components-ProjectSettings-f2b498
#[test]
fn stale_refresh_does_not_overwrite_a_notification_change_being_saved() {
    let mut form = SettingsForm::new(sample_values());
    let _pending = form.begin_save(SettingChange::NotificationStatuses(vec!["review".into()]));
    form.apply_refresh(&sample_values());
    assert_eq!(
        form.values.notifications.enabled_statuses,
        vec!["review".to_string()]
    );
}

// parity: TS-components-ProjectSettings-71958f
#[test]
fn interval_input_accepts_clearing_then_typing_two_digits() {
    let mut form = SettingsForm::new(sample_values());
    assert_eq!(form.interval_input, "5");
    assert_eq!(form.edit_interval_input(""), None);
    assert_eq!(
        form.interval_input, "",
        "빈 칸을 기본값으로 되돌리지 않는다"
    );
    assert_eq!(
        form.edit_interval_input("1"),
        Some(SettingChange::BackgroundSyncIntervalMs(60_000))
    );
    assert_eq!(
        form.edit_interval_input("15"),
        Some(SettingChange::BackgroundSyncIntervalMs(900_000))
    );
    assert_eq!(form.interval_input, "15");
    form.edit_interval_input("abc");
    form.blur_interval_input();
    assert_eq!(
        form.interval_input, "5",
        "벗어날 때 쓸 수 없는 값은 지금 값으로 되돌린다"
    );
}

// parity: TS-components-ProjectSettings-0ea10d
#[test]
fn interval_saves_run_one_after_another_and_end_with_the_latest() {
    let mut form = SettingsForm::new(sample_values());
    let first = form.begin_save(SettingChange::BackgroundSyncIntervalMs(60_000));
    let sent_first = form
        .enqueue_interval_save(first)
        .expect("첫 저장은 바로 보낸다");
    let second = form.begin_save(SettingChange::BackgroundSyncIntervalMs(900_000));
    assert!(
        form.enqueue_interval_save(second).is_none(),
        "앞 저장이 끝나기 전에는 보내지 않는다"
    );
    assert_eq!(
        sent_first.change,
        SettingChange::BackgroundSyncIntervalMs(60_000)
    );
    form.finish_save(sent_first, Ok(()));
    let sent_second = form.finish_interval_save().expect("다음 저장");
    assert_eq!(
        sent_second.change,
        SettingChange::BackgroundSyncIntervalMs(900_000)
    );
    assert_eq!(form.values.background_sync.interval_ms, 900_000);
}

// parity: TS-components-ProjectSettings-29a148
#[gpui::test]
fn failed_vim_mode_save_restores_the_value_and_offers_retry(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    add_fault(
        cx,
        json!({"target": "appSettings.setVimModeEnabled", "mode": "fail", "message": "database is locked"}),
    );
    view.update(cx, |view, cx| {
        view.save_setting(SettingChange::VimModeEnabled(false), cx)
    });
    cx.run_until_parked();
    draw(cx);
    view.read_with(cx, |view, _| {
        let form = view.form().unwrap();
        assert!(form.values.is_vim_mode_enabled, "저장값으로 되돌린다");
        assert_eq!(
            form.failure_for(SettingKey::VimMode)
                .map(|failure| failure.reason.as_str()),
            Some("Error invoking remote method 'kanvibe:invoke': Error: database is locked")
        );
    });
    assert!(is_shown(
        cx,
        json!({"text": "설정을 저장하지 못해 이전 값으로 되돌렸습니다: Error invoking remote method 'kanvibe:invoke': Error: database is locked"})
    ));

    clear_fault(cx, "appSettings.setVimModeEnabled");
    view.update(cx, |view, cx| view.retry_failed_save(cx));
    cx.run_until_parked();
    assert_eq!(
        stored_setting(cx, "vim_mode_enabled").as_deref(),
        Some("false")
    );
    assert!(view.read_with(cx, |view, _| view.form().unwrap().failure.is_none()));
}

// parity: TS-components-ProjectSettings-2e48c4
#[gpui::test]
fn saving_one_setting_blocks_only_that_setting(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    add_fault(
        cx,
        json!({"target": "appSettings.setSidebarDefaultCollapsed", "mode": "hang"}),
    );
    view.update(cx, |view, cx| {
        view.save_setting(SettingChange::SidebarDefaultCollapsed(true), cx);
        view.save_setting(SettingChange::VimModeEnabled(false), cx);
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let form = view.form().unwrap();
        assert!(form.is_saving(SettingKey::Sidebar));
        assert!(!form.is_saving(SettingKey::VimMode));
    });
    assert_eq!(
        stored_setting(cx, "vim_mode_enabled").as_deref(),
        Some("false")
    );
}

// parity: TS-components-ProjectSettings-0e982d
#[gpui::test]
fn failed_notification_save_restores_previous_statuses(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    let before = view.read_with(cx, |view, _| {
        view.form().unwrap().values.notifications.clone()
    });
    add_fault(
        cx,
        json!({"target": "appSettings.setNotificationStatuses", "mode": "fail", "message": "disk full"}),
    );
    view.update(cx, |view, cx| {
        view.save_setting(SettingChange::NotificationStatuses(vec![]), cx)
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.form().unwrap().values.notifications, before);
    });
}

// parity: TS-components-SessionTypeOptions-9b79cf
#[gpui::test]
fn session_type_select_offers_the_four_session_types(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (select, cx) = cx.add_window_view(|_, cx| {
        SessionTypeSelect::new("session-type-test", SessionType::Herdr, cx)
    });
    select.update(cx, |select, cx| select.toggle_open(cx));
    draw(cx);
    for session_type in ["tmux", "zellij", "herdr", "terminal"] {
        assert!(
            is_shown(
                cx,
                json!({"role": "option", "name": session_type, "exact": true})
            ),
            "{session_type}"
        );
    }
}

#[gpui::test]
fn settings_window_title_matches_the_ts_document_title(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (_view, cx) = open_settings(cx);
    assert_eq!(cx.window_title().as_deref(), Some("Settings"));
}

fn click(cx: &mut VisualTestContext, query: serde_json::Value) {
    let query: TargetQuery = serde_json::from_value(query).expect("query");
    let bounds = cx
        .update(|window, cx| {
            let window_id = window.window_handle().window_id();
            cx.global::<QaTargetRegistry>().find(window_id, &query)
        })
        .unwrap_or_else(|| panic!("{query:?} not found"));
    cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
}

/// 일괄 변경은 런타임의 작업 서비스에 닿는다(SCR-SET-06 회귀: 전에는 연결되지 않은 자리 표시가 실패로 답했다).
/// 빈 DB라 바꿀 작업이 없다는 안내로 끝난다
#[gpui::test]
fn bulk_session_type_change_asks_the_runtime_kanban_service(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_settings(cx);
    click(cx, json!({"role": "button", "name": "적용"}));
    cx.run_until_parked();
    let bulk_change = view
        .read_with(cx, |view, _| view.bulk_session_type_change())
        .expect("loaded");
    let outcome = bulk_change.read_with(cx, |bulk_change, _| bulk_change.outcome.clone());
    assert_eq!(
        outcome.map(|outcome| (outcome.message, outcome.is_failure)),
        Some(("바꿀 작업이 없습니다.".to_string(), false))
    );
}

/// 모바일 연결 코드는 프로세스 안 서버와 같은 기기 계층이 발급한다(전에는 서버가 없다는 실패로 답했다)
#[gpui::test]
fn pairing_code_comes_from_the_in_process_server(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (pairing, cx) = cx.add_window_view(|_, cx| {
        kanvibe_app::views::settings::mobile_pairing::MobilePairingSettings::new(
            kanvibe_domain::i18n::Locale::Ko,
            cx,
        )
    });
    cx.run_until_parked();
    pairing.update(cx, |pairing, cx| pairing.issue_code(cx));
    cx.run_until_parked();
    let issued = pairing.read_with(cx, |pairing, _| pairing.state.code.clone());
    let pending = cx.update(|_, cx| {
        cx.global::<AppServices>()
            .mobile_devices()
            .and_then(|devices| devices.read_mobile_pairing_code())
    });
    assert!(issued.is_some());
    assert_eq!(issued, pending);
}
