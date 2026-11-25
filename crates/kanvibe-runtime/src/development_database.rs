//! 소스로 띄운 개발 실행의 DB 고르기(TS `electron/developmentDatabase.js` `prepareDevelopmentDatabase`).
//!
//! 개발 실행은 설치된 앱과 다른 데이터 디렉터리를 쓰고, 기본값(`existing`)이면 그 디렉터리의 DB를 그대로 쓴다.
//! `--data-source application`이면 설치된 앱 DB(기본 `<appData>/KanVibe/kanvibe.db`, `--application-data-dir`로 바꿈)를
//! SQLite 온라인 백업으로 복사한다. 설치된 앱이 열려 있어도 커밋된 WAL 페이지까지 들어간다. 두 실행이 같은 DB 파일을
//! 쓰지 않도록 디렉터리·심볼릭 링크·하드 링크를 막고, 같은 데이터 디렉터리를 쓰는 개발 실행은 하나만 허용한다.
//! 설치된 앱·자동 QA·수동 QA 실행은 아무것도 하지 않는다.

use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

const DATABASE_FILE_NAME: &str = "kanvibe.db";
/// 설치된 앱의 데이터 디렉터리 이름(Electron `productName`)
pub const INSTALLED_APP_DATA_DIRECTORY_NAME: &str = "KanVibe";
/// 같은 데이터 디렉터리를 쓰는 개발 실행을 하나로 묶는 잠금 파일(Electron `requestSingleInstanceLock` 대신)
const INSTANCE_LOCK_FILE_NAME: &str = "kanvibe-development.lock";
/// targets.json 항목과 원격 터널 소켓 이름을 정하는 설치별 ID
const HOOK_CLIENT_ID_KEY: &str = "hook_client_id";

#[derive(Debug, thiserror::Error)]
pub enum DevelopmentDatabaseError {
    #[error("--data-source must be application or existing")]
    UnknownDataSource,
    #[error("Development and application must use separate database directories")]
    SharedDirectory,
    #[error("Development and application must use separate database files without links")]
    LinkedDatabaseFile,
    #[error("Another development app is using this data directory. Close it before restarting.")]
    AnotherInstance,
    #[error("Application database not found: {0}")]
    ApplicationDatabaseNotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// 이 실행이 어떤 실행인지와 명령줄 값. 모두 호출자가 시작할 때 정해 넘긴다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentDatabaseRequest {
    /// 소스로 띄운 개발 실행인지(TS `KANVIBE_RENDERER_URL`, Rust는 디버그 빌드)
    pub is_development: bool,
    pub is_packaged: bool,
    /// 자동 QA·수동 QA 실행인지
    pub is_qa: bool,
    /// `--data-source` 원문. 없거나 비면 `existing`이다
    pub data_source: Option<String>,
    /// `--application-data-dir` 원문
    pub application_data_directory: Option<PathBuf>,
    /// OS 앱 데이터 루트(Electron `appData`)
    pub platform_app_data: PathBuf,
    /// 이 실행이 쓸 데이터 디렉터리(Electron `userData`)
    pub target_directory: PathBuf,
}

/// 어떻게 끝났는지. 잠금은 프로세스가 끝날 때까지 쥐고 있어야 한다
#[derive(Debug)]
pub enum DevelopmentDatabaseOutcome {
    /// 개발 실행이 아니라 아무것도 하지 않았다
    NotDevelopmentRun,
    /// 개발 DB를 그대로 쓴다
    KeptExisting(InstanceLock),
    /// 설치된 앱 DB가 없어 개발 DB(또는 시드)를 그대로 쓴다. TS는 경고를 남겼다
    ApplicationDatabaseMissing {
        lock: InstanceLock,
        source_path: PathBuf,
    },
    Copied {
        lock: InstanceLock,
        source_path: PathBuf,
        target_path: PathBuf,
    },
}

impl DevelopmentDatabaseOutcome {
    /// TS가 남기던 안내 문구. 할 말이 없으면 `None`이다
    pub fn log_message(&self) -> Option<String> {
        match self {
            Self::ApplicationDatabaseMissing { source_path, .. } => Some(format!(
                "[kanvibe] Application database not found at {}; using the existing development database or bundled seed.",
                source_path.display()
            )),
            Self::Copied {
                source_path,
                target_path,
                ..
            } => Some(format!(
                "[kanvibe] Copied application database: {} -> {}",
                source_path.display(),
                target_path.display()
            )),
            Self::NotDevelopmentRun | Self::KeptExisting(_) => None,
        }
    }
}

/// 개발 실행 데이터 디렉터리 잠금. 놓으면 풀린다
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

