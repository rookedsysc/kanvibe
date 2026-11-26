//! 앱 전역 설정. 앱을 껐다 켜도 남아야 하는 UI 설정은 화면 상태가 아니라 여기 `app_settings`에 둔다.
//! 값 모양(`"true"`, 숫자 문자열, JSON)은 TS 앱과 같아서 두 앱이 같은 DB를 번갈아 열어도 설정이 유지된다.

use kanvibe_domain::board_sort::{
    BoardSortPreference, parse_board_sort_preference, serialize_board_sort_preference,
};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::bindings::{
    ShortcutBindings, ShortcutCommandId, ShortcutOverrides, parse_shortcut_overrides,
};
use kanvibe_domain::shortcuts::notation::canonicalize_shortcut_for_platform;
use kanvibe_domain::task::SessionType;

use crate::database::Database;
use crate::error::DatabaseResult;
use crate::repositories::app_settings::{find_app_setting, save_app_setting};

mod keys {
    pub const SIDEBAR_DEFAULT_COLLAPSED: &str = "sidebar_default_collapsed";
    pub const SIDEBAR_HINT_DISMISSED: &str = "sidebar_hint_dismissed";
    pub const NOTIFICATION_ENABLED: &str = "notification_enabled";
    pub const NOTIFICATION_STATUSES: &str = "notification_statuses";
    pub const NOTIFICATION_UNREAD_ONLY: &str = "notification_unread_only";
    pub const RELEASE_UPDATE_DISMISSED_VERSIONS: &str = "release_update_dismissed_versions";
    pub const LAN_PLAINTEXT_ALLOWED: &str = "lan_plaintext_allowed";
    pub const LAN_PLAINTEXT_OBSERVED_SINCE: &str = "lan_plaintext_observed_since";
    pub const LAN_PLAINTEXT_LAST_REQUEST_AT: &str = "lan_plaintext_last_request_at";
    pub const HOOK_CLIENT_ID: &str = "hook_client_id";
    pub const DONE_ALERT_DISMISSED: &str = "done_alert_dismissed";
    pub const BACKGROUND_SYNC_ENABLED: &str = "background_sync_enabled";
    pub const BACKGROUND_SYNC_INTERVAL_MS: &str = "background_sync_interval_ms";
    pub const DEFAULT_SESSION_TYPE: &str = "default_session_type";
    pub const SHORTCUT_BINDINGS: &str = "shortcut_bindings";
    /// 단축키 표가 생기기 전, 빠른 검색 하나만 따로 저장하던 키
    pub const LEGACY_TASK_SEARCH_SHORTCUT: &str = "task_search_shortcut";
    pub const VIM_MODE_ENABLED: &str = "vim_mode_enabled";
    pub const BOARD_SORT_PREFERENCE: &str = "board_sort_preference";
}

/// 사용자가 직접 옮기는 todo/done을 뺀 기본 알림 대상 상태
const DEFAULT_NOTIFICATION_STATUSES: [&str; 3] = ["progress", "pending", "review"];
const DEFAULT_BACKGROUND_SYNC_INTERVAL_MS: i64 = 10 * 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationSettings {
    pub is_enabled: bool,
    pub enabled_statuses: Vec<String>,
}

pub struct AppSettingsService<'database> {
    database: &'database Database,
    /// 단축키 표기를 접을 기준. 실행 중인 OS로 정하되 시험에서는 다른 플랫폼을 넘길 수 있다
    shortcut_platform: ShortcutPlatform,
}

fn is_true(value: Option<String>) -> bool {
    value.as_deref() == Some("true")
}

