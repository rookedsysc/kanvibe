//! 데스크탑과 같은 `app_settings` 저장소를 여는 곳.

use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::platform::ShortcutPlatform;

/// 모바일 경로는 단축키 설정을 읽거나 쓰지 않으므로, 단축키 표에만 쓰이는 플랫폼 값은 결과에 영향이 없다
pub(crate) fn open_app_settings(database: &Database) -> AppSettingsService<'_> {
    AppSettingsService::new(database, ShortcutPlatform::Linux)
}
