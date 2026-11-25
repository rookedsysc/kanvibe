use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

use kanvibe_services::board_notifier::{BoardNotifier, TaskStatusChangedPayload};
use serde_json::json;

use super::*;
use crate::app_settings::open_app_settings;
use crate::clock::system_clock;
use crate::notifications::{
    MobileNotifications, MobileNotificationsParts, PushRelayConfig, PushSubscriptionRegistry,
};
use crate::test_support::{
    RelayResponder, TestDatabase, lock, open_test_database, start_fake_relay, wait_until,
};

struct DeviceFixture {
    devices: Arc<MobileDevices>,
    test_database: TestDatabase,
}

fn fixture() -> DeviceFixture {
    let test_database = open_test_database();
    let devices = Arc::new(MobileDevices::new(MobileDevicesParts {
        database: Arc::clone(&test_database.database),
        clock: system_clock(),
        pairing_failure_delay: Arc::new(|_| {}),
    }));
    DeviceFixture {
        devices,
        test_database,
    }
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn is_authorized(devices: &MobileDevices, header: Option<&str>) -> bool {
    devices.authorize_mobile_request(header).unwrap().is_some()
}

fn pair_one_device(devices: &MobileDevices, device_name: &str) -> String {
    let code = devices.start_mobile_pairing().code;
    devices
        .pair_mobile_device(&code, device_name)
        .unwrap()
        .expect("페어링이 실패했다")
}

/// 다른 스레드가 코드를 바꿔 끼워도 결국 연결되도록 성공할 때까지 코드를 새로 받아 시도한다
fn pair_until_success(devices: &MobileDevices, device_name: &str) -> String {
    loop {
        let code = devices.start_mobile_pairing().code;
        if let Some(token) = devices.pair_mobile_device(&code, device_name).unwrap() {
            return token;
        }
    }
}

fn first_device_id(devices: &MobileDevices) -> String {
    devices.list_paired_mobile_devices().unwrap()[0]
        .device_id
        .clone()
}

fn subscription(push_token: &str) -> serde_json::Value {
    json!({ "pushToken": push_token, "platform": "ios", "connectionId": "connection-1" })
}

fn push_tokens(devices: &MobileDevices) -> Vec<String> {
    devices
        .list_mobile_push_subscriptions()
        .unwrap()
        .into_iter()
        .map(|target| target.push_token)
        .collect()
}

fn with_unpaired_device_ids(devices: &MobileDevices, run: impl FnOnce(&Mutex<Vec<String>>)) {
    let unpaired = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&unpaired);
    let handle = devices.on_mobile_device_unpaired(Arc::new(move |device_id: &str| {
        lock(&recorded).push(device_id.to_string());
    }));
    run(&unpaired);
    handle.unsubscribe();
}

// parity: TS-main-mobileBridgeService-934b4e
#[test]
fn pairing_with_the_screen_code_returns_a_token() {
    let fixture = fixture();
    let code = fixture.devices.start_mobile_pairing().code;

    let token = fixture
        .devices
        .pair_mobile_device(&code, "iPhone")
        .unwrap()
        .unwrap();

    assert_eq!(token.len(), 64);
    assert!(token.chars().all(|character| character.is_ascii_hexdigit()));
}

// parity: TS-main-mobileBridgeService-714fb7
#[test]
fn wrong_code_returns_no_token() {
    let fixture = fixture();
    fixture.devices.start_mobile_pairing();

    let code = fixture.devices.read_mobile_pairing_code().unwrap();
    let wrong = if code == "000000" { "111111" } else { "000000" };

    assert_eq!(
        fixture.devices.pair_mobile_device(wrong, "iPhone").unwrap(),
        None
    );
}

// parity: TS-main-mobileBridgeService-dab508
#[test]
fn pairing_without_an_issued_code_fails() {
    let fixture = fixture();
    let code = fixture
        .devices
        .read_mobile_pairing_code()
        .unwrap_or_else(|| "123456".to_string());

    assert_eq!(
        fixture.devices.pair_mobile_device(&code, "iPhone").unwrap(),
        None
    );
}

// parity: TS-main-mobileBridgeService-0381d6
#[test]
fn paired_device_is_stored_in_the_list() {
    let fixture = fixture();
    pair_one_device(&fixture.devices, "iPad");

    let devices = fixture.devices.list_paired_mobile_devices().unwrap();

    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_name, "iPad");
    assert!(!devices[0].device_id.is_empty());
    assert!(devices[0].paired_at.ends_with('Z'));
}

