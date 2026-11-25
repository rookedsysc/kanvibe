//! `socket.*` 레코드로 원격 Unix 소켓을 데스크탑에 이어 준다(TS `openForward`).
//!
//! 본인 소유의 실제 소켓에만 붙고, 포워딩을 닫아도 그 소켓을 듣는 서버(Herdr 등)의 수명은 건드리지 않는다.
//! 데스크탑이 보낸 조각은 소켓에 다 쓴 뒤 `socket.data-ready`로 알리고, 읽은 바이트는 32 KiB 조각으로 보낸다.

use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::peer::PeerRequests;
use crate::outbox::{RecordOutbox, STREAM_RESUME_BYTES};
use crate::protocol::{
    HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, HostRecord, current_uid, decode_host_bytes,
    encode_host_bytes, record_from, record_text,
};

enum ForwardWrite {
    Data(Vec<u8>),
    End,
}

pub(super) struct SocketForward {
    stream: UnixStream,
    writes: Mutex<Option<Sender<ForwardWrite>>>,
    queued_bytes: AtomicUsize,
    is_destroyed: AtomicBool,
}

impl SocketForward {
    /// 데스크탑 쪽에서 닫았다. 읽기 스레드가 끝나며 `socket.close`를 보낸다
    pub(super) fn destroy(&self) {
        self.is_destroyed.store(true, Ordering::SeqCst);
        self.writes.lock().unwrap().take();
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

struct ForwardContext {
    outbox: Arc<RecordOutbox>,
    requests: Arc<Mutex<PeerRequests>>,
    id: String,
}

impl ForwardContext {
    fn send(&self, kind: &str) {
        let _ = self
            .outbox
            .send(&record_from(json!({ "type": kind, "id": self.id })));
    }

    fn send_error(&self, error: &str) {
        let _ = self.outbox.send(&record_from(
            json!({ "type": "socket.error", "id": self.id, "error": error }),
        ));
    }
}

/// 포워딩을 연다. 열지 못하면 `socket.error`로 알린다(연결 전체를 닫지는 않는다)
pub(super) fn open_forward(
    outbox: &Arc<RecordOutbox>,
    requests: &Arc<Mutex<PeerRequests>>,
    record: &HostRecord,
    id: &str,
) {
    let context = ForwardContext {
        outbox: Arc::clone(outbox),
        requests: Arc::clone(requests),
        id: id.to_string(),
    };
    let path = record_text(record, "path").filter(|path| Path::new(path).is_absolute());
    let Some(path) = path.filter(|_| requests.lock().unwrap().has_room_for(id)) else {
        context.send_error("Invalid host socket request");
        return;
    };
    if let Err(error) = assert_owned_socket(Path::new(path)) {
        context.send_error(&error);
        return;
    }
    match UnixStream::connect(path) {
        Ok(stream) => start_forward(context, stream),
        Err(error) => {
            context.send_error(&error.to_string());
            context.send("socket.close");
        }
    }
}

/// 연결한 소켓을 등록하고 읽기·쓰기 스레드를 띄운다
fn start_forward(context: ForwardContext, stream: UnixStream) {
    let (Ok(reader), Ok(writer)) = (stream.try_clone(), stream.try_clone()) else {
        context.send_error("Host socket could not be shared");
        return;
    };
    let (sender, receiver) = channel();
    let forward = Arc::new(SocketForward {
        stream,
        writes: Mutex::new(Some(sender)),
        queued_bytes: AtomicUsize::new(0),
        is_destroyed: AtomicBool::new(false),
    });
    context
        .requests
        .lock()
        .unwrap()
        .forwards
        .insert(context.id.clone(), Arc::clone(&forward));
    context.send("socket.ready");
    let writer_context = ForwardContext {
        outbox: Arc::clone(&context.outbox),
        requests: Arc::clone(&context.requests),
        id: context.id.clone(),
    };
    let writer_forward = Arc::clone(&forward);
    std::thread::spawn(move || write_upstream(&writer_context, &writer_forward, writer, receiver));
    std::thread::spawn(move || read_upstream(&context, &forward, reader));
}

fn assert_owned_socket(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_socket() || metadata.uid() != current_uid() {
        return Err("Untrusted upstream socket".into());
    }
    Ok(())
}

fn read_upstream(context: &ForwardContext, forward: &SocketForward, mut reader: UnixStream) {
    let mut buffer = vec![0; HOST_CHUNK_BYTES];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => {
                if !forward.is_destroyed.load(Ordering::SeqCst) {
                    context.send("socket.end");
                }
                break;
            }
            Ok(count) => {
                let record = json!({ "type": "socket.data", "id": context.id, "data": encode_host_bytes(&buffer[..count]) });
                let _ = context.outbox.send(&record_from(record));
                context.outbox.wait_until_drained(STREAM_RESUME_BYTES);
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                if !forward.is_destroyed.load(Ordering::SeqCst) {
                    context.send_error(&error.to_string());
                }
                break;
            }
        }
    }
    forward.destroy();
    context
        .requests
        .lock()
        .unwrap()
        .forwards
        .remove(&context.id);
    context.send("socket.close");
}

fn write_upstream(
    context: &ForwardContext,
    forward: &SocketForward,
    mut writer: UnixStream,
    writes: Receiver<ForwardWrite>,
) {
    while let Ok(write) = writes.recv() {
        let bytes = match write {
            ForwardWrite::Data(bytes) => bytes,
            ForwardWrite::End => {
                let _ = writer.shutdown(Shutdown::Write);
                continue;
            }
        };
        let is_written = writer.write_all(&bytes).is_ok();
        forward
            .queued_bytes
            .fetch_sub(bytes.len(), Ordering::SeqCst);
        if !is_written {
            forward.destroy();
            return;
        }
        if context
            .requests
            .lock()
            .unwrap()
            .forwards
            .contains_key(&context.id)
        {
            context.send("socket.data-ready");
        }
    }
}

/// 데스크탑이 보낸 `socket.data`·`socket.end`·`socket.close`
pub(super) fn receive(
    requests: &Arc<Mutex<PeerRequests>>,
    record: &HostRecord,
    kind: &str,
    id: &str,
) -> Result<(), String> {
    let Some(forward) = requests.lock().unwrap().forwards.get(id).cloned() else {
        return Ok(());
    };
    match kind {
        "socket.data" => {
            let bytes = decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES)
                .map_err(|error| error.to_string())?;
            let queued = forward
                .queued_bytes
                .fetch_add(bytes.len(), Ordering::SeqCst)
                + bytes.len();
            if queued > HOST_BUFFER_BYTES {
                forward.destroy();
            } else if let Some(writes) = forward.writes.lock().unwrap().as_ref() {
                let _ = writes.send(ForwardWrite::Data(bytes));
            }
        }
        "socket.end" => {
            if let Some(writes) = forward.writes.lock().unwrap().as_ref() {
                let _ = writes.send(ForwardWrite::End);
            }
        }
        _ => forward.destroy(),
    }
    Ok(())
}
