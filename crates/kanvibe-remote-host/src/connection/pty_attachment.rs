//! daemon PTY attachment 하나를 터미널 레지스트리에 잇는다(TS `remoteHostPtyConnection.ts`).
//!
//! 출력은 받은 순서대로 한 조각씩 화면 콜백에 넘기고, 콜백이 돌아온 뒤에만 그 조각을 `pty.ack`로 알린다.
//! 끝난 프로세스의 마지막 출력은 종료 알림보다 먼저 넘긴다. 입력은 확인 응답을 기다리는 32 KiB 조각 수를 제한하고,
//! 연결이 끊기면 기다리던 입력을 버린다(결과를 모르는 입력은 다시 보내지 않는다). `kill`은 attachment만 떼고
//! 세션은 daemon에 남기며, `close_session`만 세션을 끝낸다.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use kanvibe_remote_agent::protocol::{
    HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, HostRecord, decode_host_bytes, encode_host_bytes,
    record_integer, record_text,
};
use kanvibe_remote_agent::terminal_agent::wire::Utf8StreamDecoder;
use kanvibe_terminal::pty::{PtyEventCallbacks, PtyExit, PtyProcess};
use kanvibe_terminal::terminal_registry::RemoteHostPty;
use serde_json::{Value, json};

use super::REMOTE_INPUT_WINDOW_CHUNKS;

pub type PtySend = Arc<dyn Fn(Value) -> Result<(), String> + Send + Sync>;

const DETACHED_EXIT: PtyExit = PtyExit {
    exit_code: 255,
    signal: None,
    is_detached: true,
};

#[derive(Default)]
struct AttachmentState {
    is_ready: bool,
    session_id: String,
    pid: u32,
    columns: u16,
    rows: u16,
    input: VecDeque<Vec<u8>>,
    input_bytes: usize,
    input_chunks_in_flight: usize,
    output: VecDeque<Vec<u8>>,
    output_bytes: usize,
    pending_exit: Option<PtyExit>,
    finished: Option<PtyExit>,
    open_error: Option<String>,
}

pub struct RemotePtyAttachment {
    id: String,
    send: PtySend,
    on_finished: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    state: Mutex<AttachmentState>,
    changed: Condvar,
}

impl RemotePtyAttachment {
    /// attachment를 만들고 출력 전달 스레드를 띄운다. 호출자는 `pty.open`을 보낸 뒤 [`Self::wait_ready`]를 부른다
    pub fn start(
        id: &str,
        send: PtySend,
        columns: u16,
        rows: u16,
        callbacks: PtyEventCallbacks,
    ) -> Arc<Self> {
        let attachment = Arc::new(Self {
            id: id.to_string(),
            send,
            on_finished: Mutex::new(None),
            state: Mutex::new(AttachmentState {
                columns,
                rows,
                ..AttachmentState::default()
            }),
            changed: Condvar::new(),
        });
        let worker = Arc::clone(&attachment);
        std::thread::spawn(move || worker.deliver_until_finished(callbacks));
        attachment
    }

    /// 끝날 때 한 번 부를 정리(연결의 수신 등록 해제)
    pub fn set_on_finished(&self, callback: Box<dyn FnOnce() + Send>) {
        *self.on_finished.lock().unwrap() = Some(callback);
    }

    pub fn wait_ready(&self, timeout: Duration) -> Result<(), String> {
        let state = self.state.lock().unwrap();
        let (state, wait) = self
            .changed
            .wait_timeout_while(state, timeout, |state| {
                !state.is_ready && state.finished.is_none()
            })
            .unwrap();
        if state.is_ready {
            return Ok(());
        }
        let error = state.open_error.clone();
        drop(state);
        if wait.timed_out() {
            self.finish(
                DETACHED_EXIT,
                Some("PTY attach timed out; session outcome may be unknown".into()),
            );
            return Err("PTY attach timed out; session outcome may be unknown".into());
        }
        Err(error.unwrap_or_else(|| "PTY detached before ready".into()))
    }

