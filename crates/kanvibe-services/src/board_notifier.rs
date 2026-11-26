//! 프로세스 안 보드 이벤트 버스(TS `lib/boardNotifier.ts`).
//! 서비스가 보드 변경·상태 변경·실패를 알리면 구독한 화면, 알림 서비스, 모바일 이벤트 소켓이 같은 이벤트를 받는다.
//! 이벤트는 TS와 같은 `type` 태그와 camelCase 필드로 직렬화되어 모바일 `/api/mobile/events`에 그대로 나갈 수 있다.

use std::sync::{Arc, Mutex};

use kanvibe_domain::background_sync::BackgroundTaskSyncStatus;
use kanvibe_domain::notifications::{
    BackgroundSyncReviewPayload, HookTunnelFailedPayload, HookTunnelRecoveredPayload,
    TaskPrMergedDetectedPayload,
};
use kanvibe_process::task_state_service::BoardUpdateNotifier;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusChangedPayload {
    pub project_name: String,
    pub branch_name: String,
    pub task_title: String,
    pub description: Option<String>,
    pub new_status: String,
    pub task_id: String,
}

/// hook이 가리킨 task를 찾지 못했다. 이유는 TS처럼 `task-not-found` 하나뿐이다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatusTargetMissingPayload {
    pub task_id: String,
    pub requested_status: String,
    pub reason: HookStatusTargetMissingReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookStatusTargetMissingReason {
    TaskNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSessionChangedPayload {
    pub task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskHookInstallFailedPayload {
    pub task_id: String,
    pub task_title: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrMachineRegistrationFailedPayload {
    pub ssh_host: String,
    pub herdr_session: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrServerHandoffRequiredPayload {
    pub ssh_host: String,
    pub herdr_session: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPeerDeletionFailedPayload {
    pub task_id: String,
    pub task_title: String,
    pub failed_peer_urls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPrMergedDetectedBatchPayload {
    pub merged_pull_requests: Vec<TaskPrMergedDetectedPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundSyncStatusChangedPayload {
    pub status: BackgroundTaskSyncStatus,
}

/// TS `BoardEventPayload` 합집합
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum BoardEvent {
    BoardUpdated,
    BackgroundSyncStatusChanged(BackgroundSyncStatusChangedPayload),
    TaskStatusChanged(TaskStatusChangedPayload),
    HookStatusTargetMissing(HookStatusTargetMissingPayload),
    TaskHookInstallFailed(TaskHookInstallFailedPayload),
    HookTunnelFailed(HookTunnelFailedPayload),
    HookTunnelRecovered(HookTunnelRecoveredPayload),
    TaskSessionChanged(TaskSessionChangedPayload),
    TaskPeerDeletionFailed(TaskPeerDeletionFailedPayload),
    HerdrMachineRegistrationFailed(HerdrMachineRegistrationFailedPayload),
    HerdrServerHandoffRequired(HerdrServerHandoffRequiredPayload),
    TaskPrMergedDetected(TaskPrMergedDetectedPayload),
    TaskPrMergedDetectedBatch(TaskPrMergedDetectedBatchPayload),
    BackgroundSyncReviewNeeded(BackgroundSyncReviewPayload),
}

pub type BoardEventListener = dyn Fn(&BoardEvent) + Send + Sync;

struct Subscriber {
    id: u64,
    listener: Arc<BoardEventListener>,
}

#[derive(Default)]
struct SubscriberList {
    next_id: u64,
    subscribers: Vec<Subscriber>,
}

/// 구독자 목록. 앱에서 하나를 만들어 모든 서비스가 `Arc`로 나눠 쓴다
#[derive(Default)]
pub struct BoardNotifier {
    subscribers: Mutex<SubscriberList>,
}

/// 구독 해제 핸들. `unsubscribe`를 부르거나 버리면 더는 이벤트를 받지 않는다
pub struct BoardEventSubscription {
    notifier: Arc<BoardNotifier>,
    subscriber_id: u64,
}

impl BoardEventSubscription {
    pub fn unsubscribe(self) {}
}

impl Drop for BoardEventSubscription {
    fn drop(&mut self) {
        let mut list = self.notifier.lock_subscribers();
        list.subscribers
            .retain(|subscriber| subscriber.id != self.subscriber_id);
    }
}

impl BoardNotifier {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock_subscribers(&self) -> std::sync::MutexGuard<'_, SubscriberList> {
        self.subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn subscribe(
        self: &Arc<Self>,
        listener: impl Fn(&BoardEvent) + Send + Sync + 'static,
    ) -> BoardEventSubscription {
        let mut list = self.lock_subscribers();
        let subscriber_id = list.next_id;
        list.next_id += 1;
        list.subscribers.push(Subscriber {
            id: subscriber_id,
            listener: Arc::new(listener),
        });
        BoardEventSubscription {
            notifier: Arc::clone(self),
            subscriber_id,
        }
    }

    /// 구독자 목록을 복사한 뒤 잠금 밖에서 부른다. 구독자가 이벤트 안에서 다시 구독·해제해도 막히지 않는다
    pub fn emit(&self, event: BoardEvent) {
        let listeners: Vec<Arc<BoardEventListener>> = self
            .lock_subscribers()
            .subscribers
            .iter()
            .map(|subscriber| Arc::clone(&subscriber.listener))
            .collect();
        for listener in listeners {
            listener(&event);
        }
    }

    pub fn broadcast_board_update(&self) {
        self.emit(BoardEvent::BoardUpdated);
    }

    pub fn broadcast_background_sync_status(&self, status: BackgroundTaskSyncStatus) {
        self.emit(BoardEvent::BackgroundSyncStatusChanged(
            BackgroundSyncStatusChangedPayload { status },
        ));
    }

    pub fn broadcast_task_status_changed(&self, payload: TaskStatusChangedPayload) {
        self.emit(BoardEvent::TaskStatusChanged(payload));
    }

    pub fn broadcast_task_session_changed(&self, task_id: &str) {
        self.emit(BoardEvent::TaskSessionChanged(TaskSessionChangedPayload {
            task_id: task_id.to_string(),
        }));
    }

    pub fn broadcast_hook_status_target_missing(&self, payload: HookStatusTargetMissingPayload) {
        self.emit(BoardEvent::HookStatusTargetMissing(payload));
    }

    pub fn broadcast_task_hook_install_failed(&self, payload: TaskHookInstallFailedPayload) {
        self.emit(BoardEvent::TaskHookInstallFailed(payload));
    }

    pub fn broadcast_hook_tunnel_failed(&self, payload: HookTunnelFailedPayload) {
        self.emit(BoardEvent::HookTunnelFailed(payload));
    }

    pub fn broadcast_hook_tunnel_recovered(&self, payload: HookTunnelRecoveredPayload) {
        self.emit(BoardEvent::HookTunnelRecovered(payload));
    }

    pub fn broadcast_herdr_machine_registration_failed(
        &self,
        payload: HerdrMachineRegistrationFailedPayload,
    ) {
        self.emit(BoardEvent::HerdrMachineRegistrationFailed(payload));
    }

    pub fn broadcast_herdr_server_handoff_required(
        &self,
        payload: HerdrServerHandoffRequiredPayload,
    ) {
        self.emit(BoardEvent::HerdrServerHandoffRequired(payload));
    }

    pub fn broadcast_task_peer_deletion_failed(&self, payload: TaskPeerDeletionFailedPayload) {
        self.emit(BoardEvent::TaskPeerDeletionFailed(payload));
    }

    pub fn broadcast_task_pr_merged_detected(&self, payload: TaskPrMergedDetectedPayload) {
        self.emit(BoardEvent::TaskPrMergedDetected(payload));
    }

    pub fn broadcast_task_pr_merged_detected_batch(
        &self,
        merged_pull_requests: Vec<TaskPrMergedDetectedPayload>,
    ) {
        self.emit(BoardEvent::TaskPrMergedDetectedBatch(
            TaskPrMergedDetectedBatchPayload {
                merged_pull_requests,
            },
        ));
    }

    pub fn broadcast_background_sync_review_needed(&self, payload: BackgroundSyncReviewPayload) {
        self.emit(BoardEvent::BackgroundSyncReviewNeeded(payload));
    }
}

/// M2 `TaskStatusSync`·`TaskStateService`가 받는 보드 갱신 알림 자리
impl BoardUpdateNotifier for BoardNotifier {
    fn broadcast_board_update(&self) {
        BoardNotifier::broadcast_board_update(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::RecordedBoardEvents;

    fn event_json(event: &BoardEvent) -> serde_json::Value {
        serde_json::to_value(event).unwrap()
    }

    // parity: TS-lib-boardNotifier-382c3b
    #[test]
    fn delivers_board_updated_to_subscribers() {
        let notifier = BoardNotifier::new();
        let events = RecordedBoardEvents::subscribe(&notifier);

        notifier.broadcast_board_update();

        let delivered: Vec<_> = events.events().iter().map(event_json).collect();
        assert_eq!(delivered, [serde_json::json!({ "type": "board-updated" })]);
    }

    fn status_changed_payload(description: Option<&str>) -> TaskStatusChangedPayload {
        TaskStatusChangedPayload {
            project_name: "kanvibe".into(),
            branch_name: "feat/test".into(),
            task_title: "테스트 작업".into(),
            description: description.map(str::to_string),
            new_status: "review".into(),
            task_id: "task-123".into(),
        }
    }

    // parity: TS-lib-boardNotifier-c541e3
    #[test]
    fn delivers_task_status_changed_with_its_payload() {
        let notifier = BoardNotifier::new();
        let events = RecordedBoardEvents::subscribe(&notifier);

        notifier.broadcast_task_status_changed(status_changed_payload(Some("설명")));

        assert_eq!(
            event_json(&events.events()[0]),
            serde_json::json!({
                "type": "task-status-changed",
                "projectName": "kanvibe",
                "branchName": "feat/test",
                "taskTitle": "테스트 작업",
                "description": "설명",
                "newStatus": "review",
                "taskId": "task-123",
            })
        );
    }

    // parity: TS-lib-boardNotifier-5b444a
    #[test]
    fn keeps_a_missing_description_as_null() {
        let notifier = BoardNotifier::new();
        let events = RecordedBoardEvents::subscribe(&notifier);

        notifier.broadcast_task_status_changed(status_changed_payload(None));

        let delivered = event_json(&events.events()[0]);
        assert_eq!(delivered["description"], serde_json::Value::Null);
        assert_eq!(delivered["type"], "task-status-changed");
    }

    // parity: TS-lib-boardNotifier-2f42ec
    #[test]
    fn delivers_hook_status_target_missing_with_its_payload() {
        let notifier = BoardNotifier::new();
        let events = RecordedBoardEvents::subscribe(&notifier);

        notifier.broadcast_hook_status_target_missing(HookStatusTargetMissingPayload {
            task_id: "task-404".into(),
            requested_status: "review".into(),
            reason: HookStatusTargetMissingReason::TaskNotFound,
        });

        assert_eq!(
            event_json(&events.events()[0]),
            serde_json::json!({
                "type": "hook-status-target-missing",
                "taskId": "task-404",
                "requestedStatus": "review",
                "reason": "task-not-found",
            })
        );
    }

    // parity: TS-lib-boardNotifier-f274a4
    #[test]
    fn stops_notifying_after_unsubscribe() {
        let notifier = BoardNotifier::new();
        let events = RecordedBoardEvents::subscribe(&notifier);

        events.unsubscribe();
        notifier.broadcast_board_update();

        assert!(events.events().is_empty());
    }
}
