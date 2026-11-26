use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

/// 설정 한 칸을 흉내 낸다. `should_fail_next_write`면 다음 쓰기 한 번만 실패한다
#[derive(Default)]
struct MemoryStorage {
    value: Mutex<Option<String>>,
    write_count: AtomicUsize,
    should_fail_next_write: AtomicBool,
}

impl MemoryStorage {
    fn with(notifications: &[AppNotification]) -> Arc<Self> {
        let storage = Self::default();
        *storage.value.lock().unwrap() = Some(serde_json::to_string(notifications).unwrap());
        Arc::new(storage)
    }

    fn stored(&self) -> Vec<AppNotification> {
        parse_notifications(self.value.lock().unwrap().as_deref())
    }
}

impl NotificationListStorage for MemoryStorage {
    fn read(&self) -> ServiceResult<Option<String>> {
        Ok(self.value.lock().unwrap().clone())
    }

    fn write(&self, serialized: &str) -> ServiceResult<()> {
        if self.should_fail_next_write.swap(false, Ordering::SeqCst) {
            return Err(ServiceError::new("settings unavailable"));
        }
        self.write_count.fetch_add(1, Ordering::SeqCst);
        *self.value.lock().unwrap() = Some(serialized.to_string());
        Ok(())
    }
}

fn notification(id: &str, task_id: Option<&str>, is_read: bool) -> AppNotification {
    AppNotification {
        id: id.into(),
        title: "title".into(),
        body: "body".into(),
        task_id: task_id.map(str::to_string),
        relative_path: "/ko/task/task-1".into(),
        locale: "ko".into(),
        is_read,
        created_at: "2026-08-08T00:00:00.000Z".into(),
        dedupe_key: "k1".into(),
        action: Some(None),
    }
}

fn base_notification() -> AppNotification {
    notification("n1", Some("task-1"), false)
}

/// 2026-11-01T00:00:00.000Z
const FIXED_NOW_MS: i64 = 1_793_491_200_000;

fn store_with(storage: Arc<MemoryStorage>) -> NotificationStore {
    NotificationStore::with_clock(storage, || FIXED_NOW_MS)
}

fn payload(
    title: &str,
    task_id: Option<&str>,
    dedupe_key: Option<&str>,
) -> DesktopNotificationPayload {
    DesktopNotificationPayload {
        title: title.into(),
        body: "새 이벤트".into(),
        task_id: task_id.map(str::to_string),
        locale: "ko".into(),
        relative_path: None,
        dedupe_key: dedupe_key.map(str::to_string),
        action: None,
    }
}

fn read_states(notifications: &[AppNotification]) -> Vec<(String, bool)> {
    notifications
        .iter()
        .map(|entry| (entry.id.clone(), entry.is_read))
        .collect()
}

#[test]
fn marks_only_unread_notifications_of_the_same_task() {
    let storage = MemoryStorage::with(&[
        notification("n1", Some("task-1"), false),
        notification("n2", Some("task-1"), false),
        notification("n3", Some("task-2"), false),
    ]);
    let store = store_with(Arc::clone(&storage));

    let updated_count = store.mark_task_notifications_read("task-1").unwrap();

    assert_eq!(updated_count, 2);
    assert_eq!(
        read_states(&storage.stored()),
        [
            ("n1".into(), true),
            ("n2".into(), true),
            ("n3".into(), false)
        ]
    );
}

#[test]
fn does_not_rewrite_when_the_task_is_already_read() {
    let storage = MemoryStorage::with(&[
        notification("n1", Some("task-1"), true),
        notification("n2", Some("task-2"), false),
    ]);
    let store = store_with(Arc::clone(&storage));

    let updated_count = store.mark_task_notifications_read("task-1").unwrap();

    assert_eq!(updated_count, 0);
    assert_eq!(storage.write_count.load(Ordering::SeqCst), 0);
}

#[test]
fn skips_notifications_without_a_task() {
    let storage = MemoryStorage::with(&[notification("n1", None, false)]);
    let store = store_with(Arc::clone(&storage));

    let updated_count = store.mark_task_notifications_read("task-1").unwrap();

    assert_eq!(updated_count, 0);
    assert_eq!(storage.write_count.load(Ordering::SeqCst), 0);
}

type MarkRead = fn(&NotificationStore);

#[test]
fn creating_and_marking_read_at_the_same_time_keeps_both_changes() {
    let cases: [(&str, MarkRead, bool); 3] = [
        (
            "하나 읽음",
            |store| {
                store.mark_notification_read("n1").unwrap();
            },
            false,
        ),
        (
            "태스크 읽음",
            |store| {
                store.mark_task_notifications_read("task-1").unwrap();
            },
            false,
        ),
        (
            "모두 읽음",
            |store| store.mark_all_notifications_read().unwrap(),
            true,
        ),
    ];
    for (label, mark_read, is_new_read) in cases {
        let store = store_with(MemoryStorage::with(&[base_notification()]));

        let created = std::thread::scope(|scope| {
            let creation =
                scope.spawn(|| store.create_notification(&payload(label, Some("task-2"), None)));
            scope.spawn(|| mark_read(&store));
            creation.join().unwrap().unwrap()
        });

        let mut expected_new = created.notification.clone();
        let mut expected_old = base_notification();
        expected_old.is_read = true;
        let listed = store.list_notifications().unwrap();
        // 두 스레드 중 먼저 잠금을 잡은 쪽이 달라도 두 변경은 모두 남는다. 모두 읽음이 생성보다 늦게 잡혔을 때만 새 알림도 읽음이다
        if !is_new_read {
            assert!(!listed[0].is_read, "{label}");
        }
        expected_new.is_read = listed[0].is_read;
        assert_eq!(listed, [expected_new, expected_old], "{label}");
    }
}