    /// 연결이 이 attachment id로 받은 레코드
    pub fn receive(&self, record: &HostRecord) {
        match record_text(record, "type").unwrap_or_default() {
            "pty.ready" => self.accept_ready(record),
            "pty.data" => self.accept_output(record),
            "pty.input-ready" => {
                let mut state = self.state.lock().unwrap();
                if state.input_chunks_in_flight > 0
                    && state.finished.is_none()
                    && let Some(chunk) = state.input.pop_front()
                {
                    state.input_bytes -= chunk.len();
                    state.input_chunks_in_flight -= 1;
                    self.transmit(&mut state);
                }
            }
            "pty.exit" => {
                let mut state = self.state.lock().unwrap();
                let exit_code = record_integer(record, "exitCode").unwrap_or(0) as i32;
                let signal = record_integer(record, "signal").map(|signal| signal as i32);
                state.pending_exit = Some(PtyExit {
                    exit_code,
                    signal,
                    is_detached: false,
                });
                state.input.clear();
                state.input_bytes = 0;
                state.input_chunks_in_flight = 0;
                self.changed.notify_all();
            }
            "pty.error" | "pty.detached" | "disconnected" => {
                let error = record_text(record, "error")
                    .unwrap_or("Host transport disconnected; input outcome may be unknown");
                self.finish(DETACHED_EXIT, Some(error.to_string()));
            }
            _ => {}
        }
    }

    fn accept_ready(&self, record: &HostRecord) {
        let session_id = record_text(record, "sessionId").map(str::to_string);
        let pid = record_integer(record, "pid").filter(|pid| *pid >= 1);
        let (Some(session_id), Some(pid)) = (session_id, pid) else {
            self.finish(DETACHED_EXIT, Some("Invalid PTY identity".into()));
            return;
        };
        let mut state = self.state.lock().unwrap();
        state.session_id = session_id;
        state.pid = pid as u32;
        state.columns = record_integer(record, "cols").unwrap_or(i64::from(state.columns)) as u16;
        state.rows = record_integer(record, "rows").unwrap_or(i64::from(state.rows)) as u16;
        state.is_ready = true;
        self.changed.notify_all();
    }

