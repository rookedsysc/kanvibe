use kanvibe_domain::board_sort::{
    BoardSortDirection, BoardSortField, BoardSortKey, BoardSortPreference,
};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::bindings::{ShortcutBindings, ShortcutCommandId};
use kanvibe_domain::task::SessionType;

use super::AppSettingsService;
use crate::database::Database;
use crate::location::DatabaseLocation;

const SHORTCUT_BINDINGS_KEY: &str = "shortcut_bindings";
const LEGACY_TASK_SEARCH_SHORTCUT_KEY: &str = "task_search_shortcut";
const BOARD_SORT_PREFERENCE_KEY: &str = "board_sort_preference";

/// 시험마다 새 DB를 열어 앞 시험이 남긴 설정을 보지 않는다
struct SettingsFixture {
    _directory: tempfile::TempDir,
    database: Database,
}

impl SettingsFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let location = DatabaseLocation {
            database_path: directory.path().join("kanvibe.db"),
            seed_database_path: None,
        };
        let database = Database::open(&location).unwrap();
        Self {
            _directory: directory,
            database,
        }
    }

    fn service(&self, platform: ShortcutPlatform) -> AppSettingsService<'_> {
        AppSettingsService::new(&self.database, platform)
    }

    fn linux_service(&self) -> AppSettingsService<'_> {
        self.service(ShortcutPlatform::Linux)
    }

    fn stored_json(&self, key: &str) -> Option<serde_json::Value> {
        let stored_value = self.linux_service().get_app_setting(key).unwrap()?;
        Some(serde_json::from_str(&stored_value).unwrap())
    }
}

fn defaults_with(command_id: ShortcutCommandId, shortcut: &str) -> ShortcutBindings {
    let mut bindings = ShortcutBindings::defaults();
    bindings.set_shortcut(command_id, shortcut);
    bindings
}

/// TS 시험의 macOS·Linux·Windows 세 경우. Windows는 Linux와 같은 `Mod` 규칙을 쓴다
const PLATFORM_CASES: [(ShortcutPlatform, &str, &str); 3] = [
    (ShortcutPlatform::Mac, "Meta", "Ctrl"),
    (ShortcutPlatform::Linux, "Ctrl", "Meta"),
    (ShortcutPlatform::Linux, "Ctrl", "Meta"),
];

#[test]
fn saved_session_type_is_returned_as_is() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();

    for session_type in SessionType::ALL {
        service.set_default_session_type(session_type).unwrap();
        assert_eq!(service.get_default_session_type().unwrap(), session_type);
    }
}

#[test]
fn missing_or_unknown_session_type_falls_back_to_herdr() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();
    assert_eq!(
        service.get_default_session_type().unwrap(),
        SessionType::Herdr
    );

    service
        .set_app_setting("default_session_type", "screen")
        .unwrap();
    assert_eq!(
        service.get_default_session_type().unwrap(),
        SessionType::Herdr
    );
}

#[test]
fn unread_only_survives_a_restart() {
    let fixture = SettingsFixture::new();
    fixture
        .linux_service()
        .set_notification_unread_only_enabled(true)
        .unwrap();

    assert!(
        fixture
            .linux_service()
            .get_notification_unread_only_enabled()
            .unwrap()
    );
}

#[test]
fn turning_unread_only_off_returns_to_all() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();
    service.set_notification_unread_only_enabled(true).unwrap();

    service.set_notification_unread_only_enabled(false).unwrap();

    assert!(!service.get_notification_unread_only_enabled().unwrap());
}

#[test]
fn unread_only_starts_off() {
    let fixture = SettingsFixture::new();

    assert!(
        !fixture
            .linux_service()
            .get_notification_unread_only_enabled()
            .unwrap()
    );
}

#[test]
fn board_sort_survives_a_restart() {
    let fixture = SettingsFixture::new();
    let preference = BoardSortPreference {
        keys: vec![BoardSortKey {
            field: BoardSortField::Priority,
            direction: BoardSortDirection::Desc,
        }],
    };

    fixture
        .linux_service()
        .set_board_sort_preference(&preference)
        .unwrap();

    assert!(fixture.stored_json(BOARD_SORT_PREFERENCE_KEY).is_some());
    assert_eq!(
        fixture.linux_service().get_board_sort_preference().unwrap(),
        preference
    );
}

#[test]
fn board_sort_starts_without_keys() {
    let fixture = SettingsFixture::new();

    assert_eq!(
        fixture.linux_service().get_board_sort_preference().unwrap(),
        BoardSortPreference::default()
    );
}

