//! 설정 화면의 AppSettings 읽기·쓰기. 키와 저장 형식은 `AppSettingsService`(TS `appSettingsService.ts`)를 그대로 쓴다.
//! 고장 주입 대상 이름은 TS IPC `appSettings.<메서드>`다.

use gpui_kit::{App, AppContext, Task};
use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::platform::ShortcutPlatform;

use crate::services::{AppServices, ServiceResult};
use crate::theme::ThemePreference;
use crate::views::settings::form::{
    BackgroundSyncPreferences, NotificationPreferences, SettingChange, SettingsValues,
};
use crate::views::settings::service_call::{call_database_read, call_database_service, failure};

/// Pane 채팅 기본 모드(TS `usePaneChatPreferences`의 `PANE_CHAT_DEFAULT_KEY`)
pub const PANE_CHAT_DEFAULT_KEY: &str = "pane_chat_default_enabled";

/// TS `SettingsRoute`가 한 번에 부르던 조회. 하나라도 실패하면 화면 전체가 실패다
const LOAD_TARGETS: [&str; 6] = [
    "appSettings.getSidebarDefaultCollapsed",
    "appSettings.getNotificationSettings",
    "appSettings.getDefaultSessionType",
    "appSettings.getVimModeEnabled",
    "appSettings.getThemePreference",
    "appSettings.getBackgroundSyncSettings",
];

fn settings_service(database: &Database, platform: ShortcutPlatform) -> AppSettingsService<'_> {
    AppSettingsService::new(database, platform)
}

/// 설정 화면 값을 읽는다. 조회 하나씩 고장 판정을 거친다
pub fn load_settings_values(cx: &App) -> Task<ServiceResult<SettingsValues>> {
    let platform = cx.global::<AppServices>().shortcut_platform;
    let (read_target, check_targets) = LOAD_TARGETS.split_last().expect("조회 대상이 있다");
    let checks: Vec<Task<ServiceResult<()>>> = check_targets
        .iter()
        .map(|target| call_database_read(cx, target, |_| Ok(())))
        .collect();
    let read = call_database_read(cx, read_target, move |database| {
        read_settings_values(database, platform)
    });
    cx.background_spawn(async move {
        for check in checks {
            check.await?;
        }
        read.await
    })
}

fn read_settings_values(
    database: &Database,
    platform: ShortcutPlatform,
) -> ServiceResult<SettingsValues> {
    let settings = settings_service(database, platform);
    let notification = settings.get_notification_settings().map_err(failure)?;
    Ok(SettingsValues {
        theme_preference: ThemePreference::parse(
            settings
                .get_app_setting(ThemePreference::APP_SETTING_KEY)
                .map_err(failure)?
                .as_deref(),
        ),
        is_sidebar_default_collapsed: settings.get_sidebar_default_collapsed().map_err(failure)?,
        default_session_type: settings.get_default_session_type().map_err(failure)?,
        is_vim_mode_enabled: settings.get_vim_mode_enabled().map_err(failure)?,
        notifications: NotificationPreferences {
            is_enabled: notification.is_enabled,
            enabled_statuses: notification.enabled_statuses,
        },
        background_sync: BackgroundSyncPreferences {
            is_enabled: settings.get_background_sync_enabled().map_err(failure)?,
            interval_ms: settings
                .get_background_sync_interval_ms()
                .map_err(failure)?,
        },
    })
}

/// 변경 하나를 저장한다. 대상 이름은 그 값을 저장하던 TS setter다
pub fn save_setting_change(change: &SettingChange, cx: &App) -> Task<ServiceResult<()>> {
    let platform = cx.global::<AppServices>().shortcut_platform;
    let (target, arguments) = setting_change_target(change);
    let change = change.clone();
    call_database_service(cx, target, arguments, move |database| {
        write_setting_change(&settings_service(database, platform), &change).map_err(failure)
    })
}

