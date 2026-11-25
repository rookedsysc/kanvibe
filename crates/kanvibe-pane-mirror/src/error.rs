//! 미러 동작의 실패. 서버 라우팅은 갈래를 보고 HTTP 상태와 명령 결과 코드를 정한다.
//!
//! TS는 오류의 `name`으로 갈랐다. `SurfaceUnavailableError`는 409와 `reason`, `PaneNotFoundError`는 `pane-not-found`,
//! `LastPaneError`는 `last-tab`, `MobileTaskInputError`는 400이고 나머지는 서버 오류다.

use serde::Serialize;

/// 세션을 비출 수 없는 이유. 화면이 빈 터미널 대신 이 사유를 보여 준다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceUnavailableReason {
    NoSession,
    SessionNotRunning,
    ZellijTooOld,
    HerdrMissing,
    RemoteUnreachable,
}

impl SurfaceUnavailableReason {
    /// 응답 JSON의 `reason`과 WebSocket 닫힘 사유에 그대로 쓰는 값
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoSession => "no-session",
            Self::SessionNotRunning => "session-not-running",
            Self::ZellijTooOld => "zellij-too-old",
            Self::HerdrMissing => "herdr-missing",
            Self::RemoteUnreachable => "remote-unreachable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaneMirrorError {
    /// TS `SurfaceUnavailableError`
    #[error("{}", .0.as_str())]
    SurfaceUnavailable(SurfaceUnavailableReason),
    /// TS `PaneNotFoundError`. 이 태스크에 없거나 이미 닫힌 pane이다
    #[error("이미 닫혔거나 이 태스크에 없는 탭입니다")]
    PaneNotFound,
    /// TS `LastPaneError`. 세션의 마지막 pane을 닫으면 세션이 통째로 사라지므로 막는다
    #[error("마지막 탭은 닫을 수 없습니다")]
    LastPane,
    /// TS `MobileTaskInputError`. 요청이 잘못된 것이라 사용자에게 그대로 보여 준다
    #[error("{0}")]
    MobileTaskInput(String),
    /// 그 밖의 실패. 메시지에 명령줄과 호스트 별칭이 섞일 수 있어 네트워크로 내보내지 않는다
    #[error("{0}")]
    Failed(String),
}

pub type MirrorResult<T> = Result<T, PaneMirrorError>;

pub(crate) fn unavailable(reason: SurfaceUnavailableReason) -> PaneMirrorError {
    PaneMirrorError::SurfaceUnavailable(reason)
}

pub(crate) fn session_not_running() -> PaneMirrorError {
    unavailable(SurfaceUnavailableReason::SessionNotRunning)
}

impl From<String> for PaneMirrorError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}
