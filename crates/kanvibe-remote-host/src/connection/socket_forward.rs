//! 원격 Unix 소켓을 로컬 소켓으로 이어 준다(TS `HostConnection.forwardSocket`).
//!
//! 로컬 소켓은 `/tmp/kvhost-XXXXXX/upstream.sock`(0700 디렉터리)이고, 로컬 연결마다 daemon에 `socket.open`을 보낸다.
//! 로컬에서 읽은 조각은 제한된 범위 안에서 `socket.data-ready`를 기다리지 않고 이어 보낸다. 포워딩을 닫아도 원격 서버는 남는다.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use kanvibe_remote_agent::protocol::{
    HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, HostRecord, decode_host_bytes, encode_host_bytes,
    record_text,
};
use kanvibe_terminal::terminal_registry::RemoteHostSocketForward;
use serde_json::json;

use super::{HostConnection, REMOTE_INPUT_WINDOW_CHUNKS};

const ACCEPT_POLL_MILLISECONDS: i32 = 100;
const SOCKET_FILE_NAME: &str = "upstream.sock";

/// 열려 있는 로컬 연결(연결 id별). 포워딩을 닫을 때 모두 끊고, 연결 하나가 끝나면 그 연결만 뺀다
type OpenLocalStreams = Arc<Mutex<HashMap<String, UnixStream>>>;

pub struct HostSocketForward {
    /// 값을 버리면 디렉터리와 소켓 파일이 함께 지워진다
    directory: tempfile::TempDir,
    is_closed: Arc<AtomicBool>,
    streams: OpenLocalStreams,
}

impl HostConnection {
    pub fn forward_socket(
        self: &Arc<Self>,
        remote_path: &str,
    ) -> Result<Arc<HostSocketForward>, String> {
        let directory = tempfile::Builder::new()
            .prefix("kvhost-")
            .tempdir_in("/tmp")
            .map_err(|error| error.to_string())?;
        set_private(&directory)?;
        let socket_path = directory.path().join(SOCKET_FILE_NAME);
        let listener = UnixListener::bind(&socket_path).map_err(|error| error.to_string())?;
        let forward = Arc::new(HostSocketForward {
            directory,
            is_closed: Arc::new(AtomicBool::new(false)),
            streams: Arc::new(Mutex::new(HashMap::new())),
        });
        let (connection, remote_path) = (Arc::clone(self), remote_path.to_string());
        let (is_closed, streams) = (Arc::clone(&forward.is_closed), Arc::clone(&forward.streams));
        std::thread::spawn(move || {
            while !is_closed.load(Ordering::SeqCst) && !connection.is_closed() {
                if !is_readable(listener.as_raw_fd()) {
                    continue;
                }
                if let Ok((stream, _)) = listener.accept() {
                    open_remote_stream(&connection, &remote_path, stream, &streams);
                }
            }
        });
        Ok(forward)
    }
}

fn set_private(directory: &tempfile::TempDir) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())
}