/// JS `parseInt(value, 10)`처럼 앞 공백을 건너뛰고 맨 앞 정수만 읽는다
fn parse_leading_integer(value: &str) -> Option<i64> {
    let trimmed = value.trim_start();
    let sign_length = usize::from(trimmed.starts_with(['-', '+']));
    let digit_length = trimmed[sign_length..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    trimmed[..sign_length + digit_length].parse().ok()
}

/// JS `Number(value)`처럼 숫자 문자열 전체를 읽는다. 빈 값은 0, 숫자가 아니면 `None`이다
fn parse_number(value: Option<String>) -> Option<f64> {
    let Some(text) = value else {
        return Some(0.0);
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Some(0.0)
    } else {
        trimmed.parse().ok()
    }
}

impl<'database> AppSettingsService<'database> {
    pub fn new(database: &'database Database, shortcut_platform: ShortcutPlatform) -> Self {
        Self {
            database,
            shortcut_platform,
        }
    }

    pub fn get_app_setting(&self, key: &str) -> DatabaseResult<Option<String>> {
        let setting = find_app_setting(&self.database.connection(), key)?;
        Ok(setting.map(|setting| setting.value))
    }

    pub fn set_app_setting(&self, key: &str, value: &str) -> DatabaseResult<()> {
        save_app_setting(&self.database.connection(), key, value)
    }

    pub fn get_sidebar_default_collapsed(&self) -> DatabaseResult<bool> {
        Ok(is_true(
            self.get_app_setting(keys::SIDEBAR_DEFAULT_COLLAPSED)?,
        ))
    }

    pub fn set_sidebar_default_collapsed(&self, is_collapsed: bool) -> DatabaseResult<()> {
        self.set_app_setting(keys::SIDEBAR_DEFAULT_COLLAPSED, &is_collapsed.to_string())
    }

    pub fn get_sidebar_hint_dismissed(&self) -> DatabaseResult<bool> {
        Ok(is_true(self.get_app_setting(keys::SIDEBAR_HINT_DISMISSED)?))
    }

    /// 사이드바 접기 힌트를 다시 보지 않는다. 앱 전역 설정이라 어느 화면에서 닫아도 모든 화면에 반영된다
    pub fn dismiss_sidebar_hint(&self) -> DatabaseResult<()> {
        self.set_app_setting(keys::SIDEBAR_HINT_DISMISSED, "true")
    }

    /// 사용자가 LAN 평문 포트를 임시로 계속 열어 두기로 했는지
    pub fn get_lan_plaintext_allowed(&self) -> DatabaseResult<bool> {
        Ok(is_true(self.get_app_setting(keys::LAN_PLAINTEXT_ALLOWED)?))
    }

    pub fn set_lan_plaintext_allowed(&self, is_allowed: bool) -> DatabaseResult<()> {
        self.set_app_setting(keys::LAN_PLAINTEXT_ALLOWED, &is_allowed.to_string())
    }

    /// LAN 평문 사용량을 재기 시작한 시각(ms). 처음 부르면 지금으로 기록한다
    pub fn get_or_start_lan_plaintext_observation(&self, now_ms: i64) -> DatabaseResult<i64> {
        let connection = self.database.connection();
        let stored_value = find_app_setting(&connection, keys::LAN_PLAINTEXT_OBSERVED_SINCE)?
            .map(|setting| setting.value);
        if let Some(observed_since) = parse_number(stored_value).filter(|&since| since > 0.0) {
            return Ok(observed_since as i64);
        }
        save_app_setting(
            &connection,
            keys::LAN_PLAINTEXT_OBSERVED_SINCE,
            &now_ms.to_string(),
        )?;
        Ok(now_ms)
    }

    pub fn get_lan_plaintext_last_request_at(&self) -> DatabaseResult<i64> {
        let stored_value = self.get_app_setting(keys::LAN_PLAINTEXT_LAST_REQUEST_AT)?;
        Ok(parse_number(stored_value)
            .filter(|time| time.is_finite())
            .unwrap_or(0.0) as i64)
    }

    pub fn set_lan_plaintext_last_request_at(&self, requested_at_ms: i64) -> DatabaseResult<()> {
        self.set_app_setting(
            keys::LAN_PLAINTEXT_LAST_REQUEST_AT,
            &requested_at_ms.to_string(),
        )
    }

    /// 원격 터널 소켓 이름으로 쓰는 이 설치의 ID. IP가 바뀌어도 그대로라 원격 스크립트가 같은 소켓을 찾는다.
    /// 조회와 생성을 한 잠금 안에서 해 동시에 처음 불려도 ID가 둘 생기지 않는다.
    pub fn get_or_create_hook_client_id(&self) -> DatabaseResult<String> {
        let connection = self.database.connection();
        let stored_client_id = find_app_setting(&connection, keys::HOOK_CLIENT_ID)?
            .map(|setting| setting.value)
            .filter(|client_id| !client_id.is_empty());
        if let Some(client_id) = stored_client_id {
            return Ok(client_id);
        }
        let client_id = uuid::Uuid::new_v4().to_string();
        save_app_setting(&connection, keys::HOOK_CLIENT_ID, &client_id)?;
        Ok(client_id)
    }

    pub fn get_done_alert_dismissed(&self) -> DatabaseResult<bool> {
        Ok(is_true(self.get_app_setting(keys::DONE_ALERT_DISMISSED)?))
    }

    pub fn dismiss_done_alert(&self) -> DatabaseResult<()> {
        self.set_app_setting(keys::DONE_ALERT_DISMISSED, "true")
    }
}

/// 저장된 값에서 문자열 버전만 앞뒤 공백을 걷어 중복 없이 남긴다. 깨진 값은 빈 목록이다
fn parse_dismissed_release_versions(value: Option<&str>) -> Vec<String> {
    let Some(Ok(serde_json::Value::Array(stored_versions))) = value.map(serde_json::from_str)
    else {
        return Vec::new();
    };
    let mut versions: Vec<String> = Vec::new();
    let trimmed_versions = stored_versions
        .iter()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim);
    for version in trimmed_versions.filter(|version| !version.is_empty()) {
        if !versions.iter().any(|known| known == version) {
            versions.push(version.to_string());
        }
    }
    versions
}