#[test]
fn broken_board_sort_falls_back_to_the_default() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();
    service
        .set_app_setting(BOARD_SORT_PREFERENCE_KEY, "{ 깨진 값")
        .unwrap();

    assert_eq!(
        service.get_board_sort_preference().unwrap(),
        BoardSortPreference::default()
    );
}

#[test]
fn reassigned_shortcuts_survive_a_restart() {
    let fixture = SettingsFixture::new();
    let bindings = defaults_with(ShortcutCommandId::TaskDetailDock4, "Mod+Shift+K");

    fixture
        .linux_service()
        .set_shortcut_bindings(&bindings)
        .unwrap();

    assert_eq!(
        fixture.linux_service().get_shortcut_bindings().unwrap(),
        bindings
    );
}

#[test]
fn entries_equal_to_the_default_are_not_stored() {
    let fixture = SettingsFixture::new();

    fixture
        .linux_service()
        .set_shortcut_bindings(&defaults_with(ShortcutCommandId::CreateTask, "Mod+Shift+Y"))
        .unwrap();

    assert_eq!(
        fixture.stored_json(SHORTCUT_BINDINGS_KEY),
        Some(serde_json::json!({"createTask": "Mod+Shift+Y"}))
    );
}

#[test]
fn shortcuts_start_from_the_defaults() {
    let fixture = SettingsFixture::new();

    assert_eq!(
        fixture.linux_service().get_shortcut_bindings().unwrap(),
        ShortcutBindings::defaults()
    );
}

#[test]
fn task_search_reads_from_the_same_table() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();

    service
        .set_shortcut_bindings(&defaults_with(ShortcutCommandId::TaskSearch, "Mod+Shift+F"))
        .unwrap();

    assert_eq!(service.get_task_search_shortcut().unwrap(), "Mod+Shift+F");
}

#[test]
fn legacy_task_search_shortcut_is_inherited_when_no_table_exists() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();
    service
        .set_app_setting(LEGACY_TASK_SEARCH_SHORTCUT_KEY, "Mod+Shift+F")
        .unwrap();

    let bindings = service.get_shortcut_bindings().unwrap();

    assert_eq!(
        bindings,
        defaults_with(ShortcutCommandId::TaskSearch, "Mod+Shift+F")
    );
    assert_eq!(
        fixture.stored_json(SHORTCUT_BINDINGS_KEY),
        Some(serde_json::json!({"taskSearch": "Mod+Shift+F"}))
    );
}

#[test]
fn legacy_platform_notation_is_folded_into_mod() {
    for (platform, primary_modifier, _) in PLATFORM_CASES {
        let fixture = SettingsFixture::new();
        let service = fixture.service(platform);
        service
            .set_app_setting(
                LEGACY_TASK_SEARCH_SHORTCUT_KEY,
                &format!("{primary_modifier}+Shift+F"),
            )
            .unwrap();

        let bindings = service.get_shortcut_bindings().unwrap();

        assert_eq!(
            bindings,
            defaults_with(ShortcutCommandId::TaskSearch, "Mod+Shift+F")
        );
        assert_eq!(
            fixture.stored_json(SHORTCUT_BINDINGS_KEY),
            Some(serde_json::json!({"taskSearch": "Mod+Shift+F"}))
        );
    }
}

#[test]
fn inherited_default_in_platform_notation_is_not_stored_as_an_override() {
    for (platform, primary_modifier, _) in PLATFORM_CASES {
        let fixture = SettingsFixture::new();
        let service = fixture.service(platform);
        service
            .set_app_setting(
                LEGACY_TASK_SEARCH_SHORTCUT_KEY,
                &format!("{primary_modifier}+Shift+O"),
            )
            .unwrap();

        assert_eq!(
            service.get_shortcut_bindings().unwrap(),
            ShortcutBindings::defaults()
        );
        assert_eq!(
            fixture.stored_json(SHORTCUT_BINDINGS_KEY),
            Some(serde_json::json!({}))
        );
    }
}

#[test]
fn legacy_value_with_a_non_mod_modifier_is_kept_as_an_override() {
    for (platform, _, opposite_modifier) in PLATFORM_CASES {
        let fixture = SettingsFixture::new();
        let service = fixture.service(platform);
        let legacy_shortcut = format!("{opposite_modifier}+Shift+O");
        service
            .set_app_setting(LEGACY_TASK_SEARCH_SHORTCUT_KEY, &legacy_shortcut)
            .unwrap();

        let bindings = service.get_shortcut_bindings().unwrap();

        assert_eq!(
            bindings,
            defaults_with(ShortcutCommandId::TaskSearch, &legacy_shortcut)
        );
        assert_eq!(
            fixture.stored_json(SHORTCUT_BINDINGS_KEY),
            Some(serde_json::json!({"taskSearch": legacy_shortcut}))
        );
    }
}