// parity: TS-main-mobileBridgeService-888eea
#[test]
fn device_list_does_not_carry_tokens() {
    let fixture = fixture();
    pair_one_device(&fixture.devices, "모바일 기기");

    let serialized =
        serde_json::to_string(&fixture.devices.list_paired_mobile_devices().unwrap()).unwrap();

    assert!(!serialized.contains("token"));
}

// parity: TS-main-mobileBridgeService-88640b
#[test]
fn overlapping_pairings_keep_every_token() {
    let fixture = fixture();

    let tokens: Vec<String> = std::thread::scope(|scope| {
        let pairings: Vec<_> = (0..4)
            .map(|index| {
                let devices = &fixture.devices;
                scope.spawn(move || pair_until_success(devices, &format!("기기 {index}")))
            })
            .collect();
        pairings
            .into_iter()
            .map(|pairing| pairing.join().unwrap())
            .collect()
    });

    for token in &tokens {
        assert!(is_authorized(&fixture.devices, Some(&bearer(token))));
    }
    assert_eq!(
        fixture.devices.list_paired_mobile_devices().unwrap().len(),
        4
    );
}

// parity: TS-main-mobileBridgeService-5ed12a
#[test]
fn pairing_during_unpair_survives_and_the_removed_device_stays_removed() {
    let fixture = fixture();
    let removed_token = pair_one_device(&fixture.devices, "iPhone");
    let removed_device_id = first_device_id(&fixture.devices);

    let added_token = std::thread::scope(|scope| {
        let pairing = scope.spawn(|| pair_until_success(&fixture.devices, "iPad"));
        scope.spawn(|| {
            fixture
                .devices
                .unpair_mobile_device(&removed_device_id)
                .unwrap()
        });
        pairing.join().unwrap()
    });

    assert!(is_authorized(&fixture.devices, Some(&bearer(&added_token))));
    assert!(!is_authorized(
        &fixture.devices,
        Some(&bearer(&removed_token))
    ));
}

// parity: TS-main-mobileBridgeService-7c36b0
#[test]
fn unpaired_device_token_no_longer_passes() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");

    fixture
        .devices
        .unpair_mobile_device(&first_device_id(&fixture.devices))
        .unwrap();

    assert!(!is_authorized(&fixture.devices, Some(&bearer(&token))));
}

// parity: TS-main-mobileBridgeService-6e5e10
#[test]
fn subscription_is_stored_rotated_and_never_exposed_with_pairing_secrets() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");
    let device_id = first_device_id(&fixture.devices);
    let header = bearer(&token);

    assert!(
        fixture
            .devices
            .update_mobile_notification_subscription(
                Some(&header),
                &subscription("fcm-token-first")
            )
            .unwrap()
    );
    let targets = fixture.devices.list_mobile_push_subscriptions().unwrap();
    assert_eq!(
        serde_json::to_value(&targets).unwrap(),
        json!([{ "deviceId": device_id, "pushToken": "fcm-token-first", "platform": "ios", "connectionId": "connection-1" }])
    );
    fixture
        .devices
        .update_mobile_notification_subscription(Some(&header), &subscription("fcm-token-rotated"))
        .unwrap();

    assert_eq!(push_tokens(&fixture.devices), vec!["fcm-token-rotated"]);
    let paired =
        serde_json::to_string(&fixture.devices.list_paired_mobile_devices().unwrap()).unwrap();
    assert!(!paired.contains("fcm-token"));
    let pushes =
        serde_json::to_string(&fixture.devices.list_mobile_push_subscriptions().unwrap()).unwrap();
    assert!(!pushes.contains(&token));
}

// parity: TS-main-mobileBridgeService-ce6b29
#[test]
fn deleting_a_subscription_keeps_other_devices_and_the_pairing() {
    let fixture = fixture();
    let first_token = pair_one_device(&fixture.devices, "iPhone");
    let second_token = pair_one_device(&fixture.devices, "iPad");
    let devices = &fixture.devices;
    devices
        .update_mobile_notification_subscription(
            Some(&bearer(&first_token)),
            &subscription("fcm-token-first"),
        )
        .unwrap();
    devices
        .update_mobile_notification_subscription(
            Some(&bearer(&second_token)),
            &subscription("fcm-token-second"),
        )
        .unwrap();

    assert!(
        devices
            .delete_mobile_notification_subscription(Some(&bearer(&first_token)))
            .unwrap()
    );

    assert_eq!(push_tokens(devices), vec!["fcm-token-second"]);
    assert!(is_authorized(devices, Some(&bearer(&first_token))));
}