fn to_json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).expect("설정 값은 항상 JSON으로 직렬화된다")
}

impl AppSettingsService<'_> {
    pub fn get_release_update_dismissed_versions(&self) -> DatabaseResult<Vec<String>> {
        let stored_value = self.get_app_setting(keys::RELEASE_UPDATE_DISMISSED_VERSIONS)?;
        Ok(parse_dismissed_release_versions(stored_value.as_deref()))
    }

    /// 이 릴리스 버전의 업데이트 안내를 다시 보지 않는다
    pub fn dismiss_release_update_version(&self, version: &str) -> DatabaseResult<()> {
        let normalized_version = version.trim();
        if normalized_version.is_empty() {
            return Ok(());
        }
        let mut dismissed_versions = self.get_release_update_dismissed_versions()?;
        if dismissed_versions
            .iter()
            .any(|dismissed| dismissed == normalized_version)
        {
            return Ok(());
        }
        dismissed_versions.push(normalized_version.to_string());
        self.set_app_setting(
            keys::RELEASE_UPDATE_DISMISSED_VERSIONS,
            &to_json(&dismissed_versions),
        )
    }

    /// 키가 없으면 알림을 켜고 기본 상태들을 대상으로 한다. 상태 목록을 읽지 못해도 기본 상태로 돌아간다
    pub fn get_notification_settings(&self) -> DatabaseResult<NotificationSettings> {
        let is_enabled =
            self.get_app_setting(keys::NOTIFICATION_ENABLED)?.as_deref() != Some("false");
        let stored_statuses = self.get_app_setting(keys::NOTIFICATION_STATUSES)?;
        let enabled_statuses = stored_statuses
            .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
            .unwrap_or_else(|| DEFAULT_NOTIFICATION_STATUSES.map(String::from).to_vec());
        Ok(NotificationSettings {
            is_enabled,
            enabled_statuses,
        })
    }

    pub fn set_notification_enabled(&self, is_enabled: bool) -> DatabaseResult<()> {
        self.set_app_setting(keys::NOTIFICATION_ENABLED, &is_enabled.to_string())
    }

    pub fn set_notification_statuses(&self, statuses: &[String]) -> DatabaseResult<()> {
        self.set_app_setting(keys::NOTIFICATION_STATUSES, &to_json(&statuses))
    }

    /// 알림 목록을 안읽음만 보기로 볼지. 저장한 적이 없으면 전체 보기로 시작한다
    pub fn get_notification_unread_only_enabled(&self) -> DatabaseResult<bool> {
        Ok(is_true(
            self.get_app_setting(keys::NOTIFICATION_UNREAD_ONLY)?,
        ))
    }

    pub fn set_notification_unread_only_enabled(&self, is_enabled: bool) -> DatabaseResult<()> {
        self.set_app_setting(keys::NOTIFICATION_UNREAD_ONLY, &is_enabled.to_string())
    }

    /// 백그라운드 sync는 사용자가 명시적으로 켠 경우에만 돈다
    pub fn get_background_sync_enabled(&self) -> DatabaseResult<bool> {
        Ok(is_true(
            self.get_app_setting(keys::BACKGROUND_SYNC_ENABLED)?,
        ))
    }

    pub fn set_background_sync_enabled(&self, is_enabled: bool) -> DatabaseResult<()> {
        self.set_app_setting(keys::BACKGROUND_SYNC_ENABLED, &is_enabled.to_string())
    }

    /// 백그라운드 sync 주기(ms). 없거나 양수가 아니면 10분이다
    pub fn get_background_sync_interval_ms(&self) -> DatabaseResult<i64> {
        let stored_value = self.get_app_setting(keys::BACKGROUND_SYNC_INTERVAL_MS)?;
        let interval_ms = stored_value
            .as_deref()
            .and_then(parse_leading_integer)
            .filter(|&interval_ms| interval_ms > 0)
            .unwrap_or(DEFAULT_BACKGROUND_SYNC_INTERVAL_MS);
        Ok(interval_ms)
    }

    pub fn set_background_sync_interval_ms(&self, interval_ms: i64) -> DatabaseResult<()> {
        self.set_app_setting(keys::BACKGROUND_SYNC_INTERVAL_MS, &interval_ms.to_string())
    }

    /// 기본 세션 종류. 없거나 모르는 값이면 Herdr다
    pub fn get_default_session_type(&self) -> DatabaseResult<SessionType> {
        let stored_value = self.get_app_setting(keys::DEFAULT_SESSION_TYPE)?;
        Ok(stored_value
            .as_deref()
            .and_then(SessionType::parse)
            .unwrap_or(SessionType::Herdr))
    }

    pub fn set_default_session_type(&self, session_type: SessionType) -> DatabaseResult<()> {
        self.set_app_setting(keys::DEFAULT_SESSION_TYPE, session_type.as_str())
    }

    /// 보드 vim 이동. 저장한 적이 없으면 켜져 있다
    pub fn get_vim_mode_enabled(&self) -> DatabaseResult<bool> {
        Ok(self.get_app_setting(keys::VIM_MODE_ENABLED)?.as_deref() != Some("false"))
    }

    pub fn set_vim_mode_enabled(&self, is_enabled: bool) -> DatabaseResult<()> {
        self.set_app_setting(keys::VIM_MODE_ENABLED, &is_enabled.to_string())
    }

    pub fn get_board_sort_preference(&self) -> DatabaseResult<BoardSortPreference> {
        let stored_value = self.get_app_setting(keys::BOARD_SORT_PREFERENCE)?;
        Ok(parse_board_sort_preference(stored_value.as_deref()))
    }

    pub fn set_board_sort_preference(
        &self,
        preference: &BoardSortPreference,
    ) -> DatabaseResult<()> {
        self.set_app_setting(
            keys::BOARD_SORT_PREFERENCE,
            &serialize_board_sort_preference(preference),
        )
    }

    /// 재배정까지 반영한 단축키 표.
    /// 표가 없으면 옛 `task_search_shortcut` 값을 빠른 검색 재배정으로 승계하고 그 결과를 한 번 저장한다.
    /// 승계하지 않으면 옛 버전에서 빠른 검색을 바꿔 둔 사용자의 단축키가 업그레이드 순간 말없이 되돌아간다.
    pub fn get_shortcut_bindings(&self) -> DatabaseResult<ShortcutBindings> {
        if let Some(stored_overrides) = self.get_app_setting(keys::SHORTCUT_BINDINGS)? {
            let overrides =
                parse_shortcut_overrides(Some(&stored_overrides), self.shortcut_platform);
            return Ok(ShortcutBindings::with_overrides(&overrides));
        }
        let legacy_shortcut = self.get_app_setting(keys::LEGACY_TASK_SEARCH_SHORTCUT)?;
        let Some(legacy_shortcut) = legacy_shortcut.filter(|shortcut| !shortcut.is_empty()) else {
            return Ok(ShortcutBindings::defaults());
        };

        // 옛 값은 `Meta+…`/`Ctrl+…` 표기라 접어야 중복 판정이 같은 조합을 알아본다.
        // 접은 결과가 비면 키가 사라진 값이라, 얹으면 빠른 검색 기본값까지 지워지므로 버린다.
        let migrated_shortcut =
            canonicalize_shortcut_for_platform(&legacy_shortcut, self.shortcut_platform);
        let mut overrides = ShortcutOverrides::new();
        if !migrated_shortcut.is_empty() {
            overrides.insert(ShortcutCommandId::TaskSearch, migrated_shortcut);
        }
        let migrated_bindings = ShortcutBindings::with_overrides(&overrides);
        self.set_shortcut_bindings(&migrated_bindings)?;
        Ok(migrated_bindings)
    }

    /// 단축키 표를 저장한다. 기본값과 같은 항목은 빼고 재배정만 남긴다
    pub fn set_shortcut_bindings(&self, bindings: &ShortcutBindings) -> DatabaseResult<()> {
        let overrides = bindings.collect_overrides(self.shortcut_platform);
        self.set_app_setting(keys::SHORTCUT_BINDINGS, &to_json(&overrides))
    }

    pub fn get_task_search_shortcut(&self) -> DatabaseResult<String> {
        let bindings = self.get_shortcut_bindings()?;
        Ok(bindings.shortcut(ShortcutCommandId::TaskSearch).to_string())
    }
}

#[cfg(test)]
mod tests;
