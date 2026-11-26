//! daemon으로 오래 도는 원격 프로세스 하나를 띄워 stdin·stdout을 스트림으로 잇는다(TS `HostConnection.createProcess`).
//!
//! `execute`와 같은 `exec.*` 레코드를 쓰지만 결과를 모으지 않는다. stdin은 32 KiB 조각마다 `exec.stdin-ready`를 기다린 뒤
//! 다음 조각을 보내고, stdin을 놓으면 `end`를 보낸다. stderr는 버린다(TS `child.stderr.resume()`).
//! 읽지 않은 stdout이 버퍼 상한을 넘으면 TS처럼 프로세스를 취소한다.

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use kanvibe_remote_agent::protocol::{
    HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, HostRecord, decode_host_bytes, encode_host_bytes,
    record_text,
};
use serde_json::json;

use super::HostConnection;

/// daemon으로 띄운 프로세스. 세 부분을 따로 다른 스레드에 넘길 수 있다
pub struct HostProcess {
    pub stdin: HostProcessInput,
    pub stdout: HostProcessOutput,
    pub termination: HostProcessTermination,
}

/// 프로세스 하나의 구독 상태. 끝나거나 취소하면 구독을 한 번만 푼다
struct ProcessSubscription {
    connection: Arc<HostConnection>,
    id: String,
    has_ended: AtomicBool,
    buffered_stdout_bytes: AtomicUsize,
}

impl ProcessSubscription {
    fn end(&self) {
        if !self.has_ended.swap(true, Ordering::SeqCst) {
            self.connection.unsubscribe(&self.id);
        }
    }

    fn cancel(&self) {
        if !self.has_ended.load(Ordering::SeqCst) {
            let _ = self
                .connection
                .send(json!({ "type": "exec.cancel", "id": self.id }));
        }
        self.end();
    }
}

impl HostConnection {
    /// `command`를 daemon에서 띄운다. `timeout`이 지나면 daemon이 프로세스를 끝낸다
    pub fn create_process(
        self: &Arc<Self>,
        command: &str,
        timeout: Duration,
    ) -> Result<HostProcess, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let subscription = Arc::new(ProcessSubscription {
            connection: Arc::clone(self),
            id: id.clone(),
            has_ended: AtomicBool::new(false),
            buffered_stdout_bytes: AtomicUsize::new(0),
        });
        let (output_sender, output_receiver) = channel();
        let (input_ready_sender, input_ready_receiver) = channel();
        let receiving = Arc::clone(&subscription);
        self.subscribe(
            &id,
            Arc::new(move |record: &HostRecord| {
                receive_process_record(record, &receiving, &output_sender, &input_ready_sender);
            }),
        )?;
        let open = json!({ "type": "exec.open", "id": id, "command": command, "timeoutMs": timeout.as_millis() as u64, "maxBufferBytes": 0, "interactive": true });
        if let Err(error) = self.send(open) {
            subscription.end();
            return Err(error);
        }
        Ok(HostProcess {
            stdin: HostProcessInput {
                subscription: Arc::clone(&subscription),
                input_ready: input_ready_receiver,
            },
            stdout: HostProcessOutput {
                subscription: Arc::clone(&subscription),
                output: output_receiver,
                unread: Vec::new(),
                unread_offset: 0,
            },
            termination: HostProcessTermination { subscription },
        })
    }
}

/// stdout 조각은 읽는 쪽으로, stdin ACK는 쓰는 쪽으로 보낸다. 끝나면 두 쪽 모두 깨운다
fn receive_process_record(
    record: &HostRecord,
    subscription: &ProcessSubscription,
    output: &Sender<Vec<u8>>,
    input_ready: &Sender<bool>,
) {
    match record_text(record, "type") {
        Some("exec.data") if record_text(record, "stream") == Some("stdout") => {
            let Ok(bytes) = decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES) else {
                return;
            };
            let buffered = subscription
                .buffered_stdout_bytes
                .fetch_add(bytes.len(), Ordering::SeqCst)
                + bytes.len();
            let _ = output.send(bytes);
            if buffered > HOST_BUFFER_BYTES {
                subscription.cancel();
            }
        }
        Some("exec.stdin-ready") => {
            let _ = input_ready.send(true);
        }
        Some("exec.exit" | "disconnected") => {
            subscription.end();
            let _ = input_ready.send(false);
        }
        _ => {}
    }
}

/// 원격 프로세스의 stdin. 놓으면 stdin 끝을 알린다
pub struct HostProcessInput {
    subscription: Arc<ProcessSubscription>,
    input_ready: Receiver<bool>,
}

impl Write for HostProcessInput {
    /// 한 조각을 보내고 daemon이 받았다고 알릴 때까지 기다린다
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let closed = || io::Error::new(io::ErrorKind::BrokenPipe, "Host command stdin closed");
        if self.subscription.has_ended.load(Ordering::SeqCst) {
            return Err(closed());
        }
        let chunk = &bytes[..bytes.len().min(HOST_CHUNK_BYTES)];
        let id = &self.subscription.id;
        self.subscription
            .connection
            .send(json!({ "type": "exec.stdin", "id": id, "data": encode_host_bytes(chunk) }))
            .map_err(|_| closed())?;
        match self.input_ready.recv() {
            Ok(true) => Ok(chunk.len()),
            _ => Err(closed()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for HostProcessInput {
    fn drop(&mut self) {
        if !self.subscription.has_ended.load(Ordering::SeqCst) {
            let id = &self.subscription.id;
            let _ = self
                .subscription
                .connection
                .send(json!({ "type": "exec.stdin", "id": id, "data": "", "end": true }));
        }
    }
}

/// 원격 프로세스의 stdout. 프로세스가 끝나고 남은 조각을 다 읽으면 EOF다
pub struct HostProcessOutput {
    subscription: Arc<ProcessSubscription>,
    output: Receiver<Vec<u8>>,
    unread: Vec<u8>,
    unread_offset: usize,
}

impl Read for HostProcessOutput {
    /// 조각을 기다린다. 보내는 쪽(구독)은 프로세스가 끝나 구독을 풀 때 함께 버려지므로, 남은 조각을 다 읽은 뒤 EOF가 된다
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.unread_offset == self.unread.len() {
            let Ok(chunk) = self.output.recv() else {
                return Ok(0);
            };
            self.subscription
                .buffered_stdout_bytes
                .fetch_sub(chunk.len(), Ordering::SeqCst);
            self.unread = chunk;
            self.unread_offset = 0;
        }
        let available = &self.unread[self.unread_offset..];
        let length = available.len().min(buffer.len());
        buffer[..length].copy_from_slice(&available[..length]);
        self.unread_offset += length;
        Ok(length)
    }
}

/// 원격 프로세스 취소(TS `child.kill()`). 이미 끝났으면 아무것도 보내지 않는다
pub struct HostProcessTermination {
    subscription: Arc<ProcessSubscription>,
}

impl HostProcessTermination {
    pub fn terminate(&self) {
        self.subscription.cancel();
    }
}