// parity: TS-main-mobileBridgeService-104d1c
#[test]
fn unpair_racing_a_subscription_never_revives_the_device() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");
    let device_id = first_device_id(&fixture.devices);
    let header = bearer(&token);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            fixture
                .devices
                .update_mobile_notification_subscription(
                    Some(&header),
                    &subscription("fcm-token-first"),
                )
                .unwrap()
        });
        scope.spawn(|| fixture.devices.unpair_mobile_device(&device_id).unwrap());
    });

    assert!(push_tokens(&fixture.devices).is_empty());
    assert!(
        !fixture
            .devices
            .update_mobile_notification_subscription(
                Some(&header),
                &subscription("fcm-token-first")
            )
            .unwrap()
    );
    assert!(
        !fixture
            .devices
            .delete_mobile_notification_subscription(Some(&header))
            .unwrap()
    );
    assert!(!is_authorized(&fixture.devices, Some(&header)));
}

// parity: TS-main-mobileBridgeService-e2cfba
#[test]
fn broken_subscription_is_not_pushed_and_pairing_survives() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");
    let settings = open_app_settings(&fixture.test_database.database);
    let mut saved: serde_json::Value = serde_json::from_str(
        &settings
            .get_app_setting("mobile_paired_devices")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    saved[0]["notification"] = json!({ "pushToken": "fcm-token-first", "platform": "web", "connectionId": "connection-1" });
    settings
        .set_app_setting("mobile_paired_devices", &saved.to_string())
        .unwrap();

    assert!(push_tokens(&fixture.devices).is_empty());
    assert!(is_authorized(&fixture.devices, Some(&bearer(&token))));
}

// parity: TS-main-mobileBridgeService-bec8e0
#[test]
fn expired_push_token_is_removed_only_for_the_same_device_and_token() {
    let fixture = fixture();
    let devices = &fixture.devices;
    let first_token = pair_one_device(devices, "iPhone");
    let device_id = first_device_id(devices);
    let second_token = pair_one_device(devices, "iPad");
    devices
        .update_mobile_notification_subscription(
            Some(&bearer(&first_token)),
            &subscription("fcm-token-first"),
        )
        .unwrap();
    devices
        .update_mobile_notification_subscription(
            Some(&bearer(&second_token)),
            &subscription("other-push-token"),
        )
        .unwrap();

    devices
        .remove_unregistered_mobile_push_subscription(&device_id, "older-push-token")
        .unwrap();
    assert_eq!(push_tokens(devices).len(), 2);
    devices
        .remove_unregistered_mobile_push_subscription(&device_id, "fcm-token-first")
        .unwrap();

    assert_eq!(push_tokens(devices), vec!["other-push-token"]);
    assert!(is_authorized(devices, Some(&bearer(&first_token))));
}

/// 만료 정리가 끝났는지 알려 주는 구독 저장소
struct ObservedRegistry {
    devices: Arc<MobileDevices>,
    is_cleanup_completed: AtomicBool,
}

impl PushSubscriptionRegistry for ObservedRegistry {
    fn read_subscriptions(&self) -> MobileResult<Vec<PushSubscriptionTarget>> {
        self.devices.list_mobile_push_subscriptions()
    }

