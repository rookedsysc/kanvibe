//! 연결 없이 PTY attachment만 레코드로 몰아 본다(TS `openRemoteHostPty` 단위 테스트).

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kanvibe_remote_agent::protocol::{decode_host_bytes, encode_host_bytes, record_from};
use kanvibe_terminal::pty::{PtyEventCallbacks, PtyExit, PtyProcess};
use kanvibe_terminal::terminal_registry::RemoteHostPty;
use serde_json::{Value, json};

use crate::connection::RemotePtyAttachment;

struct Fixture {
    attachment: Arc<RemotePtyAttachment>,
    records: Arc<Mutex<Vec<Value>>>,
    outputs: Arc<Mutex<Vec<String>>>,
    exits: Arc<Mutex<Vec<PtyExit>>>,
    render_gate: Option<Sender<()>>,
}

/// `is_render_gated`면 화면 콜백이 테스트가 풀어 줄 때까지 돌아오지 않는다
fn fixture(is_render_gated: bool) -> Fixture {
    let records = Arc::new(Mutex::new(Vec::new()));
    let (outputs, exits) = (
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(Mutex::new(Vec::new())),
    );
    let (render_gate, renders): (Sender<()>, Receiver<()>) = channel();
    let renders = Mutex::new(renders);
    let recorded = Arc::clone(&records);
    let send = Arc::new(move |record: Value| {
        recorded.lock().unwrap().push(record);
        Ok(())
    });
    let (output_log, exit_log) = (Arc::clone(&outputs), Arc::clone(&exits));
    let callbacks = PtyEventCallbacks {
        on_output: Box::new(move |text| {
            output_log.lock().unwrap().push(text.to_string());
            if is_render_gated {
                let _ = renders.lock().unwrap().recv();
            }
        }),
        on_exit: Box::new(move |exit| exit_log.lock().unwrap().push(exit)),
    };
    let attachment = RemotePtyAttachment::start("attachment", send, 80, 24, callbacks);
    attachment.receive(&record_from(json!({ "type": "pty.ready", "sessionId": "daemon-issued", "pid": 1234, "cols": 80, "rows": 24 })));
    attachment.wait_ready(Duration::from_secs(1)).unwrap();
    Fixture {
        attachment,
        records,
        outputs,
        exits,
        render_gate: is_render_gated.then_some(render_gate),
    }
}

impl Fixture {
    fn receive(&self, record: Value) {
        self.attachment.receive(&record_from(record));
    }

    fn data(&self, bytes: &[u8]) {
        self.receive(json!({ "type": "pty.data", "data": encode_host_bytes(bytes) }));
    }

    fn render(&self) {
        self.render_gate.as_ref().unwrap().send(()).unwrap();
    }

    fn acks(&self) -> Vec<u64> {
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter(|record| record["type"] == "pty.ack")
            .map(|record| record["bytes"].as_u64().unwrap())
            .collect()
    }

    fn last_record(&self) -> Value {
        self.records.lock().unwrap().last().cloned().unwrap()
    }

    fn records_of(&self, kind: &str) -> Vec<Value> {
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter(|record| record["type"] == kind)
            .cloned()
            .collect()
    }
}