    fn accept_output(&self, record: &HostRecord) {
        let Ok(bytes) = decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES) else {
            return;
        };
        let mut state = self.state.lock().unwrap();
        if state.finished.is_some() {
            return;
        }
        state.output_bytes += bytes.len();
        if state.output_bytes > HOST_BUFFER_BYTES {
            drop(state);
            self.detach();
            return;
        }
        state.output.push_back(bytes);
        self.changed.notify_all();
    }

    /// 큐 앞에서 ACK를 기다리는 조각은 건너뛰고 보낼 수 있는 만큼 순서대로 보낸다
    fn transmit(&self, state: &mut AttachmentState) {
        while state.finished.is_none()
            && state.input_chunks_in_flight < REMOTE_INPUT_WINDOW_CHUNKS
            && let Some(chunk) = state.input.get(state.input_chunks_in_flight)
        {
            let record =
                json!({ "type": "pty.input", "id": self.id, "data": encode_host_bytes(chunk) });
            if (self.send)(record).is_err() {
                state.finished = Some(DETACHED_EXIT);
                self.changed.notify_all();
                break;
            }
            state.input_chunks_in_flight += 1;
        }
    }

    /// 이 attachment만 떼고 세션은 daemon에 남긴다
    pub fn detach(&self) {
        if self.state.lock().unwrap().finished.is_some() {
            return;
        }
        let _ = (self.send)(json!({ "type": "pty.detach", "id": self.id }));
        self.finish(
            PtyExit {
                exit_code: 0,
                signal: None,
                is_detached: true,
            },
            None,
        );
    }

    /// 끝낸다. 기다리던 입력과 출력은 버리고 종료 알림만 전달 스레드가 보낸다
    /// 열기 전에 실패한 attachment를 끝내 전달 스레드를 거둔다(TS `pty.open` 전송 실패 때의 `finish`)
    pub(crate) fn finish_detached(&self, error: String) {
        self.finish(DETACHED_EXIT, Some(error));
    }

    fn finish(&self, exit: PtyExit, error: Option<String>) {
        let mut state = self.state.lock().unwrap();
        if state.finished.is_some() {
            return;
        }
        state.finished = Some(exit);
        state.open_error = error;
        state.input.clear();
        state.input_bytes = 0;
        state.input_chunks_in_flight = 0;
        state.output.clear();
        state.output_bytes = 0;
        self.changed.notify_all();
    }

    fn deliver_until_finished(&self, callbacks: PtyEventCallbacks) {
        let mut decoder = Utf8StreamDecoder::default();
        loop {
            let next = {
                let state = self.state.lock().unwrap();
                let mut state = self
                    .changed
                    .wait_while(state, |state| {
                        state.finished.is_none()
                            && state.output.is_empty()
                            && state.pending_exit.is_none()
                    })
                    .unwrap();
                if state.finished.is_none() && state.output.is_empty() {
                    state.finished = state.pending_exit;
                }
                match state.finished {
                    Some(exit) => Err(exit),
                    None => Ok(state.output.front().cloned().unwrap_or_default()),
                }
            };
            let chunk = match next {
                Ok(chunk) => chunk,
                Err(exit) => {
                    if let Some(callback) = self.on_finished.lock().unwrap().take() {
                        callback();
                    }
                    (callbacks.on_exit)(exit);
                    return;
                }
            };
            let text = decoder.write(&chunk);
            if !text.is_empty() {
                (callbacks.on_output)(&text);
            }
            self.acknowledge(chunk.len());
        }
    }

    /// 화면에 반영한 조각을 큐에서 빼고 daemon에 ACK한다
    fn acknowledge(&self, bytes: usize) {
        let mut state = self.state.lock().unwrap();
        if state.finished.is_some() {
            return;
        }
        state.output.pop_front();
        state.output_bytes -= bytes;
        if (self.send)(json!({ "type": "pty.ack", "id": self.id, "bytes": bytes })).is_err() {
            state.finished = Some(DETACHED_EXIT);
            self.changed.notify_all();
        }
    }

    pub fn session_identity(&self) -> (String, u32) {
        let state = self.state.lock().unwrap();
        (state.session_id.clone(), state.pid)
    }
}

impl PtyProcess for RemotePtyAttachment {
    fn pid(&self) -> u32 {
        self.state.lock().unwrap().pid
    }

    fn foreground_process_name(&self) -> Option<String> {
        Some("remote-shell".into())
    }

    fn columns(&self) -> u16 {
        self.state.lock().unwrap().columns
    }

    fn rows(&self) -> u16 {
        self.state.lock().unwrap().rows
    }

    fn write(&self, data: &str) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.finished.is_some() || state.pending_exit.is_some() {
            return Err("PTY detached or exited; input not replayed".into());
        }
        if state.input_bytes + data.len() > HOST_BUFFER_BYTES {
            return Err("PTY input backpressure limit".into());
        }
        state.input_bytes += data.len();
        state
            .input
            .extend(data.as_bytes().chunks(HOST_CHUNK_BYTES).map(<[u8]>::to_vec));
        self.transmit(&mut state);
        Ok(())
    }

    fn resize(&self, columns: u16, rows: u16) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.finished.is_some() {
            return Ok(());
        }
        (self.send)(json!({ "type": "pty.resize", "id": self.id, "cols": columns, "rows": rows }))?;
        state.columns = columns;
        state.rows = rows;
        Ok(())
    }

    fn kill(&self) -> Result<(), String> {
        self.detach();
        Ok(())
    }
}

impl RemoteHostPty for RemotePtyAttachment {
    fn session_id(&self) -> String {
        self.state.lock().unwrap().session_id.clone()
    }

    fn close_session(&self) -> Result<(), String> {
        if self.state.lock().unwrap().finished.is_some() {
            return Ok(());
        }
        (self.send)(json!({ "type": "pty.close", "id": self.id }))
    }
}
