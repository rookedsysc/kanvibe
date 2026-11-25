use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use super::*;
use crate::daemon::pty_launcher::{NativePtyLauncher, PtyExitStatus};

struct RecordingPeer {
    client_id: String,
    records: Arc<Mutex<Vec<HostRecord>>>,
}

impl PtyPeer for RecordingPeer {
    fn client_id(&self) -> &str {
        &self.client_id
    }

    fn send(&self, record: HostRecord) -> bool {
        self.records.lock().unwrap().push(record);
        true
    }
}

struct Fixture {
    registry: Arc<PtyRegistry>,
    peer: Arc<dyn PtyPeer>,
    records: Arc<Mutex<Vec<HostRecord>>>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.registry.close();
    }
}

fn fixture_with(launcher: Arc<dyn PtyLauncher>) -> Fixture {
    let records = Arc::new(Mutex::new(Vec::new()));
    let peer: Arc<dyn PtyPeer> = Arc::new(RecordingPeer {
        client_id: "desktop-one".into(),
        records: Arc::clone(&records),
    });
    Fixture {
        registry: PtyRegistry::new(launcher),
        peer,
        records,
    }
}

fn fixture() -> Fixture {
    fixture_with(Arc::new(NativePtyLauncher))
}

impl Fixture {
    fn other_peer(&self, client_id: &str) -> Arc<dyn PtyPeer> {
        Arc::new(RecordingPeer {
            client_id: client_id.into(),
            records: Arc::clone(&self.records),
        })
    }

    fn open(&self, id: &str, extra: Value) -> Result<HostRecord, String> {
        self.open_as(&self.peer, id, extra)
    }

    fn open_as(
        &self,
        peer: &Arc<dyn PtyPeer>,
        id: &str,
        extra: Value,
    ) -> Result<HostRecord, String> {
        let mut record = json!({ "type": "pty.open", "id": id, "key": "task-tab", "command": "exec /bin/sh", "cols": 90, "rows": 24 });
        for (key, value) in extra.as_object().cloned().unwrap_or_default() {
            record[key] = value;
        }
        self.registry.receive(peer, &record_from(record))?;
        Ok(self.last_of("pty.ready").expect("pty.ready"))
    }

    fn send(&self, record: Value) -> Result<(), String> {
        self.registry.receive(&self.peer, &record_from(record))
    }

    fn last_of(&self, kind: &str) -> Option<HostRecord> {
        let records = self.records.lock().unwrap();
        records
            .iter()
            .rev()
            .find(|record| record_text(record, "type") == Some(kind))
            .cloned()
    }

    fn count_of(&self, kind: &str) -> usize {
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter(|record| record_text(record, "type") == Some(kind))
            .count()
    }

    fn output(&self) -> String {
        let records = self.records.lock().unwrap();
        let bytes: Vec<u8> = records
            .iter()
            .filter(|record| record_text(record, "type") == Some("pty.data"))
            .flat_map(|record| decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES).unwrap())
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn has_record(&self, condition: impl Fn(&HostRecord) -> bool) -> bool {
        self.records.lock().unwrap().iter().any(condition)
    }
}

