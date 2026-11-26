//! 단축키 설정 화면(TS `ShortcutSettingsRoute`). 녹화·충돌·되돌리기·저장을 실제 DB와 키 입력으로 확인한다.

use gpui_kit::gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, WeakEntity};
use kanvibe_app::qa::faults::ServiceFault;
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::routes::{RouteResolution, resolve_route_path};
use kanvibe_app::services::AppServices;
use kanvibe_app::shortcuts::ShortcutState;
use kanvibe_app::views::shortcut_settings::model::{
    RecordingKeyOutcome, ShortcutNotice, ShortcutSettingsModel,
};
use kanvibe_app::views::shortcut_settings::{ShortcutSettingsView, describe_command};
use kanvibe_app::views::{ScreenContext, WindowNavigator};
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::i18n::Locale;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::bindings::{
    ShortcutBindings, ShortcutCommandGroup, ShortcutCommandId, shortcut_command_definitions,
};
use kanvibe_domain::shortcuts::key_event::{ModifierState, ShortcutKeyEvent};
use serde_json::json;

fn init_app_with_database(cx: &mut TestAppContext) -> tempfile::TempDir {
    let data_dir = tempfile::tempdir().expect("temp dir");
    let launch = LaunchOptions {
        data_dir: Some(data_dir.path().to_path_buf()),
        initial_route: "/ko/settings/shortcuts".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: Some(QaOptions {
            fixture_root: None,
            fixed_now_ms: 1_768_446_000_000,
            network_responses_path: None,
            faults_path: None,
            control_socket_path: None,
            platform_override: None,
            loading_holds: Default::default(),
        }),
    };
    cx.update(|cx| kanvibe_app::init_app(&launch, cx).expect("app init"));
    data_dir
}

fn add_fault(cx: &mut TestAppContext, fault: serde_json::Value) {
    let fault: ServiceFault = serde_json::from_value(fault).expect("fault");
    cx.update(|cx| cx.global::<AppServices>().faults().expect("QA").add(fault));
}

fn open_shortcuts(
    cx: &mut TestAppContext,
) -> (Entity<ShortcutSettingsView>, &mut VisualTestContext) {
    let RouteResolution::Render(route) = resolve_route_path("/ko/settings/shortcuts") else {
        panic!("route");
    };
    let screen = ScreenContext {
        route,
        navigator: WindowNavigator::new(WeakEntity::new_invalid()),
    };
    let (view, cx) = cx.add_window_view(|_, cx| ShortcutSettingsView::new(screen, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx)
}

fn stored_bindings(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        let services = cx.global::<AppServices>();
        let database = services.database().expect("database").clone();
        AppSettingsService::new(&database, services.shortcut_platform)
            .get_app_setting("shortcut_bindings")
            .expect("read")
    })
}

fn key(key: &str, has_ctrl: bool, has_shift: bool) -> ShortcutKeyEvent {
    ShortcutKeyEvent {
        key: key.to_string(),
        modifiers: ModifierState {
            has_meta: false,
            has_ctrl,
            has_alt: false,
            has_shift,
        },
    }
}

fn loaded_model() -> ShortcutSettingsModel {
    let mut model = ShortcutSettingsModel::new(ShortcutPlatform::Linux);
    model.finish_load(Ok(ShortcutBindings::defaults()));
    model
}

#[gpui::test]
fn recorded_combination_becomes_the_commands_new_shortcut(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_shortcuts(cx);
    view.update_in(cx, |view, window, cx| {
        view.toggle_recording(ShortcutCommandId::TaskDetailDock4, window, cx)
    });
    cx.simulate_keystrokes("ctrl-shift-k");
    cx.run_until_parked();
    let stored = stored_bindings(cx).expect("saved bindings");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
        json!({"taskDetailDock4": "Mod+Shift+K"})
    );
}

#[gpui::test]
fn recorded_pane_toggle_is_normalized_and_every_window_reloads_it(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_shortcuts(cx);
    view.update_in(cx, |view, window, cx| {
        view.toggle_recording(ShortcutCommandId::TerminalToggleChat, window, cx)
    });
    cx.simulate_keystrokes("ctrl-alt-j");
    cx.run_until_parked();
    let reloaded = cx.update(|_, cx| {
        cx.global::<ShortcutState>()
            .bindings
            .shortcut(ShortcutCommandId::TerminalToggleChat)
            .to_string()
    });
    assert_eq!(reloaded, "Mod+Alt+J");
}

#[test]
fn taken_combination_is_not_saved_and_names_the_other_command() {
    let mut model = loaded_model();
    model.toggle_recording(ShortcutCommandId::TaskDetailDock4);
    let outcome = model.handle_recording_key(&key("O", true, true));
    assert_eq!(outcome, RecordingKeyOutcome::Continue);
    assert_eq!(
        model.notice,
        Some(ShortcutNotice::Conflict(ShortcutCommandId::TaskSearch))
    );
    let conflict = shortcut_command_definitions()
        .iter()
        .find(|definition| definition.id == ShortcutCommandId::TaskSearch)
        .unwrap();
    assert_eq!(describe_command(conflict, Locale::Ko), "작업 빠른 검색");
}

