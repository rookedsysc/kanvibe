//! 모바일 기능 경로(`/api/mobile/tasks/search` 등)가 부르는 서비스(TS `mobileFeatureService`와 하위 모듈).
//! 모든 쓰기는 값 검증을 먼저 끝낸 뒤 데스크탑과 같은 서비스로 넘긴다. 검증 실패는 `MobileFeatureError`(400·404·409)다.
//! AI 기능은 [`ai::MobileAi`]가 따로 맡는다.

pub mod ai;
mod board_task;
mod diff;
mod settings;
mod validation;
mod workspace;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use kanvibe_db::Database;
use kanvibe_services::notification_store::NotificationStore;

use crate::ports::{
    BackgroundSyncControl, DiffFiles, ProjectCatalog, RemoteImageUpload, SessionDependencyCheck,
    TaskBoard, TaskPaneAccess, TerminalTabs,
};

pub use self::diff::MobileDiffFileContent;
pub use self::settings::{MobileBackgroundSyncSettings, MobileSettings};

pub struct MobileFeaturesParts {
    pub database: Arc<Database>,
    pub tasks: Arc<dyn TaskBoard>,
    pub projects: Arc<dyn ProjectCatalog>,
    pub diff: Arc<dyn DiffFiles>,
    pub tabs: Arc<dyn TerminalTabs>,
    pub session_dependencies: Arc<dyn SessionDependencyCheck>,
    pub background_sync: Arc<dyn BackgroundSyncControl>,
    pub notifications: Arc<NotificationStore>,
    /// pane 미러 계층의 pane 확인·입력. 원격 이미지 붙여넣기만 쓴다
    pub panes: Arc<dyn TaskPaneAccess>,
    pub images: Arc<dyn RemoteImageUpload>,
}

pub struct MobileFeatures {
    database: Arc<Database>,
    tasks: Arc<dyn TaskBoard>,
    projects: Arc<dyn ProjectCatalog>,
    diff: Arc<dyn DiffFiles>,
    tabs: Arc<dyn TerminalTabs>,
    session_dependencies: Arc<dyn SessionDependencyCheck>,
    background_sync: Arc<dyn BackgroundSyncControl>,
    notifications: Arc<NotificationStore>,
    panes: Arc<dyn TaskPaneAccess>,
    images: Arc<dyn RemoteImageUpload>,
    /// 브랜치를 연결하는 중인 태스크. 같은 태스크에 두 기기가 동시에 연결하면 worktree가 둘 생긴다
    branching_task_ids: Mutex<HashSet<String>>,
}

impl MobileFeatures {
    pub fn new(parts: MobileFeaturesParts) -> Self {
        Self {
            database: parts.database,
            tasks: parts.tasks,
            projects: parts.projects,
            diff: parts.diff,
            tabs: parts.tabs,
            session_dependencies: parts.session_dependencies,
            background_sync: parts.background_sync,
            notifications: parts.notifications,
            panes: parts.panes,
            images: parts.images,
            branching_task_ids: Mutex::new(HashSet::new()),
        }
    }
}

#[cfg(test)]
mod tests;
