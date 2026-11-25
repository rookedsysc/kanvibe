//! 보드 이벤트를 모바일 이벤트 소켓과 푸시 중계 서버로 나눠 보낸다(TS `mobileNotificationService`).
//!
//! hook 이벤트 하나는 같은 `eventId`로 WebSocket 구독자와 각 기기 푸시에 나간다. 중계 서버 주소와 토큰은 서버가
//! 시작할 때 설정으로 받고 프로세스 환경 변수는 읽지 않는다. 이벤트 발행은 외부 네트워크를 기다리지 않는다.
//! 이벤트는 전달 하나마다 작업 스레드 하나가 받은 순서대로 처리하고, 푸시 요청은 기기마다 따로 돈다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use kanvibe_db::Database;
use kanvibe_domain::background_sync::BackgroundTaskSyncStatus;
use kanvibe_domain::task_notifications::{TaskStatusNotification, build_task_status_notification};
use kanvibe_services::board_notifier::{
    BoardEvent, BoardEventSubscription, BoardNotifier, HookStatusTargetMissingPayload,
    TaskHookInstallFailedPayload, TaskStatusChangedPayload,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::app_settings::open_app_settings;
use crate::clock::{EpochClock, iso_timestamp};
use crate::devices::{MobileDevices, PushSubscriptionTarget};
use crate::error::MobileResult;
use crate::listeners::{Listener, ListenerHandle, ListenerSet};

const PUSH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_TITLE_ENCODED_BYTES: usize = 128;
const MAX_BODY_ENCODED_BYTES: usize = 700;
const EVENT_FAILURE_LABEL: &str = "Mobile event delivery failed";

/// 푸시 중계 서버. 둘 다 있어야 푸시를 보낸다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushRelayConfig {
    pub relay_url: String,
    pub relay_token: String,
}

/// 푸시 구독 목록과 만료된 구독 정리. 앱에서는 [`MobileDevices`]가 맡는다
pub trait PushSubscriptionRegistry: Send + Sync {
    fn read_subscriptions(&self) -> MobileResult<Vec<PushSubscriptionTarget>>;
    fn remove_unregistered(&self, device_id: &str, push_token: &str) -> MobileResult<()>;
}

impl PushSubscriptionRegistry for MobileDevices {
    fn read_subscriptions(&self) -> MobileResult<Vec<PushSubscriptionTarget>> {
        self.list_mobile_push_subscriptions()
    }

    fn remove_unregistered(&self, device_id: &str, push_token: &str) -> MobileResult<()> {
        self.remove_unregistered_mobile_push_subscription(device_id, push_token)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationalWarningKind {
    HookStatusTargetMissing,
    TaskHookInstallFailed,
}

/// 푸시로도 나갈 수 있는 이벤트의 공통 필드
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusChangedEvent {
    pub event_id: String,
    pub task_id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub created_at: String,
    pub should_notify: bool,
}

/// 운영 경고. 비밀값이 섞일 수 있는 원래 오류 문장은 싣지 않는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationalWarningEvent {
    pub event_id: String,
    pub kind: OperationalWarningKind,
    pub task_id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub created_at: String,
    pub should_notify: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MobileEvent {
    BoardUpdated,
    BackgroundSyncStatusChanged { status: BackgroundTaskSyncStatus },
    OperationalWarning(OperationalWarningEvent),
    TaskStatusChanged(TaskStatusChangedEvent),
}

pub struct MobileNotificationsParts {
    pub database: Arc<Database>,
    pub board: Arc<BoardNotifier>,
    pub registry: Arc<dyn PushSubscriptionRegistry>,
    pub relay: Option<PushRelayConfig>,
    pub clock: Arc<EpochClock>,
}

/// 진행 중인 전달 하나. 멈추면 늦게 끝난 요청의 결과를 버린다
struct NotificationDelivery {
    is_stopped: AtomicBool,
    board_subscription: Mutex<Option<BoardEventSubscription>>,
}

impl NotificationDelivery {
    fn is_stopped(&self) -> bool {
        self.is_stopped.load(Ordering::SeqCst)
    }
}

/// 이벤트 처리에 필요한 것. 작업 스레드와 푸시 스레드가 나눠 든다
struct DeliveryContext {
    database: Arc<Database>,
    registry: Arc<dyn PushSubscriptionRegistry>,
    relay: Option<PushRelayConfig>,
    clock: Arc<EpochClock>,
    listeners: Arc<ListenerSet<MobileEvent>>,
    agent: ureq::Agent,
    delivery: Arc<NotificationDelivery>,
}

pub struct MobileNotifications {
    database: Arc<Database>,
    board: Arc<BoardNotifier>,
    registry: Arc<dyn PushSubscriptionRegistry>,
    relay: Option<PushRelayConfig>,
    clock: Arc<EpochClock>,
    listeners: Arc<ListenerSet<MobileEvent>>,
    current_delivery: Mutex<Option<Arc<NotificationDelivery>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn build_relay_agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(PUSH_TIMEOUT))
            .build(),
    )
}