fn until(description: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "시간 안에 {description} 조건을 만족하지 못했다"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn pid_of(record: &HostRecord) -> i32 {
    record_integer(record, "pid").unwrap() as i32
}

fn is_alive(pid: i32) -> bool {
    // SAFETY: 신호 0은 존재 확인만 한다
    unsafe { libc::kill(pid, 0) == 0 }
}

fn input(id: &str, bytes: &[u8]) -> Value {
    json!({ "type": "pty.input", "id": id, "data": encode_host_bytes(bytes) })
}

// parity: TS-lib-remoteHostPty-55bb09
#[test]
fn attachment_capacity_is_reserved_synchronously_for_a_burst_of_opens() {
    let fixture = fixture();
    let mut accepted = 0;
    let mut rejected = 0;

    for index in 0..32 {
        match fixture.open(&format!("batch-{index}"), json!({})) {
            Ok(_) => accepted += 1,
            Err(error) => {
                assert!(error.contains("capacity"), "{error}");
                rejected += 1;
            }
        }
    }

    assert_eq!((accepted, rejected), (16, 16));
}

/// 종료와 출력 끝을 테스트가 따로 일으키는 PTY
struct ScriptedLauncher {
    exits: Mutex<Option<Receiver<()>>>,
    output_end: Mutex<Option<UnixStream>>,
}

struct ChannelWaiter(Receiver<()>);

impl PtyWaiter for ChannelWaiter {
    fn wait(self: Box<Self>) -> PtyExitStatus {
        let _ = self.0.recv();
        PtyExitStatus { exit_code: 0 }
    }
}

struct SilentControl;

impl PtyControl for SilentControl {
    fn resize(&self, _columns: u16, _rows: u16) -> Result<(), String> {
        Ok(())
    }

    fn kill(&self) {}
}

impl PtyLauncher for ScriptedLauncher {
    fn launch(&self, _command: &str, _columns: u16, _rows: u16) -> Result<LaunchedPty, String> {
        let (reader, writer_end) = UnixStream::pair().map_err(|error| error.to_string())?;
        *self.output_end.lock().unwrap() = Some(writer_end);
        Ok(LaunchedPty {
            pid: 12345,
            reader: Box::new(reader),
            writer: Box::new(std::io::sink()),
            control: Box::new(SilentControl),
            waiter: Box::new(ChannelWaiter(
                self.exits
                    .lock()
                    .unwrap()
                    .take()
                    .expect("PTY는 한 번만 띄운다"),
            )),
        })
    }
}

// parity: TS-lib-remoteHostPty-009374
#[test]
fn attach_is_rejected_while_the_exited_session_flushes_its_output() {
    let (exit, exits): (Sender<()>, Receiver<()>) = channel();
    let launcher = Arc::new(ScriptedLauncher {
        exits: Mutex::new(Some(exits)),
        output_end: Mutex::new(None),
    });
    let fixture = fixture_with(launcher.clone());
    let first = fixture.open("first", json!({})).unwrap();

    exit.send(()).unwrap();

    let session_id = first["sessionId"].clone();
    until("세션이 종료 중으로 바뀐다", || {
        let attempt = fixture.open(
            &uuid::Uuid::new_v4().to_string(),
            json!({ "sessionId": session_id }),
        );
        attempt.is_err_and(|error| error.contains("closing") || error.contains("exited"))
    });
    let by_key = fixture.open("during-close-key", json!({})).unwrap_err();
    assert!(
        by_key.contains("closing") || by_key.contains("exited"),
        "{by_key}"
    );
    launcher.output_end.lock().unwrap().take();
    until("첫 attachment가 종료를 받는다", || {
        fixture.has_record(|record| {
            record_text(record, "type") == Some("pty.exit")
                && record_text(record, "id") == Some("first")
        })
    });
}

// parity: TS-lib-remoteHostPty-711f2e
#[test]
fn lists_only_the_owners_detached_sessions_and_closes_one_by_daemon_id() {
    let fixture = fixture();
    let first = fixture.open("one", json!({})).unwrap();
    fixture.registry.detach(&fixture.peer);

    fixture
        .send(json!({ "type": "pty.list", "id": "list" }))
        .unwrap();
    let other = fixture.other_peer("another-desktop");
    fixture
        .registry
        .receive(
            &other,
            &record_from(json!({ "type": "pty.list", "id": "other-list" })),
        )
        .unwrap();
    let stolen_close = record_from(
        json!({ "type": "pty.close", "id": "bad-close", "sessionId": first["sessionId"] }),
    );
    let stolen = fixture.registry.receive(&other, &stolen_close).unwrap_err();
    fixture
        .send(json!({ "type": "pty.close", "id": "close", "sessionId": first["sessionId"] }))
        .unwrap();

    let listed = fixture
        .records
        .lock()
        .unwrap()
        .iter()
        .find(|record| record_text(record, "id") == Some("list"))
        .cloned()
        .unwrap();
    assert_eq!(
        listed["sessions"],
        json!([{ "sessionId": first["sessionId"], "key": "task-tab", "pid": first["pid"] }])
    );
    let other_listed = fixture
        .records
        .lock()
        .unwrap()
        .iter()
        .find(|record| record_text(record, "id") == Some("other-list"))
        .cloned()
        .unwrap();
    assert_eq!(other_listed["sessions"], json!([]));
    assert!(
        stolen.contains("unowned") || stolen.contains("Unknown"),
        "{stolen}"
    );
    until("닫은 세션의 셸이 끝난다", || {
        !is_alive(pid_of(&first))
    });
}

// parity: TS-lib-remoteHostPty-b8c56b
#[test]
fn session_ids_and_the_shell_pid_survive_detach_and_reattach() {
    let fixture = fixture();
    let first = fixture.open("first", json!({})).unwrap();
    assert_eq!(record_text(&first, "sessionId").unwrap().len(), 36);
    assert!(pid_of(&first) > 0);

    fixture.registry.detach(&fixture.peer);
    assert!(is_alive(pid_of(&first)));
    let second = fixture.open("second", json!({})).unwrap();

    assert_eq!(second["sessionId"], first["sessionId"]);
    assert_eq!(second["pid"], first["pid"]);
    fixture
        .send(input(
            "second",
            b"printf '\\121\\101\\137\\111\\116\\120\\125\\124\\137\\117\\113\\n'\n",
        ))
        .unwrap();
    until("입력 결과가 나온다", || {
        fixture.output().contains("QA_INPUT_OK")
    });
    fixture
        .send(json!({ "type": "pty.resize", "id": "second", "cols": 111, "rows": 37 }))
        .unwrap();
    fixture.send(input("second", b"stty size\n")).unwrap();
    until("바뀐 크기가 보인다", || {
        fixture.output().contains("37 111")
    });
    fixture
        .send(json!({ "type": "pty.close", "id": "second" }))
        .unwrap();
    until("종료를 받는다", || {
        fixture.has_record(|record| {
            record_text(record, "type") == Some("pty.exit")
                && record_text(record, "id") == Some("second")
        })
    });
}

// parity: TS-lib-remoteHostPty-8d3c36
#[test]
fn another_desktop_can_neither_attach_to_nor_close_a_session() {
    let fixture = fixture();
    let first = fixture.open("one", json!({})).unwrap();
    let other = fixture.other_peer("desktop-two");

    let steal = fixture
        .open_as(
            &other,
            "steal",
            json!({ "sessionId": first["sessionId"], "cols": 80 }),
        )
        .unwrap_err();
    fixture
        .registry
        .receive(
            &other,
            &record_from(json!({ "type": "pty.close", "id": "one" })),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let own = fixture
        .open_as(&other, "two", json!({ "cols": 80 }))
        .unwrap();

    assert!(
        steal.contains("unowned") || steal.contains("Unknown"),
        "{steal}"
    );
    assert!(is_alive(pid_of(&first)));
    assert_ne!(own["sessionId"], first["sessionId"]);
}

// parity: TS-lib-remoteHostPty-7b6d53
#[test]
fn invalid_bounds_and_duplicate_attachment_ids_are_rejected() {
    let fixture = fixture();
    fixture.open("one", json!({})).unwrap();

    let duplicate = fixture.open("one", json!({})).unwrap_err();
    let bad_size = fixture.open("bad", json!({ "rows": 0 })).unwrap_err();
    let oversized = fixture
        .send(input("one", &vec![0; HOST_CHUNK_BYTES + 1]))
        .unwrap_err();

    assert!(duplicate.contains("Duplicate"), "{duplicate}");
    assert!(bad_size.contains("size"), "{bad_size}");
    assert!(oversized.contains("bytes"), "{oversized}");
    assert_eq!(fixture.registry.state.lock().unwrap().sessions.len(), 1);
}

// parity: TS-lib-remoteHostPty-f28b89
#[test]
fn a_large_raw_paste_is_written_once_and_each_chunk_is_acknowledged() {
    let fixture = fixture();
    fixture
        .open(
            "paste",
            json!({ "command": "stty raw -echo; printf READY; head -c 262144 | wc -c; sleep 10" }),
        )
        .unwrap();
    until("준비 표시", || fixture.output().contains("READY"));

    for _ in 0..8 {
        fixture
            .send(input("paste", &vec![b'x'; HOST_CHUNK_BYTES]))
            .unwrap();
    }

    until("입력 ACK 8개", || {
        fixture.count_of("pty.input-ready") == 8
    });
    until("받은 바이트 수", || {
        fixture.output().contains("262144")
    });
}

// parity: TS-lib-remoteHostPty-5986f4
#[test]
fn input_acks_wait_for_the_reader_and_queued_paste_memory_is_bounded() {
    let fixture = fixture();
    fixture
        .open(
            "blocked",
            json!({ "command": "stty raw -echo; printf READY; sleep 30" }),
        )
        .unwrap();
    until("준비 표시", || fixture.output().contains("READY"));

    for _ in 0..32 {
        fixture
            .send(input("blocked", &vec![b'x'; HOST_CHUNK_BYTES]))
            .unwrap();
    }
    std::thread::sleep(Duration::from_millis(50));
    let acknowledged = fixture.count_of("pty.input-ready");
    let overflow = (0..257)
        .map(|_| fixture.send(input("blocked", &vec![b'x'; HOST_CHUNK_BYTES])))
        .find_map(Result::err);

    assert!(acknowledged < 32, "{acknowledged}");
    assert!(overflow.is_some_and(|error| error.contains("backpressure")));
    fixture.registry.detach(&fixture.peer);
}

// parity: TS-lib-remoteHostPty-70f50f
#[test]
fn a_consumer_that_never_acks_is_detached_while_the_session_survives() {
    let fixture = fixture();
    let first = fixture
        .open(
            "slow",
            json!({ "command": "head -c 2097152 /dev/zero | tr '\\000' x; sleep 30" }),
        )
        .unwrap();

    until("출력 흐름 제어로 떨어진다", || {
        fixture.count_of("pty.detached") > 0
    });

    assert!(is_alive(pid_of(&first)));
    let again = fixture
        .open(
            "again",
            json!({ "command": "head -c 2097152 /dev/zero | tr '\\000' x; sleep 30" }),
        )
        .unwrap();
    assert_eq!(again["pid"], first["pid"]);
    assert_eq!(again["sessionId"], first["sessionId"]);
}