    fn remove_unregistered(&self, device_id: &str, push_token: &str) -> MobileResult<()> {
        self.devices
            .remove_unregistered_mobile_push_subscription(device_id, push_token)?;
        self.is_cleanup_completed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

// parity: TS-main-mobileBridgeService-bc12a4
#[test]
fn late_410_does_not_remove_a_token_rotated_in_between() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");
    let header = bearer(&token);
    fixture
        .devices
        .update_mobile_notification_subscription(Some(&header), &subscription("fcm-token-first"))
        .unwrap();
    let (release_relay, relay_gate) = channel::<()>();
    let relay_gate = Mutex::new(relay_gate);
    let responder: RelayResponder = Arc::new(move |_| {
        let _ = lock(&relay_gate).recv();
        410
    });
    let relay = start_fake_relay(responder);
    let registry = Arc::new(ObservedRegistry {
        devices: Arc::clone(&fixture.devices),
        is_cleanup_completed: AtomicBool::new(false),
    });
    let board = BoardNotifier::new();
    let notifications = MobileNotifications::new(MobileNotificationsParts {
        database: Arc::clone(&fixture.test_database.database),
        board: Arc::clone(&board),
        registry: registry.clone(),
        relay: Some(PushRelayConfig {
            relay_url: relay.url.clone(),
            relay_token: "relay-secret".to_string(),
        }),
        clock: system_clock(),
    });
    notifications.start();

    board.broadcast_task_status_changed(TaskStatusChangedPayload {
        project_name: "kanvibe".to_string(),
        branch_name: "feat/mobile".to_string(),
        task_title: "모바일 구현".to_string(),
        description: None,
        new_status: "review".to_string(),
        task_id: "task-1".to_string(),
    });
    wait_until("중계 요청 도착", || relay.received().len() == 1);
    fixture
        .devices
        .update_mobile_notification_subscription(Some(&header), &subscription("rotated-push-token"))
        .unwrap();
    release_relay.send(()).unwrap();
    wait_until("만료 정리 완료", || {
        registry.is_cleanup_completed.load(Ordering::SeqCst)
    });

    assert_eq!(push_tokens(&fixture.devices), vec!["rotated-push-token"]);
    assert!(is_authorized(&fixture.devices, Some(&header)));
    notifications.stop();
}

// parity: TS-main-mobileBridgeService-2534f6
#[test]
fn paired_device_token_passes() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");

    assert!(is_authorized(&fixture.devices, Some(&bearer(&token))));
}

// parity: TS-main-mobileBridgeService-802377
#[test]
fn missing_token_is_blocked() {
    let fixture = fixture();
    pair_one_device(&fixture.devices, "iPhone");

    assert!(!is_authorized(&fixture.devices, None));
}

// parity: TS-main-mobileBridgeService-448213
#[test]
fn unknown_token_is_blocked() {
    let fixture = fixture();
    pair_one_device(&fixture.devices, "iPhone");

    assert!(!is_authorized(
        &fixture.devices,
        Some(&bearer(&"f".repeat(64)))
    ));
}

// parity: TS-main-mobileBridgeService-5c2ac3
#[test]
fn nothing_passes_without_paired_devices() {
    let fixture = fixture();

    assert!(!is_authorized(
        &fixture.devices,
        Some(&bearer(&"a".repeat(64)))
    ));
}

// parity: TS-main-mobileBridgeService-910b43
#[test]
fn authorized_request_returns_the_owner_device_id() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");
    let device_id = first_device_id(&fixture.devices);

    assert_eq!(
        fixture
            .devices
            .authorize_mobile_request(Some(&bearer(&token)))
            .unwrap(),
        Some(device_id)
    );
}

// parity: TS-main-mobileBridgeService-a8e63f
#[test]
fn devices_paired_at_the_same_moment_unpair_separately() {
    let fixture = fixture();
    let kept_token = pair_one_device(&fixture.devices, "iPhone");
    pair_one_device(&fixture.devices, "iPad");
    let devices = fixture.devices.list_paired_mobile_devices().unwrap();

    fixture
        .devices
        .unpair_mobile_device(&devices[1].device_id)
        .unwrap();

    assert!(is_authorized(&fixture.devices, Some(&bearer(&kept_token))));
}

// parity: TS-main-mobileBridgeService-f3689e
#[test]
fn unpair_by_token_removes_only_the_owner() {
    let fixture = fixture();
    let removed_token = pair_one_device(&fixture.devices, "iPhone");
    let kept_token = pair_one_device(&fixture.devices, "iPad");

    assert!(
        fixture
            .devices
            .unpair_mobile_device_by_token(Some(&bearer(&removed_token)))
            .unwrap()
    );

    assert!(!is_authorized(
        &fixture.devices,
        Some(&bearer(&removed_token))
    ));
    assert!(is_authorized(&fixture.devices, Some(&bearer(&kept_token))));
    let remaining = fixture.devices.list_paired_mobile_devices().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].device_name, "iPad");
}