#[test]
fn legacy_value_without_a_key_is_not_inherited() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();
    service
        .set_app_setting(LEGACY_TASK_SEARCH_SHORTCUT_KEY, "Ctrl+Shift++")
        .unwrap();

    assert_eq!(
        service.get_shortcut_bindings().unwrap(),
        ShortcutBindings::defaults()
    );
    assert_eq!(
        fixture.stored_json(SHORTCUT_BINDINGS_KEY),
        Some(serde_json::json!({}))
    );
}

#[test]
fn table_written_after_inheritance_wins_over_the_legacy_value() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();
    service
        .set_app_setting(LEGACY_TASK_SEARCH_SHORTCUT_KEY, "Mod+Shift+F")
        .unwrap();
    service.get_shortcut_bindings().unwrap();

    service
        .set_shortcut_bindings(&ShortcutBindings::defaults())
        .unwrap();

    assert_eq!(
        service.get_shortcut_bindings().unwrap(),
        ShortcutBindings::defaults()
    );
}

#[test]
fn defaults_are_returned_without_being_stored() {
    let fixture = SettingsFixture::new();

    assert_eq!(
        fixture.linux_service().get_shortcut_bindings().unwrap(),
        ShortcutBindings::defaults()
    );
    assert_eq!(fixture.stored_json(SHORTCUT_BINDINGS_KEY), None);
}

#[test]
fn background_sync_starts_off() {
    let fixture = SettingsFixture::new();

    assert!(
        !fixture
            .linux_service()
            .get_background_sync_enabled()
            .unwrap()
    );
}

#[test]
fn background_sync_setting_is_kept() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();

    for is_enabled in [true, false] {
        service.set_background_sync_enabled(is_enabled).unwrap();
        assert_eq!(service.get_background_sync_enabled().unwrap(), is_enabled);
    }
}

#[test]
fn sidebar_hint_dismissal_is_stored_as_the_app_wide_true_value() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();
    assert!(!service.get_sidebar_hint_dismissed().unwrap());

    service.dismiss_sidebar_hint().unwrap();

    assert!(service.get_sidebar_hint_dismissed().unwrap());
    assert_eq!(
        service
            .get_app_setting("sidebar_hint_dismissed")
            .unwrap()
            .as_deref(),
        Some("true")
    );
}

#[test]
fn release_dismissals_are_trimmed_and_unique() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();

    service.dismiss_release_update_version(" 1.6.0 ").unwrap();
    service.dismiss_release_update_version("1.6.0").unwrap();
    service.dismiss_release_update_version("  ").unwrap();
    service.dismiss_release_update_version("v1.7.0").unwrap();

    assert_eq!(
        service
            .get_app_setting("release_update_dismissed_versions")
            .unwrap()
            .as_deref(),
        Some(r#"["1.6.0","v1.7.0"]"#)
    );
}

#[test]
fn numeric_settings_follow_the_ts_parsing_rules() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();
    assert_eq!(service.get_background_sync_interval_ms().unwrap(), 600_000);

    service
        .set_app_setting("background_sync_interval_ms", " 300000ms")
        .unwrap();
    assert_eq!(service.get_background_sync_interval_ms().unwrap(), 300_000);
    service
        .set_app_setting("background_sync_interval_ms", "-5")
        .unwrap();
    assert_eq!(service.get_background_sync_interval_ms().unwrap(), 600_000);

    assert_eq!(
        service
            .get_or_start_lan_plaintext_observation(1_000)
            .unwrap(),
        1_000
    );
    assert_eq!(
        service
            .get_or_start_lan_plaintext_observation(2_000)
            .unwrap(),
        1_000
    );
    assert_eq!(service.get_lan_plaintext_last_request_at().unwrap(), 0);
}

#[test]
fn notification_settings_default_to_enabled_progress_pending_review() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();
    let defaults = service.get_notification_settings().unwrap();
    assert!(defaults.is_enabled);
    assert_eq!(defaults.enabled_statuses, ["progress", "pending", "review"]);

    service.set_notification_enabled(false).unwrap();
    service
        .set_notification_statuses(&["review".to_string()])
        .unwrap();

    let saved = service.get_notification_settings().unwrap();
    assert!(!saved.is_enabled);
    assert_eq!(saved.enabled_statuses, ["review"]);
}

#[test]
fn hook_client_id_is_created_once() {
    let fixture = SettingsFixture::new();
    let service = fixture.linux_service();

    let first_client_id = service.get_or_create_hook_client_id().unwrap();

    assert_eq!(
        service.get_or_create_hook_client_id().unwrap(),
        first_client_id
    );
    assert_eq!(first_client_id.len(), 36);
}
