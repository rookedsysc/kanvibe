//! 백그라운드 sync 진행 상태. 보드 화면과 모바일이 같은 모양으로 받는다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackgroundTaskSyncPhase {
    Idle,
    Worktrees,
    PullRequests,
    Pulls,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTaskSyncStatus {
    pub phase: BackgroundTaskSyncPhase,
    pub is_running: bool,
    pub completed_stages: u32,
    pub review_needed: bool,
    pub board_updated: bool,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_with_the_wire_names() {
        let status = BackgroundTaskSyncStatus {
            phase: BackgroundTaskSyncPhase::PullRequests,
            is_running: true,
            completed_stages: 1,
            review_needed: false,
            board_updated: false,
            error: None,
        };

        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            r#"{"phase":"pull-requests","isRunning":true,"completedStages":1,"reviewNeeded":false,"boardUpdated":false,"error":null}"#
        );
    }
}
