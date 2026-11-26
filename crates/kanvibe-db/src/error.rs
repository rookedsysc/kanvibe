use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    #[error("DB 파일을 준비하지 못했습니다 ({path}): {source}")]
    File {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// 재생성한 projects 테이블이 받아줄 수 없는 컬럼이 있어 데이터를 잃기 전에 멈췄다
    #[error("projects 테이블 재생성이 처리할 수 없는 컬럼이 있습니다: {}", .0.join(", "))]
    UnsupportedProjectColumns(Vec<String>),

    /// 저장된 값이 알려진 어휘가 아니다
    #[error("{column} 컬럼에 알 수 없는 값이 있습니다: {value}")]
    UnknownStoredValue { column: &'static str, value: String },

    #[error("{column} 컬럼의 JSON을 읽지 못했습니다: {source}")]
    StoredJson {
        column: &'static str,
        #[source]
        source: serde_json::Error,
    },
}

pub type DatabaseResult<T> = Result<T, DatabaseError>;