fn is_readable(file_descriptor: i32) -> bool {
    let mut poll = libc::pollfd {
        fd: file_descriptor,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: 살아 있는 리스너 fd 하나만 넘긴다
    let ready = unsafe { libc::poll(&mut poll, 1, ACCEPT_POLL_MILLISECONDS) };
    ready > 0 && poll.revents & libc::POLLIN != 0
}

enum RemoteEvent {
    Ready,
    DataReady,
    Closed,
}

/// 로컬 연결 하나를 원격 소켓 하나에 잇는다
fn open_remote_stream(
    connection: &Arc<HostConnection>,
    remote_path: &str,
    stream: UnixStream,
    streams: &OpenLocalStreams,
) {
    let id = uuid::Uuid::new_v4().to_string();
    let (Ok(writer), Ok(reader), Ok(tracked)) =
        (stream.try_clone(), stream.try_clone(), stream.try_clone())
    else {
        return;
    };
    streams.lock().unwrap().insert(id.clone(), tracked);
    let (events, event_receiver) = channel();
    let Some(writer) = LocalSocketWriter::start(writer) else {
        streams.lock().unwrap().remove(&id);
        return;
    };
    let receiver =
        Arc::new(move |record: &HostRecord| deliver_remote_record(record, &writer, &events));
    if connection.subscribe(&id, receiver).is_err() {
        streams.lock().unwrap().remove(&id);
        let _ = stream.shutdown(Shutdown::Both);
        return;
    }
    if connection
        .send(json!({ "type": "socket.open", "id": id, "path": remote_path }))
        .is_err()
    {
        connection.unsubscribe(&id);
        streams.lock().unwrap().remove(&id);
        let _ = stream.shutdown(Shutdown::Both);
        return;
    }
    let (connection, streams) = (Arc::clone(connection), Arc::clone(streams));
    std::thread::spawn(move || {
        pump_local_to_remote(&connection, &id, reader, &event_receiver);
        connection.unsubscribe(&id);
        let _ = connection.send(json!({ "type": "socket.close", "id": id }));
        streams.lock().unwrap().remove(&id);
        let _ = stream.shutdown(Shutdown::Both);
    });
}

enum LocalWrite {
    Data(Vec<u8>),
    End,
}

/// 원격에서 온 바이트를 로컬 소켓에 쓰는 쪽. 연결 수신 스레드는 호스트 연결마다 하나뿐이라 로컬 쪽이 읽지 않을 때
/// 그 스레드가 막히면 같은 호스트의 PTY 출력·명령 결과·ping까지 멈춘다. 그래서 수신 스레드는 큐에 넣기만 하고
/// 쓰기는 로컬 연결마다 둔 스레드가 맡는다. 쌓인 바이트가 상한을 넘으면 그 로컬 연결만 끊는다(TS `socket.destroy()`)
struct LocalSocketWriter {
    stream: UnixStream,
    queue: Sender<LocalWrite>,
    queued_bytes: Arc<AtomicUsize>,
}

impl LocalSocketWriter {
    fn start(stream: UnixStream) -> Option<Self> {
        let mut output = stream.try_clone().ok()?;
        let (queue, writes) = channel::<LocalWrite>();
        let queued_bytes = Arc::new(AtomicUsize::new(0));
        let written_bytes = Arc::clone(&queued_bytes);
        std::thread::spawn(move || {
            for write in writes {
                match write {
                    LocalWrite::Data(bytes) => {
                        let result = output.write_all(&bytes);
                        written_bytes.fetch_sub(bytes.len(), Ordering::SeqCst);
                        if result.is_err() {
                            let _ = output.shutdown(Shutdown::Both);
                            return;
                        }
                    }
                    LocalWrite::End => drop(output.shutdown(Shutdown::Write)),
                }
            }
        });
        Some(Self {
            stream,
            queue,
            queued_bytes,
        })
    }

    fn enqueue(&self, bytes: Vec<u8>) {
        let queued = self.queued_bytes.fetch_add(bytes.len(), Ordering::SeqCst) + bytes.len();
        if queued > HOST_BUFFER_BYTES || self.queue.send(LocalWrite::Data(bytes)).is_err() {
            self.destroy();
        }
    }

    /// 앞서 넣은 바이트를 다 쓴 뒤 쓰기 방향을 닫는다
    fn end(&self) {
        let _ = self.queue.send(LocalWrite::End);
    }

    fn destroy(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

/// daemon이 보낸 레코드를 로컬 소켓에 반영한다. 연결 수신 스레드에서 불리므로 쓰기는 큐에 넣기만 한다
fn deliver_remote_record(
    record: &HostRecord,
    writer: &LocalSocketWriter,
    events: &Sender<RemoteEvent>,
) {
    match record_text(record, "type").unwrap_or_default() {
        "socket.ready" => drop(events.send(RemoteEvent::Ready)),
        "socket.data-ready" => drop(events.send(RemoteEvent::DataReady)),
        "socket.data" => match decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES) {
            Ok(bytes) => writer.enqueue(bytes),
            Err(_) => writer.destroy(),
        },
        "socket.end" => writer.end(),
        "socket.error" | "socket.close" | "disconnected" => {
            writer.destroy();
            let _ = events.send(RemoteEvent::Closed);
        }
        _ => {}
    }
}

fn pump_local_to_remote(
    connection: &HostConnection,
    id: &str,
    mut reader: UnixStream,
    events: &Receiver<RemoteEvent>,
) {
    if !matches!(events.recv(), Ok(RemoteEvent::Ready)) {
        return;
    }
    let mut buffer = vec![0; HOST_CHUNK_BYTES];
    let mut chunks_in_flight = 0;
    loop {
        if chunks_in_flight == REMOTE_INPUT_WINDOW_CHUNKS {
            if !matches!(events.recv(), Ok(RemoteEvent::DataReady)) {
                return;
            }
            chunks_in_flight -= 1;
        }
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        if connection.send(json!({ "type": "socket.data", "id": id, "data": encode_host_bytes(&buffer[..count]) })).is_err() {
            return;
        }
        chunks_in_flight += 1;
    }
    for _ in 0..chunks_in_flight {
        if !matches!(events.recv(), Ok(RemoteEvent::DataReady)) {
            return;
        }
    }
    let _ = connection.send(json!({ "type": "socket.end", "id": id }));
    let _ = events.recv();
}

impl HostSocketForward {
    pub fn path(&self) -> PathBuf {
        self.directory.path().join(SOCKET_FILE_NAME)
    }

    #[cfg(test)]
    pub(super) fn open_local_stream_count(&self) -> usize {
        self.streams.lock().unwrap().len()
    }

    fn shut_down(&self) {
        if self.is_closed.swap(true, Ordering::SeqCst) {
            return;
        }
        for (_, stream) in self.streams.lock().unwrap().drain() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let _ = std::fs::remove_file(self.path());
    }
}

impl RemoteHostSocketForward for HostSocketForward {
    fn socket_path(&self) -> String {
        self.path().to_string_lossy().into_owned()
    }

    fn close(&self) -> Result<(), String> {
        self.shut_down();
        Ok(())
    }
}

impl Drop for HostSocketForward {
    fn drop(&mut self) {
        self.shut_down();
    }
}
