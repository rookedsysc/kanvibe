use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_services::board_notifier::{
    BoardNotifier, HookStatusTargetMissingPayload, HookStatusTargetMissingReason,
    TaskHookInstallFailedPayload, TaskStatusChangedPayload,
};
use serde_json::{Value, json};

use super::*;
use crate::clock::system_clock;
use crate::error::MobileResult;
use crate::pairing::PushPlatform;
use crate::test_support::{
    FakeRelay, RelayResponder, TestDatabase, lock, open_test_database, start_fake_relay, wait_until,
};

/// 정해 둔 구독을 돌려주고, 문이 있으면 열릴 때까지 읽기를 멈춘다
#[derive(Default)]
struct FakeRegistry {
    subscriptions: Vec<PushSubscriptionTarget>,
    read_gate: Option<Mutex<Receiver<()>>>,
    removed: Mutex<Vec<(String, String)>>,
}

impl PushSubscriptionRegistry for FakeRegistry {
    fn read_subscriptions(&self) -> MobileResult<Vec<PushSubscriptionTarget>> {
        if let Some(gate) = &self.read_gate {
            let _ = lock(gate).recv();
        }
        Ok(self.subscriptions.clone())
    }

    fn remove_unregistered(&self, device_id: &str, push_token: &str) -> MobileResult<()> {
        lock(&self.removed).push((device_id.to_string(), push_token.to_string()));
        Ok(())
    }
}

fn subscriptions() -> Vec<PushSubscriptionTarget> {
    vec![
        PushSubscriptionTarget {
            device_id: "device-1".to_string(),
            push_token: "push-1".to_string(),
            platform: PushPlatform::Ios,
            connection_id: "connection-1".to_string(),
        },
        PushSubscriptionTarget {
            device_id: "device-2".to_string(),
            push_token: "push-2".to_string(),
            platform: PushPlatform::Android,
            connection_id: "connection-2".to_string(),
        },
    ]
}

struct NotificationFixture {
    notifications: MobileNotifications,
    board: Arc<BoardNotifier>,
    events: Arc<Mutex<Vec<Value>>>,
    _events_handle: ListenerHandle,
    test_database: TestDatabase,
}

fn ok_relay() -> FakeRelay {
    start_fake_relay(Arc::new(|_| 200))
}

