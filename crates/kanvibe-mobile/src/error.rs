//! 모바일 경로가 오류 이름으로 응답을 고르던 TS 예외들. 변형 하나가 TS 오류 이름 하나에 대응하고,
//! 메시지·상태 코드·사유는 TS가 던지던 값 그대로 싣는다. 서버는 이 값만 보고 응답 바이트를 정한다.

use kanvibe_services::ServiceError;
use kanvibe_services::diff::TaskWorkspaceFileFailure;
use kanvibe_services::kanban::TaskDeletionError;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MobileError {
    /// TS `MobileTaskInputError`. 기기가 고칠 수 있는 입력 오류라 라우트가 400으로 돌려준다
    #[error("{0}")]
    TaskInput(String),
    /// TS `MobileFeatureError`. 상태 코드는 400·404·409 가운데 하나다
    #[error("{message}")]
    Feature { status: u16, message: String },
    /// TS `MobileSessionDependencyMissingError`. 기기에서는 설치할 수 없어 라우트가 409로 돌려준다
    #[error("{0}")]
    SessionDependencyMissing(String),
    /// TS `TaskWorkspaceFileError`. 라우트는 사유별 상태와 문구를 고른다
    #[error("{0}")]
    TaskWorkspaceFile(TaskWorkspaceFileFailure),
    /// TS `MobileAiRequestError`와 로그인 브리지 오류. 상태가 400·403·404·409·429일 때만 문구를 그대로 내보낸다
    #[error("{message}")]
    AiRequest { status: u16, message: String },
    /// TS `DirtyWorktreeError`. 이벤트 소켓의 태스크 삭제 명령이 `dirty-worktree`로 옮긴다
    #[error("{0}")]
    DirtyWorktree(String),
    /// TS `InvalidWorktreeError`. 이벤트 소켓의 태스크 삭제 명령이 `invalid-worktree`와 이 문구로 옮긴다
    #[error("{0}")]
    InvalidWorktree(String),
    /// 이름 없는 예외. 원문에는 명령줄과 경로가 섞일 수 있어 라우트는 "서버 오류"로 감춘다
    #[error("{0}")]
    Internal(String),
}

pub type MobileResult<T> = Result<T, MobileError>;

impl MobileError {
    /// TS 오류 객체의 `name`. 이름 없는 예외는 `Error`다
    pub fn ts_name(&self) -> &'static str {
        match self {
            Self::TaskInput(_) => "MobileTaskInputError",
            Self::Feature { .. } => "MobileFeatureError",
            Self::SessionDependencyMissing(_) => "MobileSessionDependencyMissingError",
            Self::TaskWorkspaceFile(_) => "TaskWorkspaceFileError",
            Self::AiRequest { .. } => "MobileAiRequestError",
            Self::DirtyWorktree(_) => "DirtyWorktreeError",
            Self::InvalidWorktree(_) => "InvalidWorktreeError",
            Self::Internal(_) => "Error",
        }
    }

    /// TS 오류가 직접 실은 `statusCode`. 없는 오류는 `None`이다
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Feature { status, .. } | Self::AiRequest { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub(crate) fn task_input(message: impl Into<String>) -> Self {
        Self::TaskInput(message.into())
    }

    pub(crate) fn feature(status: u16, message: impl Into<String>) -> Self {
        Self::Feature {
            status,
            message: message.into(),
        }
    }

    pub(crate) fn ai_request(status: u16, message: impl Into<String>) -> Self {
        Self::AiRequest {
            status,
            message: message.into(),
        }
    }

    pub(crate) fn internal(error: impl std::fmt::Display) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<ServiceError> for MobileError {
    fn from(error: ServiceError) -> Self {
        Self::Internal(error.message)
    }
}

impl From<kanvibe_db::DatabaseError> for MobileError {
    fn from(error: kanvibe_db::DatabaseError) -> Self {
        Self::internal(error)
    }
}

impl From<TaskDeletionError> for MobileError {
    fn from(error: TaskDeletionError) -> Self {
        match error {
            TaskDeletionError::DirtyWorktree { .. } => Self::DirtyWorktree(error.to_string()),
            TaskDeletionError::InvalidWorktree => Self::InvalidWorktree(error.to_string()),
            TaskDeletionError::Failed(failure) => failure.into(),
        }
    }
}