fn setting_change_target(change: &SettingChange) -> (&'static str, String) {
    let json = |value: serde_json::Value| format!("[{value}]");
    match change {
        SettingChange::Theme(preference) => (
            "appSettings.setThemePreference",
            json(theme_preference_value(*preference).into()),
        ),
        SettingChange::SidebarDefaultCollapsed(value) => (
            "appSettings.setSidebarDefaultCollapsed",
            json((*value).into()),
        ),
        SettingChange::DefaultSessionType(session_type) => (
            "appSettings.setDefaultSessionType",
            json(session_type.as_str().into()),
        ),
        SettingChange::VimModeEnabled(value) => {
            ("appSettings.setVimModeEnabled", json((*value).into()))
        }
        SettingChange::NotificationEnabled(value) => {
            ("appSettings.setNotificationEnabled", json((*value).into()))
        }
        SettingChange::NotificationStatuses(statuses) => (
            "appSettings.setNotificationStatuses",
            json(serde_json::json!(statuses)),
        ),
        SettingChange::BackgroundSyncEnabled(value) => (
            "appSettings.setBackgroundSyncEnabled",
            json((*value).into()),
        ),
        SettingChange::BackgroundSyncIntervalMs(value) => (
            "appSettings.setBackgroundSyncIntervalMs",
            json((*value).into()),
        ),
    }
}

pub fn theme_preference_value(preference: ThemePreference) -> &'static str {
    match preference {
        ThemePreference::System => "system",
        ThemePreference::Dark => "dark",
        ThemePreference::Light => "light",
    }
}

fn write_setting_change(
    settings: &AppSettingsService<'_>,
    change: &SettingChange,
) -> kanvibe_db::DatabaseResult<()> {
    match change {
        SettingChange::Theme(preference) => settings.set_app_setting(
            ThemePreference::APP_SETTING_KEY,
            theme_preference_value(*preference),
        ),
        SettingChange::SidebarDefaultCollapsed(value) => {
            settings.set_sidebar_default_collapsed(*value)
        }
        SettingChange::DefaultSessionType(session_type) => {
            settings.set_default_session_type(*session_type)
        }
        SettingChange::VimModeEnabled(value) => settings.set_vim_mode_enabled(*value),
        SettingChange::NotificationEnabled(value) => settings.set_notification_enabled(*value),
        SettingChange::NotificationStatuses(statuses) => {
            settings.set_notification_statuses(statuses)
        }
        SettingChange::BackgroundSyncEnabled(value) => settings.set_background_sync_enabled(*value),
        SettingChange::BackgroundSyncIntervalMs(value) => {
            settings.set_background_sync_interval_ms(*value)
        }
    }
}

/// `pane_chat_default_enabled`를 읽는다(`"true"`일 때만 켜짐)
pub fn load_pane_chat_default_enabled(cx: &App) -> Task<ServiceResult<bool>> {
    let platform = cx.global::<AppServices>().shortcut_platform;
    call_database_service(
        cx,
        "appSettings.getAppSetting",
        format!("[\"{PANE_CHAT_DEFAULT_KEY}\"]"),
        move |database| {
            let stored = settings_service(database, platform)
                .get_app_setting(PANE_CHAT_DEFAULT_KEY)
                .map_err(failure)?;
            Ok(stored.as_deref() == Some("true"))
        },
    )
}

pub fn save_pane_chat_default_enabled(is_enabled: bool, cx: &App) -> Task<ServiceResult<()>> {
    let platform = cx.global::<AppServices>().shortcut_platform;
    call_database_service(
        cx,
        "appSettings.setAppSetting",
        format!("[\"{PANE_CHAT_DEFAULT_KEY}\",\"{is_enabled}\"]"),
        move |database| {
            settings_service(database, platform)
                .set_app_setting(PANE_CHAT_DEFAULT_KEY, &is_enabled.to_string())
                .map_err(failure)
        },
    )
}