fn fixture(
    registry: Arc<dyn PushSubscriptionRegistry>,
    relay: Option<&FakeRelay>,
) -> NotificationFixture {
    let test_database = open_test_database();
    let board = BoardNotifier::new();
    let notifications = MobileNotifications::new(MobileNotificationsParts {
        database: Arc::clone(&test_database.database),
        board: Arc::clone(&board),
        registry,
        relay: relay.map(|relay| PushRelayConfig {
            relay_url: relay.url.clone(),
            relay_token: "relay-secret".to_string(),
        }),
        clock: system_clock(),
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&events);
    let events_handle = notifications.subscribe_to_mobile_events(Arc::new(move |event| {
        lock(&recorded).push(serde_json::to_value(event).unwrap());
    }));
    let settings = open_app_settings(&test_database.database);
    settings
        .set_notification_statuses(
            &["todo", "progress", "pending", "review", "done"].map(String::from),
        )
        .unwrap();
    notifications.start();
    NotificationFixture {
        notifications,
        board,
        events,
        _events_handle: events_handle,
        test_database,
    }
}

impl NotificationFixture {
    fn events(&self) -> Vec<Value> {
        lock(&self.events).clone()
    }

    fn wait_for_events(&self, count: usize) {
        wait_until("모바일 이벤트", || lock(&self.events).len() >= count);
    }

    fn set_notification_settings(&self, is_enabled: bool, statuses: &[&str]) {
        let settings = open_app_settings(&self.test_database.database);
        settings.set_notification_enabled(is_enabled).unwrap();
        let statuses: Vec<String> = statuses.iter().map(|status| status.to_string()).collect();
        settings.set_notification_statuses(&statuses).unwrap();
    }
}

fn status_change(new_status: &str) -> TaskStatusChangedPayload {
    TaskStatusChangedPayload {
        project_name: "kanvibe".to_string(),
        branch_name: "feat/mobile".to_string(),
        task_title: "모바일 구현".to_string(),
        description: None,
        new_status: new_status.to_string(),
        task_id: "task-1".to_string(),
    }
}

fn target_missing() -> HookStatusTargetMissingPayload {
    HookStatusTargetMissingPayload {
        task_id: "missing".to_string(),
        requested_status: "review".to_string(),
        reason: HookStatusTargetMissingReason::TaskNotFound,
    }
}

fn without_should_notify(event: &Value) -> Value {
    let mut fields = event.clone();
    fields.as_object_mut().unwrap().remove("shouldNotify");
    fields
}

fn with_connection_id(event: &Value, connection_id: &str) -> Value {
    let mut fields = without_should_notify(event);
    fields["connectionId"] = json!(connection_id);
    fields
}

/// 잠시 기다려도 더 오지 않는지 본다
fn settle() {
    std::thread::sleep(Duration::from_millis(200));
}

#[test]
fn long_unicode_text_fits_the_fcm_limit_and_matches_the_socket_event() {
    let relay = ok_relay();
    let registry = FakeRegistry {
        subscriptions: vec![PushSubscriptionTarget {
            connection_id: "🚀".repeat(32),
            ..subscriptions()[0].clone()
        }],
        ..FakeRegistry::default()
    };
    let fixture = fixture(Arc::new(registry), Some(&relay));

    fixture
        .board
        .broadcast_task_status_changed(TaskStatusChangedPayload {
            project_name: "🚀".repeat(300),
            task_title: "한글".repeat(1000),
            description: Some("상세 설명".repeat(2000)),
            ..status_change("review")
        });
    wait_until("중계 요청", || relay.received().len() == 1);

    let mut payload = relay.received()[0].body.clone();
    payload.as_object_mut().unwrap().remove("token");
    assert!(payload.to_string().len() <= 4096);
    assert_eq!(
        payload["data"],
        with_connection_id(&fixture.events()[0], &"🚀".repeat(32))
    );
    assert!(
        payload["notification"]["body"]
            .as_str()
            .unwrap()
            .contains("한글")
    );
}

#[test]
fn one_hook_event_shares_its_id_between_socket_and_every_push() {
    let relay = ok_relay();
    let registry = FakeRegistry {
        subscriptions: subscriptions(),
        ..FakeRegistry::default()
    };
    let fixture = fixture(Arc::new(registry), Some(&relay));

    fixture.board.broadcast_board_update();
    fixture
        .board
        .broadcast_task_status_changed(status_change("review"));
    fixture.wait_for_events(2);
    wait_until("두 기기 푸시", || relay.received().len() == 2);

    let events = fixture.events();
    assert_eq!(events[0], json!({ "type": "board-updated" }));
    let status_event = &events[1];
    assert_eq!(status_event["type"], "task-status-changed");
    assert_eq!(status_event["title"], "kanvibe — feat/mobile");
    assert_eq!(status_event["body"], "모바일 구현: review로 변경");
    assert_eq!(status_event["status"], "review");
    assert_eq!(status_event["shouldNotify"], true);
    let mut payloads: Vec<Value> = relay
        .received()
        .into_iter()
        .map(|request| request.body)
        .collect();
    payloads.sort_by_key(|payload| payload["token"].as_str().unwrap().to_string());
    let expected: Vec<Value> = subscriptions()
        .iter()
        .map(|subscription| json!({
            "token": subscription.push_token,
            "notification": { "title": "kanvibe — feat/mobile", "body": "모바일 구현: review로 변경" },
            "data": with_connection_id(status_event, &subscription.connection_id),
        }))
        .collect();
    assert_eq!(payloads, expected);
    let request = &relay.received()[0];
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(request.header("authorization"), Some("Bearer relay-secret"));
    assert!(
        payloads[0]["data"]
            .as_object()
            .unwrap()
            .values()
            .all(Value::is_string)
    );
}

#[test]
fn every_hook_status_reaches_the_socket_without_relay_settings() {
    let relay = ok_relay();
    let registry = FakeRegistry {
        subscriptions: subscriptions(),
        ..FakeRegistry::default()
    };
    let fixture = fixture(Arc::new(registry), None);

    fixture
        .board
        .broadcast_task_status_changed(status_change("progress"));
    fixture
        .board
        .broadcast_task_status_changed(status_change("done"));
    fixture.wait_for_events(2);
    settle();

    let statuses: Vec<Value> = fixture
        .events()
        .iter()
        .map(|event| event["status"].clone())
        .collect();
    assert_eq!(statuses, vec![json!("progress"), json!("done")]);
    assert!(relay.received().is_empty());
}

#[test]
fn stalled_relay_does_not_block_events_and_stop_discards_in_flight_results() {
    let (release_relay, relay_gate) = channel::<()>();
    let relay_gate = Mutex::new(relay_gate);
    let responder: RelayResponder = Arc::new(move |_| {
        let _ = lock(&relay_gate).recv();
        410
    });
    let relay = start_fake_relay(responder);
    let registry = Arc::new(FakeRegistry {
        subscriptions: subscriptions()[..1].to_vec(),
        ..FakeRegistry::default()
    });
    let fixture = fixture(registry.clone(), Some(&relay));

    fixture
        .board
        .broadcast_task_status_changed(status_change("review"));
    fixture
        .board
        .broadcast_task_status_changed(status_change("done"));
    fixture.wait_for_events(2);
    wait_until("멈춘 중계 요청 두 개", || {
        relay.received().len() == 2
    });
    fixture.notifications.stop();
    release_relay.send(()).unwrap();
    release_relay.send(()).unwrap();
    fixture
        .board
        .broadcast_task_status_changed(status_change("review"));
    settle();

    assert_eq!(fixture.events().len(), 2);
    assert!(lock(&registry.removed).is_empty());
}

#[test]
fn failed_relay_response_does_not_stop_other_device_pushes() {
    let responder: RelayResponder = Arc::new(|request| {
        if request.body["token"] == "push-1" {
            503
        } else {
            200
        }
    });
    let relay = start_fake_relay(responder);
    let registry = FakeRegistry {
        subscriptions: subscriptions(),
        ..FakeRegistry::default()
    };
    let fixture = fixture(Arc::new(registry), Some(&relay));

    fixture
        .board
        .broadcast_task_status_changed(status_change("review"));

    wait_until("두 기기 푸시", || relay.received().len() == 2);
}

#[test]
fn stopping_while_reading_subscriptions_sends_no_push() {
    let relay = ok_relay();
    let (open_gate, read_gate) = channel::<()>();
    let registry = FakeRegistry {
        subscriptions: subscriptions(),
        read_gate: Some(Mutex::new(read_gate)),
        ..FakeRegistry::default()
    };
    let fixture = fixture(Arc::new(registry), Some(&relay));

    fixture
        .board
        .broadcast_task_status_changed(status_change("review"));
    fixture.wait_for_events(1);
    fixture.notifications.stop();
    open_gate.send(()).unwrap();
    settle();

    assert!(relay.received().is_empty());
}

#[test]
fn only_selected_statuses_are_pushed_while_every_status_is_synced() {
    for is_enabled in [false, true] {
        let relay = ok_relay();
        let registry = FakeRegistry {
            subscriptions: subscriptions()[..1].to_vec(),
            ..FakeRegistry::default()
        };
        let fixture = fixture(Arc::new(registry), Some(&relay));
        fixture.set_notification_settings(is_enabled, &["review"]);

        fixture
            .board
            .broadcast_task_status_changed(status_change("review"));
        fixture
            .board
            .broadcast_task_status_changed(status_change("done"));
        fixture.wait_for_events(2);
        settle();

        let events = fixture.events();
        assert_eq!(
            (&events[0]["status"], &events[0]["shouldNotify"]),
            (&json!("review"), &json!(is_enabled))
        );
        assert_eq!(
            (&events[1]["status"], &events[1]["shouldNotify"]),
            (&json!("done"), &json!(false))
        );
        assert_eq!(relay.received().len(), usize::from(is_enabled));
    }
}

#[test]
fn hook_install_failure_is_a_foreground_warning_without_the_secret_error() {
    let relay = ok_relay();
    let registry = FakeRegistry {
        subscriptions: subscriptions(),
        ..FakeRegistry::default()
    };
    let fixture = fixture(Arc::new(registry), Some(&relay));

    fixture
        .board
        .broadcast_task_hook_install_failed(TaskHookInstallFailedPayload {
            task_id: "task-1".to_string(),
            task_title: "fixture".to_string(),
            error: "private shell command".to_string(),
        });
    fixture.wait_for_events(1);
    settle();

    let warning = &fixture.events()[0];
    assert_eq!(warning["type"], "operational-warning");
    assert_eq!(warning["kind"], "task-hook-install-failed");
    assert_eq!(warning["taskId"], "task-1");
    assert_eq!(warning["shouldNotify"], false);
    assert!(
        !Value::Array(fixture.events())
            .to_string()
            .contains("private shell command")
    );
    assert!(relay.received().is_empty());
}

#[test]
fn missing_target_follows_status_settings_and_shares_the_event_id() {
    let relay = ok_relay();
    let registry = FakeRegistry {
        subscriptions: subscriptions()[..1].to_vec(),
        ..FakeRegistry::default()
    };
    let fixture = fixture(Arc::new(registry), Some(&relay));

    fixture
        .board
        .broadcast_hook_status_target_missing(target_missing());
    wait_until("중계 요청", || relay.received().len() == 1);
    let data = &relay.received()[0].body["data"];
    assert_eq!(data["type"], "operational-warning");
    assert_eq!(data["kind"], "hook-status-target-missing");
    assert_eq!(data["eventId"], fixture.events()[0]["eventId"]);
    fixture.set_notification_settings(false, &["review"]);
    fixture
        .board
        .broadcast_hook_status_target_missing(target_missing());
    fixture.wait_for_events(2);
    settle();

    assert_eq!(fixture.events()[1]["shouldNotify"], false);
    assert_eq!(relay.received().len(), 1);
}

/// `services/mobile-push-relay/server.mjs`의 요청 검증 가운데 키 모양과 운영 경고 종류만 옮긴 판정
fn is_accepted_by_relay_contract(payload: &Value) -> bool {
    let has_keys = |value: &Value, keys: &[&str]| {
        value.as_object().is_some_and(|fields| {
            fields.len() == keys.len() && keys.iter().all(|key| fields.contains_key(*key))
        })
    };
    let data_keys = [
        "type",
        "eventId",
        "connectionId",
        "taskId",
        "title",
        "body",
        "status",
        "createdAt",
        "kind",
    ];
    has_keys(payload, &["token", "notification", "data"])
        && has_keys(&payload["notification"], &["title", "body"])
        && has_keys(&payload["data"], &data_keys)
        && payload["data"]["kind"] == "hook-status-target-missing"
}

#[test]
fn missing_target_warning_meets_the_relay_http_contract() {
    let accepted = Arc::new(Mutex::new(Vec::<Value>::new()));
    let recorded = Arc::clone(&accepted);
    let responder: RelayResponder = Arc::new(move |request| {
        if !is_accepted_by_relay_contract(&request.body) {
            return 400;
        }
        lock(&recorded).push(request.body.clone());
        200
    });
    let relay = start_fake_relay(responder);
    let registry = FakeRegistry {
        subscriptions: subscriptions()[..1].to_vec(),
        ..FakeRegistry::default()
    };
    let fixture = fixture(Arc::new(registry), Some(&relay));

    fixture
        .board
        .broadcast_hook_status_target_missing(target_missing());
    wait_until("중계 수락", || lock(&accepted).len() == 1);
    let message = lock(&accepted)[0].clone();
    assert_eq!(message["data"]["taskId"], "missing");
    let mut forged = message.clone();
    forged["data"]["kind"] = json!("arbitrary-command");
    let response = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build(),
    )
    .post(&relay.url)
    .header("content-type", "application/json")
    .send(forged.to_string().as_bytes())
    .unwrap();

    assert_eq!(response.status().as_u16(), 400);
    assert_eq!(lock(&accepted).len(), 1);
    drop(fixture);
}
