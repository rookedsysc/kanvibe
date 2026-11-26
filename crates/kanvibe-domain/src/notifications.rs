//! 앱 알림. 알림 목록은 `app_settings.app_notifications`에 JSON 배열로 저장되고 모바일에도 같은 필드 이름으로 나간다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPrMergedDetectedPayload {
    pub task_id: String,
    pub task_title: String,
    pub branch_name: String,
    pub pr_url: String,
    pub merged_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSyncRegisteredWorktreePayload {
    pub task_id: String,
    pub project_name: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub ssh_host: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PulledTaskStatus {
    Updated,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSyncPulledTaskPayload {
    pub task_id: String,
    pub task_title: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub ssh_host: Option<String>,
    pub status: PulledTaskStatus,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackgroundSyncOperation {
    WorktreeSync,
    PullRequestSync,
    TaskPullSync,
}

impl BackgroundSyncOperation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorktreeSync => "worktree-sync",
            Self::PullRequestSync => "pull-request-sync",
            Self::TaskPullSync => "task-pull-sync",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSyncFailurePayload {
    pub operation: BackgroundSyncOperation,
    pub target: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::json_presence"
    )]
    pub ssh_host: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSyncReviewPayload {
    pub merged_pull_requests: Vec<TaskPrMergedDetectedPayload>,
    pub registered_worktrees: Vec<BackgroundSyncRegisteredWorktreePayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pulled_tasks: Option<Vec<BackgroundSyncPulledTaskPayload>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failures: Option<Vec<BackgroundSyncFailurePayload>>,
}

/// 알림을 눌렀을 때 화면이 이어서 할 일
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AppNotificationAction {
    #[serde(rename = "background-sync-review")]
    BackgroundSyncReview {
        payload: BackgroundSyncReviewPayload,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppNotification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub task_id: Option<String>,
    pub relative_path: String,
    pub locale: String,
    pub is_read: bool,
    pub created_at: String,
    pub dedupe_key: String,
    /// 옛 알림에는 필드가 없고 새 알림은 `null`을 쓴다. 다시 저장할 때 원래 모양을 지킨다
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::json_presence"
    )]
    pub action: Option<Option<AppNotificationAction>>,
}

/// hook 터널을 잇지 못한 이유의 갈래(TS `HookTunnelFailedPayload.kind`). 갈래마다 확인할 곳이 다르다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookTunnelFailureKind {
    /// 원격 호스트에 SSH로 닿지 못했다
    Ssh,
    /// 원격 daemon과 핸드셰이크하지 못했다
    Handshake,
    /// 같은 client ID의 기존 데스크톱 연결이 ping에 답해 daemon이 새 연결을 거절했다
    ClientInUse,
    /// 원격 실행 환경이나 터널을 준비하지 못했다
    Setup,
}

impl HookTunnelFailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
            Self::Handshake => "handshake",
            Self::ClientInUse => "client-in-use",
            Self::Setup => "setup",
        }
    }
}

/// hook 터널을 잇지 못했다. 같은 이유가 이어지는 재시도에서는 한 번만 보낸다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookTunnelFailedPayload {
    pub ssh_host: String,
    pub kind: HookTunnelFailureKind,
    pub reason: String,
}

/// daemon이 응답 없는 기존 연결을 교체해 hook 터널이 다시 이어졌다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookTunnelRecoveredPayload {
    pub ssh_host: String,
}

/// 데스크톱 알림을 띄우고 알림 목록에 남길 내용
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopNotificationPayload {
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub locale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<AppNotificationAction>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_notifications_keep_absent_and_null_action_apart() {
        let stored = r#"[{"id":"a","title":"t","body":"b","taskId":null,"relativePath":"/ko","locale":"ko","isRead":false,"createdAt":"2026-11-25T00:02:39.084Z","dedupeKey":"k","action":null},{"id":"b","title":"t","body":"b","taskId":"task-1","relativePath":"/ko/task/task-1","locale":"ko","isRead":true,"createdAt":"2026-11-25T00:02:39.084Z","dedupeKey":"k"}]"#;

        let notifications: Vec<AppNotification> = serde_json::from_str(stored).unwrap();

        assert_eq!(notifications[0].action, Some(None));
        assert_eq!(notifications[1].action, None);
        assert_eq!(serde_json::to_string(&notifications).unwrap(), stored);
    }
}
