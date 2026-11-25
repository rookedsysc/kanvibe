use std::sync::Arc;
use std::thread;

use kanvibe_db::repositories::app_settings::save_app_setting;
use kanvibe_db::{Database, DatabaseLocation};

use super::{MAX_TASKS_WITH_MODES, PANE_CHAT_MODES_KEY, PaneChatModeStore, PaneChatModes};

/// 시험마다 새 DB 파일을 열어 앞 시험이 남긴 기록을 보지 않는다
struct ModeStoreFixture {
    _directory: tempfile::TempDir,
    database: Arc<Database>,
    store: PaneChatModeStore,
}

impl ModeStoreFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let location = DatabaseLocation {
            database_path: directory.path().join("kanvibe.db"),
            seed_database_path: None,
        };
        let database = Arc::new(Database::open(&location).unwrap());
        let store = PaneChatModeStore::new(Arc::clone(&database));
        Self {
            _directory: directory,
            database,
            store,
        }
    }

    fn modes(&self, task_id: &str) -> PaneChatModes {
        self.store.get_pane_chat_modes(task_id).unwrap()
    }
}

fn modes(entries: &[(&str, bool)]) -> PaneChatModes {
    entries
        .iter()
        .map(|(pane_id, is_enabled)| (pane_id.to_string(), *is_enabled))
        .collect()
}

// parity: TS-main-paneChatModeStore-651fdd
#[test]
fn reads_saved_chat_pane_by_task() {
    let fixture = ModeStoreFixture::new();

    fixture
        .store
        .set_pane_chat_mode("task-1", "%1", true)
        .unwrap();

    assert_eq!(fixture.modes("task-1"), modes(&[("%1", true)]));
}

// parity: TS-main-paneChatModeStore-55b139
#[test]
fn remembers_false_when_chat_pane_returns_to_terminal() {
    let fixture = ModeStoreFixture::new();

    fixture
        .store
        .set_pane_chat_mode("task-1", "%1", true)
        .unwrap();
    fixture
        .store
        .set_pane_chat_mode("task-1", "%1", false)
        .unwrap();

    assert_eq!(fixture.modes("task-1"), modes(&[("%1", false)]));
}

// parity: TS-main-paneChatModeStore-7667ee
#[test]
fn keeps_records_of_different_tasks_apart() {
    let fixture = ModeStoreFixture::new();

    fixture
        .store
        .set_pane_chat_mode("task-a", "%1", true)
        .unwrap();

    assert_eq!(fixture.modes("task-a"), modes(&[("%1", true)]));
    assert_eq!(fixture.modes("task-b"), PaneChatModes::new());
}

// parity: TS-main-paneChatModeStore-27e9ed
#[test]
fn keeps_both_panes_saved_concurrently() {
    let fixture = ModeStoreFixture::new();

    thread::scope(|scope| {
        scope.spawn(|| {
            fixture
                .store
                .set_pane_chat_mode("task-1", "%1", true)
                .unwrap()
        });
        scope.spawn(|| {
            fixture
                .store
                .set_pane_chat_mode("task-1", "%2", false)
                .unwrap()
        });
    });

    assert_eq!(
        fixture.modes("task-1"),
        modes(&[("%1", true), ("%2", false)])
    );
}

// parity: TS-main-paneChatModeStore-729edf
#[test]
fn reads_broken_json_as_empty_and_saves_normally_afterwards() {
    let fixture = ModeStoreFixture::new();
    save_app_setting(
        &fixture.database.connection(),
        PANE_CHAT_MODES_KEY,
        "{not json",
    )
    .unwrap();

    assert_eq!(fixture.modes("task-1"), PaneChatModes::new());
    fixture
        .store
        .set_pane_chat_mode("task-1", "%1", true)
        .unwrap();
    assert_eq!(fixture.modes("task-1"), modes(&[("%1", true)]));
}

// parity: TS-main-paneChatModeStore-da16ca
#[test]
fn drops_only_the_least_recently_changed_task_past_the_limit() {
    let fixture = ModeStoreFixture::new();
    for index in 1..=MAX_TASKS_WITH_MODES {
        fixture
            .store
            .set_pane_chat_mode(&format!("task-{index}"), "%1", true)
            .unwrap();
    }
    // task-1을 다시 바꾸면 가장 오래된 작업은 task-2가 된다
    fixture
        .store
        .set_pane_chat_mode("task-1", "%2", true)
        .unwrap();

    let newest_task_id = format!("task-{}", MAX_TASKS_WITH_MODES + 1);
    fixture
        .store
        .set_pane_chat_mode(&newest_task_id, "%1", true)
        .unwrap();

    assert_eq!(fixture.modes("task-2"), PaneChatModes::new());
    assert_eq!(
        fixture.modes("task-1"),
        modes(&[("%1", true), ("%2", true)])
    );
    assert_eq!(fixture.modes("task-3"), modes(&[("%1", true)]));
    assert_eq!(fixture.modes(&newest_task_id), modes(&[("%1", true)]));
}

#[test]
fn rejects_empty_task_or_pane_id() {
    let fixture = ModeStoreFixture::new();

    let error = fixture
        .store
        .set_pane_chat_mode("", "%1", true)
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "pane 채팅 모드 저장 요청 형식이 올바르지 않습니다."
    );
    assert!(
        fixture
            .store
            .set_pane_chat_mode("task-1", "", true)
            .is_err()
    );
}
