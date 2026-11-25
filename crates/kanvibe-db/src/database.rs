//! 앱이 쓰는 DB 연결. 쓰기 연결 하나를 잠금 뒤에 두고 모든 조회·저장이 그 연결을 거친다.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OpenFlags};

use crate::error::DatabaseResult;
use crate::location::DatabaseLocation;
use crate::table_introspection::has_table;
use crate::{migrations, schema};

pub struct Database {
    connection: Mutex<Connection>,
}

fn file_has_kanban_tasks(database_path: &Path) -> DatabaseResult<bool> {
    let connection = Connection::open_with_flags(database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    has_table(&connection, "kanban_tasks")
}

impl Database {
    /// TS 앱과 같은 순서로 DB를 연다. 파일이 없으면 시드를 복사하고, 이미 작업 테이블이 있으면 스키마를 복구한 뒤
    /// 기록 없는 옛 DB를 baseline 처리하고, 그래도 남은 마이그레이션만 실행한다.
    pub fn open(location: &DatabaseLocation) -> DatabaseResult<Self> {
        let database_path = location.ensure_database_file()?;
        if file_has_kanban_tasks(database_path)? {
            schema::ensure_sqlite_database_ready(database_path)?;
        }

        let connection = Connection::open(database_path)?;
        schema::configure_connection(&connection)?;
        migrations::baseline_existing_database(&connection)?;
        migrations::run_pending_migrations(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    /// 연결을 잠그고 빌려준다. 앞선 사용자가 패닉으로 잠금을 놓쳤어도 SQLite 연결 자체는 계속 쓸 수 있다
    pub fn connection(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
