//! task CLI 토큰(TS `taskCliTokenService.ts`). 토큰은 `HMAC-SHA256(task_cli_secret, taskId)`의 hex다.
//! 비밀값과 taskId로 다시 계산할 수 있으므로 토큰 목록을 따로 저장하지 않는다.

use std::sync::Mutex;

use hmac::{Hmac, Mac};
use kanvibe_db::repositories::app_settings::{find_app_setting, save_app_setting};
use kanvibe_db::{Database, DatabaseError};
use sha2::Sha256;

/// task CLI 토큰을 서명하는 이 client 고유의 비밀값이 저장된 app_settings 키
pub const TASK_CLI_SECRET_SETTING_KEY: &str = "task_cli_secret";

const TASK_CLI_SECRET_BYTES: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum TaskCliTokenError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error("task CLI 비밀값을 만들 난수를 얻지 못했습니다: {0}")]
    Random(String),
}

pub struct TaskCliTokenService<'database> {
    database: &'database Database,
    /// 처음 읽은 비밀값. 잠금을 쥔 채 만들고 저장하므로 동시에 처음 발급해도 비밀값을 한 번만 저장한다
    secret: Mutex<Option<String>>,
}

fn create_secret() -> Result<String, TaskCliTokenError> {
    let mut secret_bytes = [0_u8; TASK_CLI_SECRET_BYTES];
    getrandom::fill(&mut secret_bytes)
        .map_err(|error| TaskCliTokenError::Random(error.to_string()))?;
    Ok(hex::encode(secret_bytes))
}

/// 길이가 같을 때만 모든 바이트를 비교해 일치 여부가 비교 시간으로 새지 않게 한다
fn is_constant_time_equal(expected: &[u8], received: &[u8]) -> bool {
    if expected.len() != received.len() {
        return false;
    }
    let difference = expected
        .iter()
        .zip(received)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        });
    difference == 0
}

impl<'database> TaskCliTokenService<'database> {
    pub fn new(database: &'database Database) -> Self {
        Self {
            database,
            secret: Mutex::new(None),
        }
    }

    fn load_task_cli_secret(&self) -> Result<String, TaskCliTokenError> {
        let mut cached_secret = self
            .secret
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(secret) = cached_secret.as_ref() {
            return Ok(secret.clone());
        }
        let connection = self.database.connection();
        let stored_secret = find_app_setting(&connection, TASK_CLI_SECRET_SETTING_KEY)?
            .map(|setting| setting.value)
            .filter(|value| !value.is_empty());
        let secret = match stored_secret {
            Some(secret) => secret,
            None => {
                let created_secret = create_secret()?;
                save_app_setting(&connection, TASK_CLI_SECRET_SETTING_KEY, &created_secret)?;
                created_secret
            }
        };
        *cached_secret = Some(secret.clone());
        Ok(secret)
    }

    /// task 하나에만 통하는 CLI 토큰. worktree의 `.kanvibe/targets.json`을 읽을 수 있는 사용자가
    /// 그 task가 속한 프로젝트를 다룰 수 있게 하는 근거다.
    pub fn build_task_cli_token(&self, task_id: &str) -> Result<String, TaskCliTokenError> {
        let secret = self.load_task_cli_secret()?;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("HMAC은 모든 키 길이를 받는다");
        mac.update(task_id.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }

    pub fn is_valid_task_cli_token(
        &self,
        task_id: &str,
        submitted_token: &str,
    ) -> Result<bool, TaskCliTokenError> {
        let expected_token = self.build_task_cli_token(task_id)?;
        Ok(is_constant_time_equal(
            expected_token.as_bytes(),
            submitted_token.as_bytes(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_db::DatabaseLocation;

    fn open_database(directory: &tempfile::TempDir) -> Database {
        Database::open(&DatabaseLocation::in_app_data_directory(
            directory.path(),
            None,
        ))
        .unwrap()
    }

    // parity: TS-main-taskCliTokenService-1d56d1
    #[test]
    fn same_task_gets_the_same_token_and_other_tasks_differ() {
        let directory = tempfile::tempdir().unwrap();
        let database = open_database(&directory);
        let service = TaskCliTokenService::new(&database);

        let first_token = service.build_task_cli_token("task-1").unwrap();

        assert_eq!(first_token.len(), 64);
        assert!(
            first_token
                .chars()
                .all(|character| matches!(character, '0'..='9' | 'a'..='f'))
        );
        assert_eq!(service.build_task_cli_token("task-1").unwrap(), first_token);
        assert_ne!(service.build_task_cli_token("task-2").unwrap(), first_token);
    }

    // parity: TS-main-taskCliTokenService-a2d1a0
    #[test]
    fn token_is_valid_only_for_its_task() {
        let directory = tempfile::tempdir().unwrap();
        let database = open_database(&directory);
        let service = TaskCliTokenService::new(&database);
        let token = service.build_task_cli_token("task-1").unwrap();

        assert!(service.is_valid_task_cli_token("task-1", &token).unwrap());
        assert!(!service.is_valid_task_cli_token("task-2", &token).unwrap());
        assert!(!service.is_valid_task_cli_token("task-1", "forged").unwrap());
    }

    // parity: TS-main-taskCliTokenService-bdf1b2
    #[test]
    fn concurrent_first_issue_stores_one_secret_that_survives_a_restart() {
        let directory = tempfile::tempdir().unwrap();
        let database = open_database(&directory);
        let service = TaskCliTokenService::new(&database);

        let tokens: Vec<String> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|_| scope.spawn(|| service.build_task_cli_token("task-1").unwrap()))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        let restarted_service = TaskCliTokenService::new(&database);

        assert_eq!(tokens[0], tokens[1]);
        let stored_rows: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM app_settings WHERE key = ?1",
                [TASK_CLI_SECRET_SETTING_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_rows, 1);
        assert!(
            restarted_service
                .is_valid_task_cli_token("task-1", &tokens[0])
                .unwrap()
        );
    }

    #[test]
    fn token_matches_node_hmac_for_a_known_secret() {
        let directory = tempfile::tempdir().unwrap();
        let database = open_database(&directory);
        save_app_setting(
            &database.connection(),
            TASK_CLI_SECRET_SETTING_KEY,
            "secret",
        )
        .unwrap();
        let service = TaskCliTokenService::new(&database);

        // 표준 HMAC-SHA256("secret", "task-1")이다. Node `createHmac`도 같은 값을 낸다
        assert_eq!(
            service.build_task_cli_token("task-1").unwrap(),
            "ee8408f8be0df1feacb4264ad6166be0be15c6ecca8eb34a7228d6abf04874df"
        );
    }
}
