//! DB 파일 위치. 경로는 호출부가 명시적으로 넘기고 프로세스 환경 변수는 읽지 않는다.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{DatabaseError, DatabaseResult};

const DATABASE_FILE_NAME: &str = "kanvibe.db";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseLocation {
    pub database_path: PathBuf,
    /// 패키지에 들어 있는 시드 DB. 첫 실행에서 이 파일을 복사해 시작한다
    pub seed_database_path: Option<PathBuf>,
}

impl DatabaseLocation {
    /// 앱 데이터 디렉터리 아래 `kanvibe.db`를 쓴다
    pub fn in_app_data_directory(
        app_data_directory: &Path,
        seed_database_path: Option<PathBuf>,
    ) -> Self {
        Self {
            database_path: app_data_directory.join(DATABASE_FILE_NAME),
            seed_database_path,
        }
    }

    /// DB 파일이 없을 때만 시드를 복사하거나 빈 파일을 만든다.
    /// 이미 있는 DB는 시드가 바뀌어도 건드리지 않아야 사용자 데이터가 업그레이드 때 사라지지 않는다.
    pub fn ensure_database_file(&self) -> DatabaseResult<&Path> {
        let database_path = self.database_path.as_path();
        if database_path.exists() {
            return Ok(database_path);
        }
        let file_error = |source| DatabaseError::File {
            path: database_path.to_path_buf(),
            source,
        };
        if let Some(parent_directory) = database_path.parent() {
            fs::create_dir_all(parent_directory).map_err(file_error)?;
        }

        match self
            .seed_database_path
            .as_deref()
            .filter(|seed_path| seed_path.exists())
        {
            Some(seed_path) => fs::copy(seed_path, database_path).map(|_| ()),
            None => fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(database_path)
                .map(|_| ()),
        }
        .map_err(file_error)?;
        Ok(database_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LocationFixture {
        directory: tempfile::TempDir,
    }

    impl LocationFixture {
        fn new() -> Self {
            Self {
                directory: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self, relative_path: &str) -> PathBuf {
            self.directory.path().join(relative_path)
        }

        fn qa_location(&self) -> DatabaseLocation {
            DatabaseLocation::in_app_data_directory(
                &self.path("qa"),
                Some(self.path("app.seed.db")),
            )
        }
    }

    // parity: TS-lib-databasePaths-763c95
    #[test]
    fn database_lives_in_the_selected_app_data_directory_only() {
        let fixture = LocationFixture::new();
        let location = fixture.qa_location();

        location.ensure_database_file().unwrap();

        assert_eq!(location.database_path, fixture.path("qa/kanvibe.db"));
        assert!(!fixture.path("production").exists());
    }

    // parity: TS-lib-databasePaths-fb6433
    #[test]
    fn existing_database_survives_a_changed_seed() {
        let fixture = LocationFixture::new();
        let location = fixture.qa_location();
        fs::create_dir_all(fixture.path("qa")).unwrap();
        fs::write(&location.database_path, "existing QA database fixture").unwrap();
        fs::write(fixture.path("app.seed.db"), "new version seed fixture").unwrap();

        assert_eq!(
            location.ensure_database_file().unwrap(),
            location.database_path
        );
        assert_eq!(
            location.ensure_database_file().unwrap(),
            location.database_path
        );
        assert_eq!(
            fs::read_to_string(&location.database_path).unwrap(),
            "existing QA database fixture"
        );
    }

    // parity: TS-lib-databasePaths-b27828
    #[test]
    fn new_database_is_seeded_only_once() {
        let fixture = LocationFixture::new();
        let location = fixture.qa_location();
        fs::write(fixture.path("app.seed.db"), "initial seed fixture").unwrap();

        location.ensure_database_file().unwrap();
        assert_eq!(
            fs::read_to_string(&location.database_path).unwrap(),
            "initial seed fixture"
        );

        fs::write(fixture.path("app.seed.db"), "updated seed fixture").unwrap();
        location.ensure_database_file().unwrap();
        assert_eq!(
            fs::read_to_string(&location.database_path).unwrap(),
            "initial seed fixture"
        );
    }

    #[test]
    fn missing_seed_creates_an_empty_database_file() {
        let fixture = LocationFixture::new();
        let location = fixture.qa_location();

        location.ensure_database_file().unwrap();

        assert_eq!(fs::metadata(&location.database_path).unwrap().len(), 0);
    }
}
