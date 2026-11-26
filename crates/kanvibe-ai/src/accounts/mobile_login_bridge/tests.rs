use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::accounts::login_terminal::fake::FakeSpawner;
use crate::usage::test_context::TestUsage;

#[derive(Default)]
struct RecordingSocket {
    is_closed: AtomicBool,
    events: Mutex<Vec<MobileAiLoginEvent>>,
}

impl MobileLoginSocket for RecordingSocket {
    fn is_open(&self) -> bool {
        !self.is_closed.load(Ordering::SeqCst)
    }

    fn send_event(&self, event: MobileAiLoginEvent) {
        self.events.lock().unwrap().push(event);
    }
}

type ScheduledTask = (u64, Box<dyn FnOnce() + Send>, Receiver<()>);

/// 예약된 지연 작업을 쌓아 두었다가 테스트가 원할 때 돌린다
#[derive(Default)]
struct ManualTimers {
    tasks: Mutex<Vec<ScheduledTask>>,
}

impl ManualTimers {
    fn run_due(&self, elapsed_ms: u64) {
        let tasks = std::mem::take(&mut *self.tasks.lock().unwrap());
        for (delay_ms, task, cancelled) in tasks {
            let is_cancelled = cancelled.try_recv() == Err(mpsc::TryRecvError::Disconnected);
            if delay_ms <= elapsed_ms && !is_cancelled {
                task();
            }
        }
    }

    /// 아직 취소되지 않은 지연 작업 수
    fn pending_count(&self) -> usize {
        let tasks = self.tasks.lock().unwrap();
        tasks
            .iter()
            .filter(|(_, _, cancelled)| {
                cancelled.try_recv() != Err(mpsc::TryRecvError::Disconnected)
            })
            .count()
    }
}

impl LoginTimerScheduler for ManualTimers {
    fn schedule(&self, delay_ms: u64, task: Box<dyn FnOnce() + Send>) -> LoginTimerCancellation {
        let (cancellation, cancelled) = LoginTimerCancellation::new();
        self.tasks.lock().unwrap().push((delay_ms, task, cancelled));
        cancellation
    }
}

struct BridgeFixture {
    bridge: MobileLoginBridge,
    spawner: Arc<FakeSpawner>,
    timers: Arc<ManualTimers>,
}

fn fixture() -> BridgeFixture {
    let usage = TestUsage::new("/fixture");
    let spawner = Arc::new(FakeSpawner::default());
    let timers = Arc::new(ManualTimers::default());
    BridgeFixture {
        bridge: MobileLoginBridge::new(usage.context.clone(), spawner.clone(), timers.clone()),
        spawner,
        timers,
    }
}

fn request(
    device_id: &str,
    socket_id: &str,
    account_root: &str,
    socket: Arc<RecordingSocket>,
) -> MobileLoginOpenRequest {
    MobileLoginOpenRequest {
        device_id: device_id.to_string(),
        socket_id: socket_id.to_string(),
        provider: AiUsageProvider::Claude,
        account_root: account_root.to_string(),
        columns: 80.0,
        rows: 24.0,
        socket,
    }
}

fn default_request() -> (MobileLoginOpenRequest, Arc<RecordingSocket>) {
    let socket = Arc::new(RecordingSocket::default());
    (
        request("device-1", "socket-1", "/fixture/account", socket.clone()),
        socket,
    )
}

impl BridgeFixture {
    fn open_default(&self) -> Arc<RecordingSocket> {
        let (open_request, socket) = default_request();
        self.bridge
            .open_mobile_ai_login_session(open_request)
            .unwrap();
        socket
    }

    fn open_second_device(&self) {
        let socket = Arc::new(RecordingSocket::default());
        self.bridge
            .open_mobile_ai_login_session(request(
                "device-2",
                "socket-2",
                "/fixture/second",
                socket,
            ))
            .unwrap();
    }
}