#[test]
fn resetting_to_a_default_taken_by_another_command_is_refused() {
    let mut model = loaded_model();
    let mut bindings = ShortcutBindings::defaults();
    bindings.set_shortcut(ShortcutCommandId::TaskSearch, "Mod+Shift+K");
    bindings.set_shortcut(ShortcutCommandId::CreateTask, "Mod+Shift+O");
    model.finish_load(Ok(bindings));
    assert_eq!(model.reset_command(ShortcutCommandId::TaskSearch), None);
    assert_eq!(
        model.notice,
        Some(ShortcutNotice::Conflict(ShortcutCommandId::CreateTask))
    );
}

#[gpui::test]
fn escape_cancels_recording(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_shortcuts(cx);
    view.update_in(cx, |view, window, cx| {
        view.toggle_recording(ShortcutCommandId::TaskSearch, window, cx)
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.model.recording), None);
    assert_eq!(stored_bindings(cx), None, "아무것도 저장하지 않는다");
}

#[test]
fn modifier_only_presses_do_not_raise_an_error() {
    let mut model = loaded_model();
    model.toggle_recording(ShortcutCommandId::TaskSearch);
    for modifier in ["Control", "Shift", "Alt", "Meta"] {
        assert_eq!(
            model.handle_recording_key(&key(modifier, true, false)),
            RecordingKeyOutcome::Continue
        );
    }
    assert_eq!(model.notice, None);
    assert_eq!(model.recording, Some(ShortcutCommandId::TaskSearch));
}

#[test]
fn unsupported_combination_is_not_saved() {
    let mut model = loaded_model();
    model.toggle_recording(ShortcutCommandId::TaskSearch);
    assert_eq!(
        model.handle_recording_key(&key("k", false, false)),
        RecordingKeyOutcome::Continue
    );
    assert_eq!(model.notice, Some(ShortcutNotice::Unsupported));
    assert_eq!(model.bindings, ShortcutBindings::defaults());
}

#[test]
fn escape_also_clears_a_leftover_error() {
    let mut model = loaded_model();
    model.toggle_recording(ShortcutCommandId::TaskSearch);
    model.handle_recording_key(&key("k", false, false));
    assert!(model.notice.is_some());
    assert_eq!(
        model.handle_recording_key(&key("Escape", false, false)),
        RecordingKeyOutcome::Cancelled
    );
    assert_eq!(model.notice, None);
}

#[gpui::test]
fn reset_all_removes_every_stored_override(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    cx.update(|cx| {
        let services = cx.global::<AppServices>();
        let database = services.database().unwrap().clone();
        AppSettingsService::new(&database, services.shortcut_platform)
            .set_app_setting("shortcut_bindings", r#"{"taskSearch":"Mod+Shift+K"}"#)
            .unwrap();
    });
    let (view, cx) = open_shortcuts(cx);
    assert!(view.read_with(cx, |view, _| {
        view.model.is_customized(ShortcutCommandId::TaskSearch)
    }));
    view.update(cx, |view, cx| view.reset_all(cx));
    cx.run_until_parked();
    assert_eq!(stored_bindings(cx).as_deref(), Some("{}"));
}

#[gpui::test]
fn recording_pauses_app_shortcuts_for_this_window_until_it_ends(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (view, cx) = open_shortcuts(cx);
    let is_capturing = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            cx.global::<ShortcutState>()
                .capturing_windows
                .contains(&window.window_handle().window_id())
        })
    };
    view.update_in(cx, |view, window, cx| {
        view.toggle_recording(ShortcutCommandId::TaskSearch, window, cx)
    });
    assert!(is_capturing(cx));
    view.update_in(cx, |view, window, cx| {
        view.toggle_recording(ShortcutCommandId::TaskSearch, window, cx)
    });
    assert!(!is_capturing(cx));
}

#[gpui::test]
fn failed_save_raises_the_error_banner(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    add_fault(
        cx,
        json!({"target": "appSettings.setShortcutBindings", "mode": "fail", "message": "disk full"}),
    );
    let (view, cx) = open_shortcuts(cx);
    view.update(cx, |view, cx| view.reset_all(cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.model.notice),
        Some(ShortcutNotice::SaveFailed)
    );
}

#[gpui::test]
fn failed_load_blocks_saving_and_reports_it(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    add_fault(
        cx,
        json!({"target": "appSettings.getShortcutBindings", "mode": "fail", "message": "database is locked"}),
    );
    let (view, cx) = open_shortcuts(cx);
    view.update(cx, |view, cx| view.reset_all(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.model.is_loaded);
        assert_eq!(view.model.notice, Some(ShortcutNotice::LoadFailed));
    });
    assert_eq!(stored_bindings(cx), None);
}

#[test]
fn dock_commands_use_the_dock_item_names_for_each_dock_slot() {
    let dock_labels: Vec<String> = shortcut_command_definitions()
        .iter()
        .filter(|definition| definition.group == ShortcutCommandGroup::TaskDetailDock)
        .map(|definition| describe_command(definition, Locale::Ko))
        .collect();
    assert_eq!(
        dock_labels,
        vec![
            "도크 1 · 작업 정보",
            "도크 2 · 상태 변경 · Hooks 상태",
            "도크 3 · AI 채팅",
            "도크 4 · PR",
            "도크 5 · 실행중 세션",
            "도크 6 · VS Code에서 열기",
        ]
    );
}

#[gpui::test]
fn window_title_comes_from_the_translation(cx: &mut TestAppContext) {
    let _data_dir = init_app_with_database(cx);
    let (_view, cx) = open_shortcuts(cx);
    assert_eq!(cx.window_title().as_deref(), Some("단축키"));
}