fn until(description: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(Instant::now() < deadline, "{description}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn settle() {
    std::thread::sleep(Duration::from_millis(30));
}

// parity: TS-lib-remoteHostPtyConnection-dbe59b
#[test]
fn final_buffered_output_is_rendered_before_the_exit_is_reported() {
    let fixture = fixture(true);

    fixture.data(b"last output");
    fixture.receive(json!({ "type": "pty.exit", "exitCode": 7 }));
    until("마지막 출력", || {
        fixture.outputs.lock().unwrap().len() == 1
    });
    settle();
    assert!(fixture.exits.lock().unwrap().is_empty());
    fixture.render();

    until("종료 알림", || fixture.exits.lock().unwrap().len() == 1);
    assert_eq!(
        *fixture.outputs.lock().unwrap(),
        vec!["last output".to_string()]
    );
    assert_eq!(fixture.exits.lock().unwrap()[0].exit_code, 7);
}

// parity: TS-lib-remoteHostPtyConnection-347d50
#[test]
fn snapshot_bytes_after_ready_are_acknowledged_only_after_rendering() {
    let fixture = fixture(true);

    fixture.data(b"screen");
    until("화면 전달", || {
        fixture.outputs.lock().unwrap().len() == 1
    });
    settle();
    assert!(fixture.acks().is_empty());
    fixture.render();

    until("ACK", || fixture.acks() == vec![6]);
    fixture.attachment.kill().unwrap();
}

// parity: TS-lib-remoteHostPtyConnection-1d51b0
#[test]
fn split_utf8_and_live_frames_are_rendered_one_at_a_time_in_order() {
    let fixture = fixture(true);
    let bytes = "한글".as_bytes();

    fixture.data(&bytes[..1]);
    fixture.data(&bytes[1..]);
    fixture.data(b"live");
    until("완성된 글자", || {
        fixture.outputs.lock().unwrap().len() == 1
    });
    settle();
    assert_eq!(*fixture.outputs.lock().unwrap(), vec!["한글".to_string()]);
    assert_eq!(fixture.acks(), vec![1]);
    fixture.render();
    until("다음 프레임", || {
        fixture.outputs.lock().unwrap().len() == 2
    });
    assert_eq!(fixture.acks(), vec![1, 5]);
    fixture.render();

    until("마지막 ACK", || fixture.acks() == vec![1, 5, 4]);
    assert_eq!(
        *fixture.outputs.lock().unwrap(),
        vec!["한글".to_string(), "live".to_string()]
    );
    fixture.attachment.kill().unwrap();
}

// parity: TS-lib-remoteHostPtyConnection-8a5337
#[test]
fn the_output_ack_waits_until_the_screen_consumer_has_rendered() {
    let fixture = fixture(true);

    fixture.data(b"screen");
    until("화면 전달", || {
        fixture.outputs.lock().unwrap().len() == 1
    });
    settle();
    assert!(fixture.acks().is_empty());
    fixture.render();

    until("ACK", || fixture.acks() == vec![6]);
    assert_eq!(
        fixture.last_record(),
        json!({ "type": "pty.ack", "id": "attachment", "bytes": 6 })
    );
    fixture.attachment.kill().unwrap();
}

// parity: TS-lib-remoteHostPtyConnection-7a215a
#[test]
fn a_large_paste_is_chunked_with_one_chunk_in_flight_and_an_overfull_queue_is_rejected() {
    let fixture = fixture(false);

    fixture.attachment.write(&"x".repeat(200_000)).unwrap();
    assert_eq!(fixture.records_of("pty.input").len(), 1);
    for _ in 0..7 {
        fixture.receive(json!({ "type": "pty.input-ready" }));
    }

    let inputs = fixture.records_of("pty.input");
    assert_eq!(inputs.len(), 7);
    let sent: usize = inputs
        .iter()
        .map(|record| {
            decode_host_bytes(record.get("data"), 32 * 1024)
                .unwrap()
                .len()
        })
        .sum();
    assert_eq!(sent, 200_000);
    let overflow = fixture
        .attachment
        .write(&"x".repeat(8 * 1024 * 1024 + 1))
        .unwrap_err();
    assert!(overflow.contains("backpressure"), "{overflow}");
    fixture.attachment.kill().unwrap();
}

// parity: TS-lib-remoteHostPtyConnection-80c8dd
#[test]
fn a_disconnect_drops_queued_input_while_output_bytes_are_acknowledged() {
    let fixture = fixture(false);
    let bytes = "한글".as_bytes();

    fixture.data(&bytes[..1]);
    fixture.data(&bytes[1..]);
    until("ACK", || fixture.acks() == vec![1, 5]);
    assert_eq!(fixture.outputs.lock().unwrap().concat(), "한글");
    fixture.attachment.write(&"\0".repeat(100_000)).unwrap();
    fixture.receive(json!({ "type": "disconnected" }));
    let count = fixture.records.lock().unwrap().len();
    fixture.receive(json!({ "type": "pty.input-ready" }));

    assert_eq!(fixture.records.lock().unwrap().len(), count);
    let replay = fixture.attachment.write("again").unwrap_err();
    assert!(replay.contains("not replayed"), "{replay}");
}

// parity: TS-lib-remoteHostPtyConnection-061c30
#[test]
fn detach_and_explicit_close_are_separate_and_the_daemon_identity_is_kept() {
    let fixture = fixture(false);
    assert_eq!(fixture.attachment.session_id(), "daemon-issued");
    assert_eq!(fixture.attachment.pid(), 1234);

    fixture.attachment.resize(110, 40).unwrap();
    assert_eq!(
        fixture.last_record(),
        json!({ "type": "pty.resize", "id": "attachment", "cols": 110, "rows": 40 })
    );
    fixture.attachment.kill().unwrap();
    assert_eq!(fixture.last_record()["type"], "pty.detach");
    assert!(fixture.records_of("pty.close").is_empty());

    let second = self::fixture(false);
    second.attachment.close_session().unwrap();
    assert_eq!(second.last_record()["type"], "pty.close");
    second.receive(json!({ "type": "pty.exit", "exitCode": 0 }));
    until("종료", || second.exits.lock().unwrap().len() == 1);
}