#[test]
fn concurrent_duplicates_are_stored_once() {
    let store = store_with(MemoryStorage::with(&[base_notification()]));
    let duplicate_payload = DesktopNotificationPayload {
        body: "같은 이벤트".into(),
        ..payload("동시 중복 알림", None, Some("concurrent-event"))
    };

    let results: Vec<NotificationCreation> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| scope.spawn(|| store.create_notification(&duplicate_payload).unwrap()))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });

    let created: Vec<_> = results.iter().filter(|result| result.is_created).collect();
    let duplicate: Vec<_> = results.iter().filter(|result| !result.is_created).collect();
    assert_eq!(created.len(), 1);
    assert_eq!(duplicate[0].notification, created[0].notification);
    assert_eq!(
        store.list_notifications().unwrap(),
        [created[0].notification.clone(), base_notification()]
    );
}

#[test]
fn keeps_the_newest_hundred_and_lists_newest_first() {
    let base_ms = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .timestamp_millis();
    let retained: Vec<AppNotification> = (0..100)
        .map(|index| AppNotification {
            created_at: iso_timestamp(base_ms - index),
            ..notification(&format!("retained-{index}"), Some("task-1"), false)
        })
        .collect();
    let store = store_with(MemoryStorage::with(&retained));

    let created = store
        .create_notification(&payload("보관 상한", None, None))
        .unwrap();
    let saved = store.list_notifications().unwrap();

    assert_eq!(saved.len(), 100);
    assert_eq!(saved[0], created.notification);
    assert_eq!(saved.last().unwrap().id, "retained-98");
    assert!(!saved.iter().any(|entry| entry.id == "retained-99"));
}

#[test]
fn notifies_only_successful_changes() {
    let storage = MemoryStorage::with(&[base_notification()]);
    let store = store_with(Arc::clone(&storage));
    let changes = Arc::new(Mutex::new(Vec::new()));
    let observed_storage = Arc::clone(&storage);
    let sink = Arc::clone(&changes);
    let subscription = store.subscribe_to_notification_changes(move || {
        sink.lock()
            .unwrap()
            .push(read_states(&observed_storage.stored()));
    });
    let event = DesktopNotificationPayload {
        body: "본문".into(),
        ..payload("변경 이벤트", None, Some("change-event"))
    };

    let created = store.create_notification(&event).unwrap().notification;
    store.create_notification(&event).unwrap();
    assert_eq!(store.mark_notification_read("missing").unwrap(), None);
    store.mark_notification_read(&created.id).unwrap();
    store.mark_notification_read(&created.id).unwrap();
    store.mark_all_notifications_read().unwrap();
    store.mark_all_notifications_read().unwrap();

    let new_id = created.id.clone();
    assert_eq!(
        *changes.lock().unwrap(),
        [
            vec![(new_id.clone(), false), ("n1".into(), false)],
            vec![(new_id.clone(), true), ("n1".into(), false)],
            vec![(new_id, true), ("n1".into(), true)],
        ]
    );
    drop(subscription);
    store
        .create_notification(&payload("구독 해제 후", None, None))
        .unwrap();
    assert_eq!(changes.lock().unwrap().len(), 3);
}

#[test]
fn task_read_changes_notify_after_saving() {
    let storage = MemoryStorage::with(&[base_notification()]);
    let store = store_with(Arc::clone(&storage));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_storage = Arc::clone(&storage);
    let sink = Arc::clone(&observed);
    let _subscription = store.subscribe_to_notification_changes(move || {
        sink.lock().unwrap().push(observed_storage.stored());
    });

    store.mark_task_notifications_read("task-1").unwrap();
    store.mark_task_notifications_read("task-1").unwrap();

    let mut read = base_notification();
    read.is_read = true;
    assert_eq!(*observed.lock().unwrap(), [vec![read]]);
}

#[test]
fn a_failed_save_does_not_notify_and_later_writes_continue() {
    let storage = MemoryStorage::with(&[base_notification()]);
    let store = store_with(Arc::clone(&storage));
    let change_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&change_count);
    let _subscription = store.subscribe_to_notification_changes(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    storage.should_fail_next_write.store(true, Ordering::SeqCst);

    let failed = store.mark_notification_read("n1").unwrap_err();
    assert_eq!(failed.message, "settings unavailable");
    assert_eq!(change_count.load(Ordering::SeqCst), 0);
    assert_eq!(store.list_notifications().unwrap(), [base_notification()]);

    store.mark_notification_read("n1").unwrap();

    let mut read = base_notification();
    read.is_read = true;
    assert_eq!(store.list_notifications().unwrap(), [read]);
    assert_eq!(change_count.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failing_listener_does_not_block_saving_or_other_listeners() {
    let store = store_with(MemoryStorage::with(&[base_notification()]));
    let _failing = store.subscribe_to_notification_changes(|| panic!("renderer gone"));
    let change_count = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&change_count);
    let _subscription = store.subscribe_to_notification_changes(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    let updated = store.mark_notification_read("n1").unwrap();

    let mut read = base_notification();
    read.is_read = true;
    assert_eq!(updated, Some(read.clone()));
    assert_eq!(change_count.load(Ordering::SeqCst), 1);
    assert_eq!(store.list_notifications().unwrap(), [read]);
}

#[test]
fn created_notifications_use_millisecond_utc_timestamps() {
    let store = store_with(MemoryStorage::with(&[]));

    let created = store
        .create_notification(&payload("시각", Some("task-9"), None))
        .unwrap()
        .notification;

    assert_eq!(created.created_at, "2026-11-01T00:00:00.000Z");
    assert_eq!(created.relative_path, "/ko/task/task-9");
    assert_eq!(created.dedupe_key, "시각::새 이벤트::task-9::ko");
}
