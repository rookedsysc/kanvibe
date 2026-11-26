//! 모바일 클라이언트가 보드를 읽고 태스크를 만들거나 옮기는 경로(TS `mobileBridgeService`의 pane 미러 밖 부분).
//!
//! 태스크 변경은 데스크탑 화면과 같은 보드 서비스를 거치므로 데스크탑과 같은 결과가 남는다. 페어링한 기기가
//! 이 머신에 worktree와 세션을 만들 수 있다는 뜻이라, 서버는 이 메서드들을 기기 인증 뒤에만 부른다.
//! 페어링과 기기 인증은 [`crate::devices::MobileDevices`]가 맡는다.

mod ai_history;
mod board;
mod connection;
mod directories;
mod inbox;
mod tasks;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kanvibe_db::Database;
use kanvibe_services::board_notifier::BoardNotifier;
use kanvibe_services::notification_store::NotificationStore;

use crate::ports::{
    DiffFiles, PeerTaskRegistration, ProjectCatalog, SessionDependencyCheck, TaskBoard,
};

pub use self::ai_history::MobileAiHistoryOptions;
pub use self::board::MobileBoard;
pub use self::connection::MobileConnectionSettings;
pub(crate) use self::directories::normalize_posix_path;
pub use self::inbox::{MobileNotification, MobileNotificationSettings};
pub use self::tasks::{MobileTaskDeletion, MobileTaskDraft};

pub struct MobileBridgeParts {
    pub database: Arc<Database>,
    pub board: Arc<BoardNotifier>,
    pub tasks: Arc<dyn TaskBoard>,
    pub projects: Arc<dyn ProjectCatalog>,
    pub diff: Arc<dyn DiffFiles>,
    pub session_dependencies: Arc<dyn SessionDependencyCheck>,
    pub peers: Arc<dyn PeerTaskRegistration>,
    pub notifications: Arc<NotificationStore>,
    /// 기기가 보낸 `~`를 펼칠 이 PC의 홈 디렉터리
    pub home_directory: PathBuf,
    /// 설정 화면에 보여 줄 모바일 TLS 포트(hook 포트 + 10)
    pub mobile_tls_port: u16,
}

pub struct MobileBridge {
    database: Arc<Database>,
    board: Arc<BoardNotifier>,
    tasks: Arc<dyn TaskBoard>,
    projects: Arc<dyn ProjectCatalog>,
    diff: Arc<dyn DiffFiles>,
    session_dependencies: Arc<dyn SessionDependencyCheck>,
    peers: Arc<dyn PeerTaskRegistration>,
    notifications: Arc<NotificationStore>,
    home_directory: PathBuf,
    mobile_tls_port: u16,
    /// 목록에서 발견한 AI 기록 원본 경로. 상세·첨부 조회가 다시 목록을 훑지 않게 하는 탐색 힌트다
    ai_sources: Mutex<ai_history::AiSourceCache>,
}

impl MobileBridge {
    pub fn new(parts: MobileBridgeParts) -> Self {
        Self {
            database: parts.database,
            board: parts.board,
            tasks: parts.tasks,
            projects: parts.projects,
            diff: parts.diff,
            session_dependencies: parts.session_dependencies,
            peers: parts.peers,
            notifications: parts.notifications,
            home_directory: parts.home_directory,
            mobile_tls_port: parts.mobile_tls_port,
            ai_sources: Mutex::new(ai_history::AiSourceCache::default()),
        }
    }
}

#[cfg(test)]
mod tests;