/// JSON 문자열 안에서 차지하는 바이트(따옴표 제외)
fn encoded_length(value: &str) -> usize {
    serde_json::to_string(value).map_or(0, |encoded| encoded.len() - 2)
}

/// FCM의 notification·data·APNs alert에 문구가 반복되므로 UTF-8과 JSON 이스케이프 크기를 함께 제한한다
pub(crate) fn truncate_notification_text(value: &str, max_encoded_bytes: usize) -> String {
    if encoded_length(value) <= max_encoded_bytes {
        return value.to_string();
    }
    let mut truncated = String::new();
    let mut encoded_bytes = 0;
    for character in value.chars() {
        let character_bytes = encoded_length(character.encode_utf8(&mut [0u8; 4]));
        if encoded_bytes + character_bytes > max_encoded_bytes.saturating_sub(3) {
            break;
        }
        truncated.push(character);
        encoded_bytes += character_bytes;
    }
    truncated.push('…');
    truncated
}

impl MobileNotifications {
    pub fn new(parts: MobileNotificationsParts) -> Self {
        Self {
            database: parts.database,
            board: parts.board,
            registry: parts.registry,
            relay: parts.relay,
            clock: parts.clock,
            listeners: Arc::new(ListenerSet::default()),
            current_delivery: Mutex::new(None),
        }
    }

    /// 소켓을 가진 네트워크 계층만 구독한다. 핸들을 버리면 구독이 풀린다
    pub fn subscribe_to_mobile_events(&self, listener: Listener<MobileEvent>) -> ListenerHandle {
        self.listeners.add(listener)
    }

    /// 보드 이벤트 구독을 시작한다. 이미 돌고 있으면 앞의 전달을 멈추고 새로 시작한다
    pub fn start(&self) {
        self.stop();
        let delivery = Arc::new(NotificationDelivery {
            is_stopped: AtomicBool::new(false),
            board_subscription: Mutex::new(None),
        });
        let context = Arc::new(DeliveryContext {
            database: Arc::clone(&self.database),
            registry: Arc::clone(&self.registry),
            relay: self.relay.clone(),
            clock: Arc::clone(&self.clock),
            listeners: Arc::clone(&self.listeners),
            agent: build_relay_agent(),
            delivery: Arc::clone(&delivery),
        });
        let (sender, receiver) = channel::<BoardEvent>();
        spawn_event_worker(Arc::clone(&context), receiver);
        let subscription = self.board.subscribe(move |event| {
            let _ = sender.send(event.clone());
        });
        *lock(&delivery.board_subscription) = Some(subscription);
        *lock(&self.current_delivery) = Some(delivery);
    }

    /// 서버를 끄면 보드 구독을 풀고, 진행 중인 중계 요청은 끝나더라도 결과(만료 정리·로그)를 버린다
    pub fn stop(&self) {
        let Some(delivery) = lock(&self.current_delivery).take() else {
            return;
        };
        delivery.is_stopped.store(true, Ordering::SeqCst);
        lock(&delivery.board_subscription).take();
    }
}

impl Drop for MobileNotifications {
    fn drop(&mut self) {
        self.stop();
    }
}