#[test]
fn opens_provider_cli_with_scoped_environment_and_forwards_events() {
    let fixture = fixture();
    let socket = fixture.open_default();
    let spawned = fixture.spawner.request(0);
    assert_eq!(spawned.program, "claude");
    assert_eq!(spawned.args, ["auth", "login", "--claudeai"]);
    assert_eq!((spawned.columns, spawned.rows), (80, 24));
    assert_eq!(spawned.terminal_name, "xterm-color");
    assert_eq!(spawned.working_directory, "/fixture");
    assert_eq!(spawned.environment["CLAUDE_CONFIG_DIR"], "/fixture/account");
    fixture.spawner.emit_output(0, "enter code:");
    fixture.spawner.emit_exit(0, 0);
    assert_eq!(
        *socket.events.lock().unwrap(),
        [
            MobileAiLoginEvent::Output {
                data: "enter code:".to_string()
            },
            MobileAiLoginEvent::Exit { exit_code: 0 },
        ]
    );
    fixture
        .bridge
        .write_mobile_ai_login("device-1", "socket-1", "ignored")
        .unwrap();
    assert!(
        fixture
            .spawner
            .terminal(0)
            .writes
            .lock()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn codex_uses_device_auth_for_phone_browsers() {
    let fixture = fixture();
    let (mut open_request, _) = default_request();
    open_request.provider = AiUsageProvider::Codex;
    fixture
        .bridge
        .open_mobile_ai_login_session(open_request)
        .unwrap();
    let spawned = fixture.spawner.request(0);
    assert_eq!(spawned.program, "codex");
    assert_eq!(spawned.args, ["login", "--device-auth"]);
    assert_eq!(
        crate::usage::provider_cli::get_provider_login_command(AiUsageProvider::Codex).args,
        ["login"]
    );
}

#[test]
fn input_resize_and_close_belong_to_the_owning_socket() {
    let fixture = fixture();
    fixture.open_default();
    let terminal = fixture.spawner.terminal(0);
    fixture
        .bridge
        .write_mobile_ai_login("device-2", "socket-1", "attack")
        .unwrap();
    fixture
        .bridge
        .resize_mobile_ai_login("device-1", "another-socket", 90.0, 30.0)
        .unwrap();
    fixture.bridge.close_mobile_ai_login("device-2", "socket-1");
    assert!(terminal.writes.lock().unwrap().is_empty());
    assert!(terminal.resizes.lock().unwrap().is_empty());
    assert_eq!(terminal.kills(), 0);
    fixture
        .bridge
        .write_mobile_ai_login("device-1", "socket-1", "code\r")
        .unwrap();
    fixture
        .bridge
        .resize_mobile_ai_login("device-1", "socket-1", 90.0, 30.0)
        .unwrap();
    assert_eq!(*terminal.writes.lock().unwrap(), ["code\r"]);
    assert_eq!(*terminal.resizes.lock().unwrap(), [(90, 30)]);
    fixture.bridge.close_mobile_ai_login("device-1", "socket-1");
    assert_eq!(terminal.kills(), 1);
}

#[test]
fn one_login_writer_per_account() {
    let fixture = fixture();
    fixture.open_default();
    let socket = Arc::new(RecordingSocket::default());
    let error = fixture
        .bridge
        .open_mobile_ai_login_session(request("device-2", "socket-2", "/fixture/account", socket))
        .unwrap_err();
    assert_eq!(error.status_code, 409);
    assert!(error.message.contains("로그인"));
    assert_eq!(fixture.spawner.count(), 1);
}

#[test]
fn unpairing_closes_only_that_devices_processes() {
    let fixture = fixture();
    fixture.open_default();
    fixture.open_second_device();
    fixture.bridge.close_mobile_ai_logins_for_device("device-1");
    assert_eq!(fixture.spawner.terminal(0).kills(), 1);
    assert_eq!(fixture.spawner.terminal(1).kills(), 0);
    fixture
        .bridge
        .write_mobile_ai_login("device-2", "socket-2", "still active")
        .unwrap();
    assert_eq!(
        *fixture.spawner.terminal(1).writes.lock().unwrap(),
        ["still active"]
    );
}

#[test]
fn account_removal_kills_every_owner() {
    let fixture = fixture();
    let socket = fixture.open_default();
    fixture
        .bridge
        .close_mobile_ai_logins_for_account("/fixture/account");
    assert_eq!(fixture.spawner.terminal(0).kills(), 1);
    assert_eq!(
        *socket.events.lock().unwrap(),
        [MobileAiLoginEvent::Error {
            error: "계정 삭제를 위해 로그인 터미널을 닫았습니다".to_string()
        }]
    );
}

#[test]
fn abandoned_logins_expire_and_free_ownership() {
    let fixture = fixture();
    let socket = fixture.open_default();
    fixture.timers.run_due(10 * 60 * 1000);
    assert_eq!(fixture.spawner.terminal(0).kills(), 1);
    let events = socket.events.lock().unwrap().clone();
    assert!(matches!(&events[..], [MobileAiLoginEvent::Error { error }] if error.contains("만료")));
    fixture.open_default();
    assert_eq!(fixture.spawner.count(), 2);
}

#[test]
fn closing_a_login_cancels_its_expiry_timer() {
    let fixture = fixture();
    fixture.open_default();
    assert_eq!(fixture.timers.pending_count(), 1);

    fixture.bridge.close_mobile_ai_login("device-1", "socket-1");

    assert_eq!(fixture.timers.pending_count(), 0);
}

#[test]
fn a_cancelled_thread_timer_ends_without_waiting_for_its_delay() {
    let task_lifetime = Arc::new(());
    let task_capture = Arc::clone(&task_lifetime);
    let has_run = Arc::new(AtomicBool::new(false));
    let run_flag = Arc::clone(&has_run);
    let cancellation = ThreadTimerScheduler.schedule(
        LOGIN_TIMEOUT_MS,
        Box::new(move || {
            let _keep = task_capture;
            run_flag.store(true, Ordering::SeqCst);
        }),
    );

    drop(cancellation);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while Arc::strong_count(&task_lifetime) > 1 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        Arc::strong_count(&task_lifetime),
        1,
        "타이머 스레드가 작업을 버리고 끝나야 한다"
    );
    assert!(!has_run.load(Ordering::SeqCst));
}

#[test]
fn startup_failure_is_sanitized_and_releases_the_account() {
    let fixture = fixture();
    fixture
        .spawner
        .should_fail_next
        .store(true, Ordering::SeqCst);
    let (open_request, _) = default_request();
    let error = fixture
        .bridge
        .open_mobile_ai_login_session(open_request)
        .unwrap_err();
    assert_eq!(error.status_code, 500);
    assert!(error.message.contains("로그인 터미널"));
    assert!(!error.message.contains("secret"));
    fixture.open_default();
    assert_eq!(fixture.spawner.count(), 1);
}

#[test]
fn shutdown_clears_every_session() {
    let fixture = fixture();
    fixture.bridge.close_mobile_ai_logins_for_device("device-1");
    fixture.open_default();
    fixture.open_second_device();
    fixture.bridge.close_all_mobile_ai_logins();
    assert_eq!(
        [
            fixture.spawner.terminal(0).kills(),
            fixture.spawner.terminal(1).kills()
        ],
        [1, 1]
    );
}

#[test]
fn invalid_sizes_and_oversized_input_are_rejected_before_the_pty() {
    let fixture = fixture();
    let (mut open_request, _) = default_request();
    open_request.columns = -1.0;
    assert_eq!(
        fixture
            .bridge
            .open_mobile_ai_login_session(open_request)
            .unwrap_err()
            .status_code,
        400
    );
    fixture.open_default();
    let terminal = fixture.spawner.terminal(0);
    assert!(
        fixture
            .bridge
            .write_mobile_ai_login("device-1", "socket-1", &"x".repeat(65_537))
            .is_err()
    );
    assert!(
        fixture
            .bridge
            .resize_mobile_ai_login("device-1", "socket-1", 80.0, 251.0)
            .is_err()
    );
    assert!(terminal.writes.lock().unwrap().is_empty());
    assert!(terminal.resizes.lock().unwrap().is_empty());
}

#[test]
fn stale_callbacks_after_replacement_are_ignored() {
    let fixture = fixture();
    let first_socket = fixture.open_default();
    fixture.bridge.close_mobile_ai_login("device-1", "socket-1");
    fixture.open_default();
    fixture.spawner.emit_output(0, "stale");
    fixture.spawner.emit_exit(0, 0);
    fixture
        .bridge
        .write_mobile_ai_login("device-1", "socket-1", "new")
        .unwrap();
    assert!(first_socket.events.lock().unwrap().is_empty());
    assert_eq!(*fixture.spawner.terminal(1).writes.lock().unwrap(), ["new"]);
}

#[test]
fn closed_socket_opens_nothing() {
    let fixture = fixture();
    let (open_request, socket) = default_request();
    socket.is_closed.store(true, Ordering::SeqCst);
    fixture
        .bridge
        .open_mobile_ai_login_session(open_request)
        .unwrap();
    assert_eq!(fixture.spawner.count(), 0);
}

#[test]
fn device_and_total_limits_are_enforced() {
    let fixture = fixture();
    for index in 0..4 {
        let socket = Arc::new(RecordingSocket::default());
        fixture
            .bridge
            .open_mobile_ai_login_session(request(
                "device-1",
                &format!("socket-{index}"),
                &format!("/fixture/{index}"),
                socket,
            ))
            .unwrap();
    }
    let socket = Arc::new(RecordingSocket::default());
    let error = fixture
        .bridge
        .open_mobile_ai_login_session(request("device-1", "socket-9", "/fixture/9", socket))
        .unwrap_err();
    assert_eq!(error.status_code, 429);
}
