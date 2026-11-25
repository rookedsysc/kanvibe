//! 설정 화면 값과 저장 상태(TS `ProjectSettings`의 상태 부분). 화면 값을 먼저 바꾸고 저장하며,
//! 저장이 실패하면 그 항목만 마지막 값으로 되돌리고 실패를 그 항목 아래에 남긴다. 한 항목을 저장하는 동안
//! 그 항목만 막고 다른 항목은 바꿀 수 있다.

use std::collections::{HashSet, VecDeque};

use kanvibe_domain::task::SessionType;
use serde::{Deserialize, Serialize};

use crate::theme::ThemePreference;

pub const MIN_SYNC_INTERVAL_MINUTES: i64 = 1;
pub const MAX_SYNC_INTERVAL_MINUTES: i64 = 1440;
const MS_PER_MINUTE: i64 = 60_000;

/// 알림 대상 상태(사용자가 직접 옮기는 todo/done은 뺀다). 라벨은 `board.columns.<상태>`
pub const NOTIFICATION_STATUS_OPTIONS: [&str; 3] = ["progress", "pending", "review"];

/// 저장 상태를 따로 보이는 설정 항목
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingKey {
    Theme,
    Sidebar,
    SessionType,
    Notifications,
    BackgroundSync,
    VimMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPreferences {
    pub is_enabled: bool,
    pub enabled_statuses: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSyncPreferences {
    pub is_enabled: bool,
    pub interval_ms: i64,
}

/// 설정 화면이 보이는 값 묶음(TS `SettingsData`에서 화면이 쓰는 부분)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsValues {
    pub theme_preference: ThemePreference,
    pub is_sidebar_default_collapsed: bool,
    pub default_session_type: SessionType,
    pub is_vim_mode_enabled: bool,
    pub notifications: NotificationPreferences,
    pub background_sync: BackgroundSyncPreferences,
}

/// 사용자가 바꾼 값 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingChange {
    Theme(ThemePreference),
    SidebarDefaultCollapsed(bool),
    DefaultSessionType(SessionType),
    VimModeEnabled(bool),
    NotificationEnabled(bool),
    NotificationStatuses(Vec<String>),
    BackgroundSyncEnabled(bool),
    BackgroundSyncIntervalMs(i64),
}

impl SettingChange {
    pub fn key(&self) -> SettingKey {
        match self {
            Self::Theme(_) => SettingKey::Theme,
            Self::SidebarDefaultCollapsed(_) => SettingKey::Sidebar,
            Self::DefaultSessionType(_) => SettingKey::SessionType,
            Self::VimModeEnabled(_) => SettingKey::VimMode,
            Self::NotificationEnabled(_) | Self::NotificationStatuses(_) => {
                SettingKey::Notifications
            }
            Self::BackgroundSyncEnabled(_) | Self::BackgroundSyncIntervalMs(_) => {
                SettingKey::BackgroundSync
            }
        }
    }

    fn apply(&self, values: &mut SettingsValues) {
        match self {
            Self::Theme(preference) => values.theme_preference = *preference,
            Self::SidebarDefaultCollapsed(is_collapsed) => {
                values.is_sidebar_default_collapsed = *is_collapsed
            }
            Self::DefaultSessionType(session_type) => values.default_session_type = *session_type,
            Self::VimModeEnabled(is_enabled) => values.is_vim_mode_enabled = *is_enabled,
            Self::NotificationEnabled(is_enabled) => values.notifications.is_enabled = *is_enabled,
            Self::NotificationStatuses(statuses) => {
                values.notifications.enabled_statuses = statuses.clone()
            }
            Self::BackgroundSyncEnabled(is_enabled) => {
                values.background_sync.is_enabled = *is_enabled
            }
            Self::BackgroundSyncIntervalMs(interval_ms) => {
                values.background_sync.interval_ms = *interval_ms
            }
        }
    }
}

/// 한 항목의 값 묶음을 다른 값 묶음에서 그대로 옮긴다(되돌리기·새로 고침 반영)
fn copy_setting(key: SettingKey, source: &SettingsValues, target: &mut SettingsValues) {
    match key {
        SettingKey::Theme => target.theme_preference = source.theme_preference,
        SettingKey::Sidebar => {
            target.is_sidebar_default_collapsed = source.is_sidebar_default_collapsed
        }
        SettingKey::SessionType => target.default_session_type = source.default_session_type,
        SettingKey::VimMode => target.is_vim_mode_enabled = source.is_vim_mode_enabled,
        SettingKey::Notifications => target.notifications = source.notifications.clone(),
        SettingKey::BackgroundSync => target.background_sync = source.background_sync,
    }
}

const ALL_SETTING_KEYS: [SettingKey; 6] = [
    SettingKey::Theme,
    SettingKey::Sidebar,
    SettingKey::SessionType,
    SettingKey::Notifications,
    SettingKey::BackgroundSync,
    SettingKey::VimMode,
];

/// 저장에 실패한 항목. 같은 변경을 다시 시도할 수 있게 변경을 들고 있는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingSaveFailure {
    pub key: SettingKey,
    pub reason: String,
    pub change: SettingChange,
}

/// 진행 중인 저장 하나. 실패하면 `previous`로 그 항목을 되돌린다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSave {
    pub change: SettingChange,
    previous: SettingsValues,
}

