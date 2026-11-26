use std::sync::atomic::Ordering;

use super::*;
use crate::accounts::login_terminal::fake::FakeSpawner;
use crate::usage::test_context::TestUsage;

const WINDOW_ID: u64 = 91;
const ACCOUNT_ROOT: &str = "/tmp/account";

#[derive(Default)]
struct RecordingWindow {
    outputs: Mutex<Vec<(String, String)>>,
    exits: Mutex<Vec<(String, i32)>>,
}

impl LoginWindow for RecordingWindow {
    fn id(&self) -> u64 {
        WINDOW_ID
    }

    fn is_destroyed(&self) -> bool {
        false
    }

    fn send_output(&self, account_root: &str, data: &str) {
        self.outputs
            .lock()
            .unwrap()
            .push((account_root.to_string(), data.to_string()));
    }

    fn send_exit(&self, account_root: &str, exit_code: i32) {
        self.exits
            .lock()
            .unwrap()
            .push((account_root.to_string(), exit_code));
    }
}

struct BridgeFixture {
    bridge: DesktopLoginBridge,
    spawner: Arc<FakeSpawner>,
    window: Arc<RecordingWindow>,
    warnings: Arc<Mutex<Vec<String>>>,
}

fn fixture() -> BridgeFixture {
    let usage = TestUsage::new("/tmp");
    let spawner = Arc::new(FakeSpawner::default());
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let recorded = warnings.clone();
    let bridge = DesktopLoginBridge::new(
        usage.context.clone(),
        spawner.clone(),
        Arc::new(move |message: &str| recorded.lock().unwrap().push(message.to_string())),
    );
    BridgeFixture {
        bridge,
        spawner,
        window: Arc::new(RecordingWindow::default()),
        warnings,
    }
}

impl BridgeFixture {
    fn open(&self, provider: AiUsageProvider, account_root: &str) -> AiAccountLoginOpenResult {
        self.bridge
            .open_ai_account_login(self.window.clone(), provider, account_root, 80, 24)
    }
}

fn ok() -> AiAccountLoginOpenResult {
    AiAccountLoginOpenResult {
        ok: true,
        error: None,
    }
}

#[test]
fn survives_operations_after_native_pty_closes_before_exit() {
    for operation in ["resize", "write", "kill"] {
        let fixture = fixture();
        fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT);
        fixture
            .spawner
            .terminal(0)
            .is_broken
            .store(true, Ordering::SeqCst);
        match operation {
            "resize" => fixture
                .bridge
                .resize_ai_account_login(WINDOW_ID, ACCOUNT_ROOT, 100, 30),
            "write" => fixture
                .bridge
                .write_ai_account_login(WINDOW_ID, ACCOUNT_ROOT, "code"),
            _ => fixture
                .bridge
                .close_ai_account_login(WINDOW_ID, ACCOUNT_ROOT),
        }
        assert_eq!(fixture.warnings.lock().unwrap().len(), 1, "{operation}");
        fixture.spawner.emit_exit(0, 1);
        assert_eq!(fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT), ok());
    }
}

#[test]
fn closing_continues_when_one_kill_fails() {
    let fixture = fixture();
    fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT);
    fixture.open(AiUsageProvider::Codex, "/tmp/second");
    fixture
        .spawner
        .terminal(0)
        .is_broken
        .store(true, Ordering::SeqCst);
    fixture.bridge.close_window_ai_account_logins(WINDOW_ID);
    assert_eq!(fixture.spawner.terminal(1).kills(), 1);
}

#[test]
fn forwards_dimensions_and_ignores_resize_after_exit() {
    let fixture = fixture();
    fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT);
    fixture
        .bridge
        .resize_ai_account_login(WINDOW_ID, ACCOUNT_ROOT, 100, 30);
    let terminal = fixture.spawner.terminal(0);
    assert_eq!(*terminal.resizes.lock().unwrap(), [(100, 30)]);
    fixture.spawner.emit_exit(0, 0);
    fixture
        .bridge
        .resize_ai_account_login(WINDOW_ID, ACCOUNT_ROOT, 120, 40);
    assert_eq!(terminal.resizes.lock().unwrap().len(), 1);
    assert_eq!(
        *fixture.window.exits.lock().unwrap(),
        [(ACCOUNT_ROOT.to_string(), 0)]
    );
}

#[test]
fn concurrent_opens_share_one_pty_that_close_kills() {
    let fixture = fixture();
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT));
        let second = scope.spawn(|| fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(results, [ok(), ok()]);
    assert_eq!(fixture.spawner.count(), 1);
    fixture
        .bridge
        .close_ai_account_login(WINDOW_ID, ACCOUNT_ROOT);
    assert_eq!(fixture.spawner.terminal(0).kills(), 1);
}

#[test]
fn stale_exit_after_replacement_is_ignored() {
    let fixture = fixture();
    fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT);
    fixture
        .bridge
        .close_ai_account_login(WINDOW_ID, ACCOUNT_ROOT);
    fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT);
    fixture.spawner.emit_exit(0, 0);
    fixture
        .bridge
        .write_ai_account_login(WINDOW_ID, ACCOUNT_ROOT, "hello");
    assert_eq!(
        *fixture.spawner.terminal(1).writes.lock().unwrap(),
        ["hello"]
    );
    assert!(fixture.window.exits.lock().unwrap().is_empty());
    assert!(fixture.window.outputs.lock().unwrap().is_empty());
}

#[test]
fn login_pty_runs_provider_cli_from_home_with_account_location() {
    let fixture = fixture();
    fixture.open(AiUsageProvider::Claude, "/tmp/.claude-work");
    let request = fixture.spawner.request(0);
    assert_eq!(request.program, "claude");
    assert_eq!(request.args, ["auth", "login", "--claudeai"]);
    assert_eq!(request.terminal_name, "xterm-color");
    assert_eq!(request.working_directory, "/tmp");
    assert_eq!(
        request.environment["CLAUDE_CONFIG_DIR"],
        "/tmp/.claude-work"
    );
    fixture.spawner.emit_output(0, "Paste code:");
    assert_eq!(
        *fixture.window.outputs.lock().unwrap(),
        [("/tmp/.claude-work".to_string(), "Paste code:".to_string())]
    );
}

#[test]
fn spawn_failure_reports_error_and_frees_the_slot() {
    let fixture = fixture();
    fixture
        .spawner
        .should_fail_next
        .store(true, Ordering::SeqCst);
    let failed = fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT);
    assert!(!failed.ok);
    assert!(failed.error.is_some());
    assert_eq!(fixture.open(AiUsageProvider::Claude, ACCOUNT_ROOT), ok());
}