pub fn prepare_development_database(
    request: &DevelopmentDatabaseRequest,
) -> Result<DevelopmentDatabaseOutcome, DevelopmentDatabaseError> {
    if !request.is_development || request.is_packaged || request.is_qa {
        return Ok(DevelopmentDatabaseOutcome::NotDevelopmentRun);
    }
    let is_application_mode = parse_data_source(request.data_source.as_deref())?;
    let source_directory = std::path::absolute(
        request
            .application_data_directory
            .clone()
            .unwrap_or_else(|| {
                request
                    .platform_app_data
                    .join(INSTALLED_APP_DATA_DIRECTORY_NAME)
            }),
    )?;
    let target_directory = &request.target_directory;
    let source_path = source_directory.join(DATABASE_FILE_NAME);

    assert_separate_database(&source_directory, target_directory)?;
    let lock = acquire_instance_lock(target_directory)?;

    if !is_application_mode {
        return Ok(DevelopmentDatabaseOutcome::KeptExisting(lock));
    }
    if !source_path.exists() {
        if request.application_data_directory.is_some() {
            return Err(DevelopmentDatabaseError::ApplicationDatabaseNotFound(
                source_path.display().to_string(),
            ));
        }
        return Ok(DevelopmentDatabaseOutcome::ApplicationDatabaseMissing { lock, source_path });
    }
    let target_path = target_directory.join(DATABASE_FILE_NAME);
    copy_application_database(&source_path, &target_path)?;
    Ok(DevelopmentDatabaseOutcome::Copied {
        lock,
        source_path,
        target_path,
    })
}

/// `application`이면 `true`, `existing`(기본)이면 `false`
fn parse_data_source(data_source: Option<&str>) -> Result<bool, DevelopmentDatabaseError> {
    match data_source
        .filter(|mode| !mode.is_empty())
        .unwrap_or("existing")
    {
        "application" => Ok(true),
        "existing" => Ok(false),
        _ => Err(DevelopmentDatabaseError::UnknownDataSource),
    }
}

/// TS `assertSeparateDatabase`. 대상 디렉터리를 만든 뒤 같은 디렉터리(링크 포함)와 같은 DB 파일을 막는다
fn assert_separate_database(
    source_directory: &Path,
    target_directory: &Path,
) -> Result<(), DevelopmentDatabaseError> {
    std::fs::create_dir_all(target_directory)?;
    let source_real_directory = if source_directory.exists() {
        std::fs::canonicalize(source_directory)?
    } else {
        source_directory.to_path_buf()
    };
    if source_real_directory == std::fs::canonicalize(target_directory)? {
        return Err(DevelopmentDatabaseError::SharedDirectory);
    }
    let source_path = source_directory.join(DATABASE_FILE_NAME);
    let target_path = target_directory.join(DATABASE_FILE_NAME);
    let Ok(target) = std::fs::symlink_metadata(&target_path) else {
        return Ok(());
    };
    let source = std::fs::metadata(&source_path).ok();
    let is_same_file =
        source.is_some_and(|source| source.dev() == target.dev() && source.ino() == target.ino());
    if target.file_type().is_symlink() || is_same_file {
        return Err(DevelopmentDatabaseError::LinkedDatabaseFile);
    }
    Ok(())
}

fn acquire_instance_lock(
    target_directory: &Path,
) -> Result<InstanceLock, DevelopmentDatabaseError> {
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(target_directory.join(INSTANCE_LOCK_FILE_NAME))?;
    file.try_lock()
        .map_err(|_| DevelopmentDatabaseError::AnotherInstance)?;
    Ok(InstanceLock { _file: file })
}

/// 설치 앱 DB를 온라인 백업으로 복사한다. 복사본의 hook client ID는 개발 실행이 전에 쓰던 값만 남긴다.
/// 설치 앱의 ID를 함께 쓰면 개발 실행이 targets.json의 설치 앱 항목을 덮어써 설치 앱에 알림이 오지 않기 때문이다.
/// 설치 앱 DB를 읽지 못하면 복사 전에 실패해 이전 개발 DB를 그대로 둔다
fn copy_application_database(
    source_path: &Path,
    target_path: &Path,
) -> Result<(), DevelopmentDatabaseError> {
    let source = Connection::open_with_flags(
        source_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let application_hook_client_id = read_hook_client_id(&source)?;
    let development_hook_client_id = read_previous_development_hook_client_id(target_path);
    source.backup(
        rusqlite::MAIN_DB,
        target_path,
        None::<fn(rusqlite::backup::Progress)>,
    )?;
    drop(source);
    let retained_hook_client_id =
        development_hook_client_id.filter(|id| Some(id) != application_hook_client_id.as_ref());
    replace_hook_client_id(target_path, retained_hook_client_id.as_deref())
}

fn read_hook_client_id(database: &Connection) -> rusqlite::Result<Option<String>> {
    database
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?",
            [HOOK_CLIENT_ID_KEY],
            |row| row.get(0),
        )
        .optional()
}

/// 읽을 수 없는 이전 개발 DB는 곧 덮어쓰므로 새 ID를 받게 둔다
fn read_previous_development_hook_client_id(target_path: &Path) -> Option<String> {
    if !target_path.exists() {
        return None;
    }
    let target = Connection::open_with_flags(target_path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    read_hook_client_id(&target).ok().flatten()
}

fn replace_hook_client_id(
    target_path: &Path,
    hook_client_id: Option<&str>,
) -> Result<(), DevelopmentDatabaseError> {
    let target = Connection::open_with_flags(target_path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    target.execute(
        "DELETE FROM app_settings WHERE key = ?",
        [HOOK_CLIENT_ID_KEY],
    )?;
    if let Some(hook_client_id) = hook_client_id {
        target.execute(
            "INSERT INTO app_settings (id, key, value) VALUES (?, ?, ?)",
            params![
                uuid::Uuid::new_v4().to_string(),
                HOOK_CLIENT_ID_KEY,
                hook_client_id
            ],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