pub struct SettingsForm {
    pub values: SettingsValues,
    saving_keys: HashSet<SettingKey>,
    pub failure: Option<SettingSaveFailure>,
    /// 주기 입력란 글자. 숫자가 아니어도 입력 중에는 그대로 둔다
    pub interval_input: String,
    /// 주기 저장은 앞 저장이 끝난 뒤 차례로 보낸다
    queued_interval_saves: VecDeque<PendingSave>,
    is_interval_save_running: bool,
}

impl SettingsForm {
    pub fn new(values: SettingsValues) -> Self {
        let interval_input = format_sync_interval_minutes(values.background_sync.interval_ms);
        Self {
            values,
            saving_keys: HashSet::new(),
            failure: None,
            interval_input,
            queued_interval_saves: VecDeque::new(),
            is_interval_save_running: false,
        }
    }

    pub fn is_saving(&self, key: SettingKey) -> bool {
        self.saving_keys.contains(&key)
    }

    pub fn failure_for(&self, key: SettingKey) -> Option<&SettingSaveFailure> {
        self.failure.as_ref().filter(|failure| failure.key == key)
    }

    /// 화면 값을 먼저 바꾸고 그 항목을 저장 중으로 둔다. 같은 항목의 지난 실패는 지운다
    pub fn begin_save(&mut self, change: SettingChange) -> PendingSave {
        let previous = self.values.clone();
        let key = change.key();
        change.apply(&mut self.values);
        self.saving_keys.insert(key);
        if self
            .failure
            .as_ref()
            .is_some_and(|failure| failure.key == key)
        {
            self.failure = None;
        }
        PendingSave { change, previous }
    }

    /// 저장 결과를 반영한다. 실패하면 그 항목을 되돌리고 실패를 남긴다. 되돌렸으면 `true`
    pub fn finish_save(&mut self, pending: PendingSave, result: Result<(), String>) -> bool {
        let key = pending.change.key();
        self.saving_keys.remove(&key);
        let Err(reason) = result else {
            return false;
        };
        copy_setting(key, &pending.previous, &mut self.values);
        if key == SettingKey::BackgroundSync {
            self.interval_input =
                format_sync_interval_minutes(self.values.background_sync.interval_ms);
        }
        self.failure = Some(SettingSaveFailure {
            key,
            reason,
            change: pending.change,
        });
        true
    }

    /// 다시 읽은 값을 반영한다. 저장 중인 항목은 방금 바꾼 값을 지키려고 덮지 않는다
    pub fn apply_refresh(&mut self, fresh: &SettingsValues) {
        for key in ALL_SETTING_KEYS {
            if !self.is_saving(key) {
                copy_setting(key, fresh, &mut self.values);
            }
        }
        if !self.is_saving(SettingKey::BackgroundSync) {
            self.interval_input =
                format_sync_interval_minutes(self.values.background_sync.interval_ms);
        }
    }

    /// 주기 입력란이 바뀌었다. 1~1440 정수이고 지금 값과 다르면 저장할 변경을 돌려준다
    pub fn edit_interval_input(&mut self, text: &str) -> Option<SettingChange> {
        self.interval_input = text.to_string();
        let minutes = parse_sync_interval_minutes(text)?;
        let interval_ms = minutes * MS_PER_MINUTE;
        (interval_ms != self.values.background_sync.interval_ms)
            .then_some(SettingChange::BackgroundSyncIntervalMs(interval_ms))
    }

    /// 입력란에서 벗어날 때 쓸 수 없는 글자가 남아 있으면 지금 값으로 되돌린다
    pub fn blur_interval_input(&mut self) {
        if parse_sync_interval_minutes(&self.interval_input).is_none() {
            self.interval_input =
                format_sync_interval_minutes(self.values.background_sync.interval_ms);
        }
    }

    /// 주기 저장을 줄 세운다. 지금 바로 보낼 저장이 있으면 돌려준다
    pub fn enqueue_interval_save(&mut self, pending: PendingSave) -> Option<PendingSave> {
        self.queued_interval_saves.push_back(pending);
        self.take_next_interval_save()
    }

    /// 앞 주기 저장이 끝났다. 다음으로 보낼 저장이 있으면 돌려준다
    pub fn finish_interval_save(&mut self) -> Option<PendingSave> {
        self.is_interval_save_running = false;
        self.take_next_interval_save()
    }

    fn take_next_interval_save(&mut self) -> Option<PendingSave> {
        if self.is_interval_save_running {
            return None;
        }
        let next = self.queued_interval_saves.pop_front()?;
        self.is_interval_save_running = true;
        Some(next)
    }
}

pub fn format_sync_interval_minutes(interval_ms: i64) -> String {
    ((interval_ms as f64) / MS_PER_MINUTE as f64)
        .round()
        .to_string()
}

/// 앞뒤 공백을 뺀 숫자만 받고 1~1440분 밖이면 버린다
pub fn parse_sync_interval_minutes(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let minutes: i64 = trimmed.parse().ok()?;
    (MIN_SYNC_INTERVAL_MINUTES..=MAX_SYNC_INTERVAL_MINUTES)
        .contains(&minutes)
        .then_some(minutes)
}

/// 알림 상태 칩을 눌렀을 때의 새 목록. 이미 있으면 빼고 없으면 뒤에 붙인다
pub fn toggle_notification_status(current: &[String], status: &str) -> Vec<String> {
    if current.iter().any(|selected| selected == status) {
        current
            .iter()
            .filter(|selected| selected.as_str() != status)
            .cloned()
            .collect()
    } else {
        let mut next = current.to_vec();
        next.push(status.to_string());
        next
    }
}
