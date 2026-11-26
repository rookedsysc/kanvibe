use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;

use super::*;

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    workspace: PathBuf,
    signals: PaneChatConnectionSignals,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let workspace = root.path().join("repo");
        std::fs::create_dir_all(home.join(".claude/sessions")).unwrap();
        std::fs::create_dir_all(workspace.join(".kanvibe/pane-chat")).unwrap();
        let signals = PaneChatConnectionSignals::new(home.to_str());
        Self {
            _root: root,
            home,
            workspace,
            signals,
        }
    }

    fn scope(&self, task_id: &str) -> PaneChatConnectionScope {
        PaneChatConnectionScope {
            task_id: task_id.to_string(),
            local_workspaces: vec![self.workspace.to_string_lossy().into_owned()],
        }
    }

    fn subscribe(
        &self,
        scope: PaneChatConnectionScope,
    ) -> (PaneChatConnectionSubscription, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let sink = count.clone();
        let subscription = self.signals.subscribe(
            scope,
            Arc::new(move || {
                sink.fetch_add(1, Ordering::SeqCst);
            }),
        );
        (subscription, count)
    }

    fn write_claude_record(&self, pid: u32, session_id: &str, cwd: &Path) -> PathBuf {
        let path = self.home.join(format!(".claude/sessions/{pid}.json"));
        let record = json!({"pid": pid, "sessionId": session_id, "cwd": cwd, "kind": "interactive", "startedAt": 1});
        std::fs::write(&path, record.to_string()).unwrap();
        path
    }

    fn write_opencode_route(
        &self,
        pid: u32,
        session_id: Value,
        revision: u64,
        updated_at: u64,
    ) -> PathBuf {
        let path = self
            .workspace
            .join(format!(".kanvibe/pane-chat/opencode-{pid}.json"));
        let route = json!({"pid": pid, "cwd": self.workspace, "sessionId": session_id, "revision": revision, "updatedAt": updated_at});
        std::fs::write(&path, route.to_string()).unwrap();
        path
    }
}

#[test]
fn claude_record_signals_once_per_new_session_id_in_the_task_workspace() {
    let fixture = Fixture::new();
    let (_subscription, count) = fixture.subscribe(fixture.scope("task-a"));
    let path = fixture.write_claude_record(41, "session-a", &fixture.workspace.join("src"));
    fixture.signals.handle_file_change(&path);
    assert_eq!(count.load(Ordering::SeqCst), 1);

    // 같은 대화를 다시 쓴 기록(상태 표시 등 다른 필드 변경)은 알리지 않는다
    fixture.signals.handle_file_change(&path);
    assert_eq!(count.load(Ordering::SeqCst), 1);

    // `/clear`·`/resume`으로 대화 ID가 바뀌면 다시 알린다
    fixture.write_claude_record(41, "session-b", &fixture.workspace);
    fixture.signals.handle_file_change(&path);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn claude_record_outside_the_workspace_or_temporary_files_do_not_signal() {
    let fixture = Fixture::new();
    let (_subscription, count) = fixture.subscribe(fixture.scope("task-a"));
    let sibling = fixture.workspace.with_file_name("repo-other");
    let other = fixture.write_claude_record(42, "session-x", &sibling);
    fixture.signals.handle_file_change(&other);
    let staging = fixture.home.join(".claude/sessions/43.tmp");
    std::fs::write(&staging, "{}").unwrap();
    fixture.signals.handle_file_change(&staging);
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn opencode_heartbeat_is_silent_and_conversation_or_revision_change_signals() {
    let fixture = Fixture::new();
    let (_subscription, count) = fixture.subscribe(fixture.scope("task-a"));
    let path = fixture.write_opencode_route(77, Value::Null, 1, 1_000);
    fixture.signals.handle_file_change(&path);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    fixture.write_opencode_route(77, Value::Null, 1, 2_000);
    fixture.signals.handle_file_change(&path);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    fixture.write_opencode_route(77, json!("ses_abc"), 2, 3_000);
    fixture.signals.handle_file_change(&path);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn hook_signal_reaches_only_that_task_and_stops_after_the_subscription_is_dropped() {
    let fixture = Fixture::new();
    let (subscription, task_a) = fixture.subscribe(fixture.scope("task-a"));
    let (_other, task_b) = fixture.subscribe(PaneChatConnectionScope {
        task_id: "task-b".into(),
        local_workspaces: Vec::new(),
    });
    fixture.signals.notify_task("task-a");
    assert_eq!(
        (task_a.load(Ordering::SeqCst), task_b.load(Ordering::SeqCst)),
        (1, 0)
    );
    drop(subscription);
    fixture.signals.notify_task("task-a");
    assert_eq!(task_a.load(Ordering::SeqCst), 1);
    // 남은 구독은 원격 범위라 디렉터리를 감시하지 않으므로, 감시도 함께 멈춘다
    assert!(fixture.signals.inner.lock().watches.is_empty());
}

#[test]
fn remote_task_scope_watches_no_local_directory() {
    let fixture = Fixture::new();
    let (_subscription, _) = fixture.subscribe(PaneChatConnectionScope {
        task_id: "remote".into(),
        local_workspaces: Vec::new(),
    });
    assert!(fixture.signals.inner.lock().watches.is_empty());
}

/// 실제 파일 감시: 새 Claude 세션 기록이 생기면 조회 주기를 기다리지 않고 알린다
#[test]
fn file_watch_signals_a_new_claude_session_record() {
    let fixture = Fixture::new();
    let (subscription, count) = fixture.subscribe(fixture.scope("task-a"));
    assert_eq!(subscription.directories.len(), 2);
    let staging = fixture.home.join(".claude/sessions/44.json.tmp");
    let record = json!({"pid": 44, "sessionId": "fresh", "cwd": fixture.workspace});
    std::fs::write(&staging, record.to_string()).unwrap();
    std::fs::rename(&staging, fixture.home.join(".claude/sessions/44.json")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while count.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
    drop(subscription);
    assert!(fixture.signals.inner.lock().watches.is_empty());
}