// parity: TS-main-mobileBridgeService-f0fc60
#[test]
fn unpair_by_unknown_token_removes_nothing() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");

    assert!(
        !fixture
            .devices
            .unpair_mobile_device_by_token(Some(&bearer(&"f".repeat(64))))
            .unwrap()
    );
    assert!(is_authorized(&fixture.devices, Some(&bearer(&token))));
    assert_eq!(
        fixture.devices.list_paired_mobile_devices().unwrap().len(),
        1
    );
}

// parity: TS-main-mobileBridgeService-bceda0
#[test]
fn unpair_without_header_removes_nothing() {
    let fixture = fixture();
    pair_one_device(&fixture.devices, "iPhone");

    assert!(!fixture.devices.unpair_mobile_device_by_token(None).unwrap());
    assert_eq!(
        fixture.devices.list_paired_mobile_devices().unwrap().len(),
        1
    );
}

// parity: TS-main-mobileBridgeService-a70810
#[test]
fn unpair_by_token_racing_a_pairing_keeps_the_new_device() {
    let fixture = fixture();
    let removed_token = pair_one_device(&fixture.devices, "iPhone");
    let removed_header = bearer(&removed_token);

    let (added_token, was_removed) = std::thread::scope(|scope| {
        let pairing = scope.spawn(|| pair_until_success(&fixture.devices, "iPad"));
        let unpairing = scope.spawn(|| {
            fixture
                .devices
                .unpair_mobile_device_by_token(Some(&removed_header))
                .unwrap()
        });
        (pairing.join().unwrap(), unpairing.join().unwrap())
    });

    assert!(was_removed);
    assert!(is_authorized(&fixture.devices, Some(&bearer(&added_token))));
    assert!(!is_authorized(&fixture.devices, Some(&removed_header)));
    assert_eq!(
        fixture.devices.list_paired_mobile_devices().unwrap().len(),
        1
    );
}

// parity: TS-main-mobileBridgeService-cb9aa3
#[test]
fn settings_unpair_announces_the_device_id() {
    let fixture = fixture();
    pair_one_device(&fixture.devices, "iPhone");
    let device_id = first_device_id(&fixture.devices);

    with_unpaired_device_ids(&fixture.devices, |unpaired| {
        fixture.devices.unpair_mobile_device(&device_id).unwrap();

        assert_eq!(*lock(unpaired), vec![device_id.clone()]);
    });
}

// parity: TS-main-mobileBridgeService-526ada
#[test]
fn self_unpair_announces_the_same_signal() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");
    let device_id = first_device_id(&fixture.devices);

    with_unpaired_device_ids(&fixture.devices, |unpaired| {
        assert!(
            fixture
                .devices
                .unpair_mobile_device_by_token(Some(&bearer(&token)))
                .unwrap()
        );

        assert_eq!(*lock(unpaired), vec![device_id.clone()]);
    });
}

// parity: TS-main-mobileBridgeService-36178f
#[test]
fn nothing_is_announced_when_no_device_was_removed() {
    let fixture = fixture();
    let token = pair_one_device(&fixture.devices, "iPhone");
    let device_id = first_device_id(&fixture.devices);

    with_unpaired_device_ids(&fixture.devices, |unpaired| {
        fixture
            .devices
            .unpair_mobile_device(&format!("{device_id}-없는-기기"))
            .unwrap();
        assert!(
            !fixture
                .devices
                .unpair_mobile_device_by_token(Some(&bearer(&"f".repeat(64))))
                .unwrap()
        );

        assert!(lock(unpaired).is_empty());
        assert!(is_authorized(&fixture.devices, Some(&bearer(&token))));
    });
}

// parity: TS-main-mobileBridgeService-f9edbe
#[test]
fn unsubscribed_listener_receives_nothing() {
    let fixture = fixture();
    pair_one_device(&fixture.devices, "iPhone");
    let device_id = first_device_id(&fixture.devices);
    let unpaired = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded = Arc::clone(&unpaired);

    fixture
        .devices
        .on_mobile_device_unpaired(Arc::new(move |id: &str| {
            lock(&recorded).push(id.to_string())
        }))
        .unsubscribe();
    fixture.devices.unpair_mobile_device(&device_id).unwrap();

    assert!(lock(&unpaired).is_empty());
}
