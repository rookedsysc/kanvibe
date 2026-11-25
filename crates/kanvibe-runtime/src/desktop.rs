//! 화면과 모바일이 함께 쓰는 나머지 단일 서비스: 보드 이벤트 알림 전달, 릴리스 확인, 다이어그램 렌더러.

use std::sync::Arc;

use kanvibe_services::desktop_notification::DesktopNotificationService;
use kanvibe_services::release_update::{
    GitHubReleaseFeedClient, ReleaseFeedClient, ReleaseUpdateService,
};

use crate::board::BoardParts;
use crate::config::RuntimeConfig;
use crate::diagram::SharedDiagramRenderer;
use crate::foundation::Foundation;
use crate::notifications::NotificationForwarding;
use crate::qa_network::TableReleaseFeedClient;

pub(crate) struct DesktopParts {
    pub notifications: NotificationForwarding,
    pub release_update: Arc<ReleaseUpdateService>,
    pub diagram: Arc<SharedDiagramRenderer>,
}

pub(crate) fn build_desktop(
    config: &RuntimeConfig,
    foundation: &Foundation,
    board: &BoardParts,
) -> DesktopParts {
    let notification_service = Arc::new(DesktopNotificationService::new(
        Arc::clone(&foundation.database),
        Arc::clone(&board.notification_store),
        Arc::clone(&config.notification_presenter),
    ));
    DesktopParts {
        notifications: NotificationForwarding::new(
            notification_service,
            config.notification_callbacks.clone(),
        ),
        release_update: Arc::new(ReleaseUpdateService::new(
            release_feed_client(config),
            &config.app_version,
            false,
        )),
        diagram: Arc::new(SharedDiagramRenderer::new(Arc::clone(
            &config.diagram_font_files,
        ))),
    }
}

/// QA 응답 표가 있으면 릴리스 목록도 표로만 답한다
fn release_feed_client(config: &RuntimeConfig) -> Arc<dyn ReleaseFeedClient> {
    match &config.qa_network_responses {
        Some(table) => Arc::new(TableReleaseFeedClient(Arc::clone(table))),
        None => Arc::new(GitHubReleaseFeedClient),
    }
}
