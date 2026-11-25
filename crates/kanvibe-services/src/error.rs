//! 서비스 공통 오류. 화면은 `message`를 그대로 보여 주므로 TS가 던지던 한국어 문구를 바이트 단위로 같게 둔다.

use kanvibe_db::DatabaseError;
use kanvibe_process::exec::ExecGitError;
use kanvibe_process::host_file_access::HostFileError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ServiceError {
    pub message: String,
}

impl ServiceError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<DatabaseError> for ServiceError {
    fn from(error: DatabaseError) -> Self {
        Self::new(error.to_string())
    }
}

impl From<ExecGitError> for ServiceError {
    fn from(error: ExecGitError) -> Self {
        Self::new(error.message)
    }
}

impl From<HostFileError> for ServiceError {
    fn from(error: HostFileError) -> Self {
        Self::new(error.to_string())
    }
}

pub type ServiceResult<T> = Result<T, ServiceError>;
