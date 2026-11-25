//! 앱 설정 키 하나를 읽고 쓰는 자리. TS `getAppSetting`·`setAppSetting`에 해당하고, 앱에서는 `kanvibe.db`의 `app_settings`를 쓴다.

use crate::error::{AiError, AiResult};

pub trait AppSettingStore: Send + Sync {
    fn get_app_setting(&self, key: &str) -> AiResult<Option<String>>;
    fn set_app_setting(&self, key: &str, value: &str) -> AiResult<()>;
}

impl AppSettingStore for kanvibe_db::Database {
    fn get_app_setting(&self, key: &str) -> AiResult<Option<String>> {
        let setting =
            kanvibe_db::repositories::app_settings::find_app_setting(&self.connection(), key)
                .map_err(|error| AiError::message(error.to_string()))?;
        Ok(setting.map(|setting| setting.value))
    }

    fn set_app_setting(&self, key: &str, value: &str) -> AiResult<()> {
        kanvibe_db::repositories::app_settings::save_app_setting(&self.connection(), key, value)
            .map_err(|error| AiError::message(error.to_string()))
    }
}

#[cfg(test)]
pub(crate) mod memory {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// 저장한 값을 다음 조회가 그대로 보는 메모리 저장소. 정한 횟수만큼 저장을 실패시킬 수 있다
    #[derive(Default)]
    pub struct MemorySettings {
        pub values: Mutex<HashMap<String, String>>,
        pub failing_writes: AtomicUsize,
        pub is_read_only: std::sync::atomic::AtomicBool,
        pub write_count: AtomicUsize,
    }

    impl AppSettingStore for MemorySettings {
        fn get_app_setting(&self, key: &str) -> AiResult<Option<String>> {
            Ok(self.values.lock().unwrap().get(key).cloned())
        }

        fn set_app_setting(&self, key: &str, value: &str) -> AiResult<()> {
            self.write_count.fetch_add(1, Ordering::SeqCst);
            if self.is_read_only.load(Ordering::SeqCst) {
                return Err(AiError::message("database is locked"));
            }
            let mut values = self.values.lock().unwrap();
            if self.failing_writes.load(Ordering::SeqCst) > 0 {
                self.failing_writes.fetch_sub(1, Ordering::SeqCst);
                return Err(AiError::message("save failed"));
            }
            values.insert(key.to_string(), value.to_string());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_db::{Database, DatabaseLocation};

    #[test]
    fn database_store_round_trips_values() {
        let directory = tempfile::tempdir().unwrap();
        let database = Database::open(&DatabaseLocation::in_app_data_directory(
            directory.path(),
            None,
        ))
        .unwrap();
        assert_eq!(database.get_app_setting("ai_accounts").unwrap(), None);
        database.set_app_setting("ai_accounts", "[]").unwrap();
        assert_eq!(
            database.get_app_setting("ai_accounts").unwrap().as_deref(),
            Some("[]")
        );
    }
}
