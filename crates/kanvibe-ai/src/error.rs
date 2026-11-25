use std::io::ErrorKind;

#[derive(Debug, Clone, thiserror::Error)]
pub enum AiError {
    /// 로컬·원격 명령이 실패했다. 연결 자체가 끊긴 실패인지는 명령을 돌린 쪽이 판정한다(TS `isSSHConnectionError`)
    #[error("{message}")]
    Command {
        message: String,
        is_connection_failure: bool,
    },

    /// 로컬 파일을 읽지 못했다. `kind`는 Node 오류 코드(ENOENT 등)에 해당한다
    #[error("{message}")]
    Io { kind: ErrorKind, message: String },

    /// 사용자에게 그대로 보여 줄 오류 문장
    #[error("{0}")]
    Message(String),
}

impl AiError {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }

    pub fn from_io(error: &std::io::Error, path: &str) -> Self {
        Self::Io {
            kind: error.kind(),
            message: format!("{error}: {path}"),
        }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            Self::Io {
                kind: ErrorKind::NotFound,
                ..
            }
        )
    }

    pub fn is_connection_failure(&self) -> bool {
        matches!(
            self,
            Self::Command {
                is_connection_failure: true,
                ..
            }
        )
    }
}

pub type AiResult<T> = Result<T, AiError>;