fn spawn_event_worker(context: Arc<DeliveryContext>, receiver: Receiver<BoardEvent>) {
    std::thread::spawn(move || {
        for event in receiver {
            if context.delivery.is_stopped() {
                return;
            }
            context.handle_board_event(&event);
        }
    });
}

impl DeliveryContext {
    fn broadcast(&self, event: &MobileEvent) {
        self.listeners.emit(event, EVENT_FAILURE_LABEL);
    }

    fn handle_board_event(self: &Arc<Self>, event: &BoardEvent) {
        match event {
            BoardEvent::BackgroundSyncStatusChanged(payload) => {
                self.broadcast(&MobileEvent::BackgroundSyncStatusChanged {
                    status: payload.status.clone(),
                });
            }
            BoardEvent::BoardUpdated => self.broadcast(&MobileEvent::BoardUpdated),
            BoardEvent::HookStatusTargetMissing(payload) => self.deliver_target_missing(payload),
            BoardEvent::TaskHookInstallFailed(payload) => self.deliver_hook_install_failed(payload),
            BoardEvent::TaskStatusChanged(payload) => self.deliver_task_status(payload),
            _ => {}
        }
    }

    fn now_iso(&self) -> String {
        iso_timestamp((self.clock)())
    }

    /// 알림 설정을 읽지 못하면 푸시는 보내지 않고 소켓 동기화만 유지한다
    fn deliver_task_status(self: &Arc<Self>, payload: &TaskStatusChangedPayload) {
        let notification = build_task_status_notification(&TaskStatusNotification {
            project_name: payload.project_name.clone(),
            branch_name: payload.branch_name.clone(),
            task_title: payload.task_title.clone(),
            description: payload.description.clone(),
            new_status: payload.new_status.clone(),
            task_id: payload.task_id.clone(),
            locale: "ko".to_string(),
        });
        let mut event = TaskStatusChangedEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            task_id: payload.task_id.clone(),
            title: truncate_notification_text(&notification.title, MAX_TITLE_ENCODED_BYTES),
            body: truncate_notification_text(&notification.body, MAX_BODY_ENCODED_BYTES),
            status: payload.new_status.clone(),
            created_at: self.now_iso(),
            should_notify: false,
        };
        match self.should_notify_status(&payload.new_status) {
            Ok(should_notify) => event.should_notify = should_notify,
            Err(()) => {
                self.broadcast(&MobileEvent::TaskStatusChanged(event));
                eprintln!("[kanvibe] Mobile notification settings could not be read");
                return;
            }
        }
        let mobile_event = MobileEvent::TaskStatusChanged(event.clone());
        self.broadcast(&mobile_event);
        if event.should_notify {
            self.spawn_push(mobile_event, event.title, event.body);
        }
    }

    fn should_notify_status(&self, status: &str) -> Result<bool, ()> {
        let settings = open_app_settings(&self.database)
            .get_notification_settings()
            .map_err(|_| ())?;
        Ok(settings.is_enabled && settings.enabled_statuses.iter().any(|s| s == status))
    }

    fn deliver_hook_install_failed(self: &Arc<Self>, payload: &TaskHookInstallFailedPayload) {
        let warning = OperationalWarningEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            kind: OperationalWarningKind::TaskHookInstallFailed,
            task_id: payload.task_id.clone(),
            title: "Hook 설치 실패".to_string(),
            body: "데스크탑에서 태스크의 Hook 설치 상태를 확인해 주세요.".to_string(),
            status: String::new(),
            created_at: self.now_iso(),
            should_notify: false,
        };
        self.broadcast(&MobileEvent::OperationalWarning(warning));
    }

    /// 대상 누락은 요청된 상태의 알림 설정을 따른다. 설정을 읽지 못하면 경고 전달 전체를 실패로 본다
    fn deliver_target_missing(self: &Arc<Self>, payload: &HookStatusTargetMissingPayload) {
        let Ok(should_notify) = self.should_notify_status(&payload.requested_status) else {
            eprintln!("[kanvibe] Mobile operational warning delivery failed");
            return;
        };
        let warning = OperationalWarningEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            kind: OperationalWarningKind::HookStatusTargetMissing,
            task_id: payload.task_id.clone(),
            title: "Hook 대상 태스크 없음".to_string(),
            body: "상태를 변경할 태스크를 찾지 못했습니다. 데스크탑에서 연결을 확인해 주세요."
                .to_string(),
            status: payload.requested_status.clone(),
            created_at: self.now_iso(),
            should_notify,
        };
        let (title, body) = (warning.title.clone(), warning.body.clone());
        let mobile_event = MobileEvent::OperationalWarning(warning);
        self.broadcast(&mobile_event);
        if should_notify {
            self.spawn_push(mobile_event, title, body);
        }
    }

    /// hook 응답과 보드 이벤트 발행은 외부 네트워크가 끝나기를 기다리지 않는다
    fn spawn_push(self: &Arc<Self>, event: MobileEvent, title: String, body: String) {
        let Some(relay) = self.relay.clone() else {
            return;
        };
        let context = Arc::clone(self);
        std::thread::spawn(move || context.deliver_push(&relay, &event, &title, &body));
    }

    fn deliver_push(
        self: &Arc<Self>,
        relay: &PushRelayConfig,
        event: &MobileEvent,
        title: &str,
        body: &str,
    ) {
        let subscriptions = match self.registry.read_subscriptions() {
            Ok(subscriptions) => subscriptions,
            Err(_) => {
                if !self.delivery.is_stopped() {
                    eprintln!("[kanvibe] Mobile push subscriptions could not be read");
                }
                return;
            }
        };
        if self.delivery.is_stopped() {
            return;
        }
        let push_event = push_event_fields(event);
        for subscription in subscriptions {
            let payload = build_push_payload(&push_event, &subscription, title, body);
            let context = Arc::clone(self);
            let relay = relay.clone();
            std::thread::spawn(move || context.send_push(&relay, &subscription, &payload));
        }
    }

    /// 응답 본문은 민감값을 포함할 수 있어 읽거나 로그에 싣지 않는다. 410은 그 기기의 구독이 만료됐다는 뜻이다
    fn send_push(
        &self,
        relay: &PushRelayConfig,
        subscription: &PushSubscriptionTarget,
        payload: &Value,
    ) {
        let response = self
            .agent
            .post(&relay.relay_url)
            .header("content-type", "application/json")
            .header("authorization", &format!("Bearer {}", relay.relay_token))
            .send(payload.to_string().as_bytes());
        let status = match response {
            Ok(response) => response.status().as_u16(),
            Err(_) => {
                if !self.delivery.is_stopped() {
                    eprintln!("[kanvibe] Mobile push delivery failed or timed out");
                }
                return;
            }
        };
        if status == 410 {
            if !self.delivery.is_stopped()
                && self
                    .registry
                    .remove_unregistered(&subscription.device_id, &subscription.push_token)
                    .is_err()
            {
                eprintln!("[kanvibe] Mobile push delivery failed or timed out");
            }
        } else if !(200..300).contains(&status) {
            eprintln!("[kanvibe] Mobile push relay rejected notification: {status}");
        }
    }
}

/// 소켓 이벤트에서 `shouldNotify`만 뺀 필드. 기기마다 `connectionId`를 덧붙여 푸시 data로 쓴다
fn push_event_fields(event: &MobileEvent) -> Map<String, Value> {
    let mut fields = match serde_json::to_value(event) {
        Ok(Value::Object(fields)) => fields,
        _ => Map::new(),
    };
    fields.remove("shouldNotify");
    fields
}

fn build_push_payload(
    push_event: &Map<String, Value>,
    subscription: &PushSubscriptionTarget,
    title: &str,
    body: &str,
) -> Value {
    let mut data = push_event.clone();
    data.insert(
        "connectionId".to_string(),
        Value::String(subscription.connection_id.clone()),
    );
    json!({
        "token": subscription.push_token,
        "notification": { "title": title, "body": body },
        "data": data,
    })
}

#[cfg(test)]
mod tests;
