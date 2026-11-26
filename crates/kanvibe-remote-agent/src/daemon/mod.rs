//! 원격 host daemon 본체(TS `startRemoteHostServer`).
//!
//! namespace마다 하나만 뜨고, SSH 연결과 따로 살아남아 PTY 세션을 유지한다. 데스크탑은 bridge를 거쳐
//! `rpc-v1.sock`에 붙고, 원격 hook·CLI는 `hooks-v1.sock`에 HTTP로 요청한다. 두 소켓은 0600이다.

pub mod bridge;
mod exec;
mod forward;
mod hook_ingress;
mod paths;
mod peer;
pub mod pty;
pub mod pty_launcher;
mod screen;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use serde_json::json;

use paths::{StartLock, remove_stale_socket};
pub use paths::{assert_private_directory, is_process_alive, prepare_paths};
use peer::Peer;
use pty::PtyRegistry;
use pty_launcher::PtyLauncher;

use crate::protocol::{
    HOST_PROTOCOL_VERSION, HostRecord, HostSocketPaths, current_uid, record_from,
};

const ACCEPT_POLL_MILLISECONDS: i32 = 100;

pub struct HostServerOptions {
    pub namespace: String,
    pub build_hash: String,
    /// 없으면 `/tmp/kv-<uid>`. 테스트는 임시 디렉터리를 준다
    pub root_directory: Option<PathBuf>,
    pub pty_launcher: Arc<dyn PtyLauncher>,
}

pub(crate) struct ServerShared {
    namespace: String,
    build_hash: String,
    identity: HostRecord,
    peers: Mutex<HashMap<String, Arc<Peer>>>,
    ptys: Arc<PtyRegistry>,
    is_closed: AtomicBool,
    stop_requested: Mutex<bool>,
    stop_changed: Condvar,
}

impl ServerShared {
    /// `stop` 레코드를 받았다. 실행 파일의 주 스레드가 서버를 닫고 끝낸다
    fn request_stop(&self) {
        *self.stop_requested.lock().unwrap() = true;
        self.stop_changed.notify_all();
    }
}

pub struct HostServer {
    pub paths: HostSocketPaths,
    pub instance_id: String,
    shared: Arc<ServerShared>,
    lock: StartLock,
    socket_inodes: Vec<(PathBuf, u64)>,
}

fn is_valid_build_hash(build_hash: &str) -> bool {
    build_hash.len() == 64
        && build_hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

impl HostServer {
    pub fn start(options: HostServerOptions) -> Result<Self, String> {
        if !is_valid_build_hash(&options.build_hash) {
            return Err("Unsupported host runtime or configuration".into());
        }
        let paths = prepare_paths(&options.namespace, options.root_directory.as_deref())?;
        let lock = StartLock::acquire(Path::new(&paths.directory))?;
        let instance_id = uuid::Uuid::new_v4().to_string();
        let shared = Arc::new(ServerShared {
            identity: build_identity(&options, &paths, &instance_id),
            namespace: options.namespace,
            build_hash: options.build_hash,
            peers: Mutex::new(HashMap::new()),
            ptys: PtyRegistry::new(options.pty_launcher),
            is_closed: AtomicBool::new(false),
            stop_requested: Mutex::new(false),
            stop_changed: Condvar::new(),
        });
        let listeners = bind_sockets(&paths).inspect_err(|_| lock.release())?;
        let socket_inodes = listeners
            .iter()
            .map(|(path, _, inode)| (path.clone(), *inode))
            .collect();
        for (index, (_, listener, _)) in listeners.into_iter().enumerate() {
            let shared = Arc::clone(&shared);
            let is_hook = index == 1;
            std::thread::spawn(move || accept_until_closed(&shared, &listener, is_hook));
        }
        Ok(Self {
            paths,
            instance_id,
            shared,
            lock,
            socket_inodes,
        })
    }

    /// 데스크탑이 `stop`을 보낼 때까지 기다린다
    pub fn wait_for_stop_request(&self) {
        let requested = self.shared.stop_requested.lock().unwrap();
        let _requested = self
            .shared
            .stop_changed
            .wait_while(requested, |requested| !*requested)
            .unwrap();
    }

    /// PTY를 끝내고 소켓을 닫은 뒤 시작 잠금을 푼다
    pub fn close(&self) {
        if self.shared.is_closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.shared.ptys.close();
        for (path, inode) in &self.socket_inodes {
            if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.ino() == *inode) {
                let _ = fs::remove_file(path);
            }
        }
        let peers: Vec<Arc<Peer>> = self
            .shared
            .peers
            .lock()
            .unwrap()
            .drain()
            .map(|(_, peer)| peer)
            .collect();
        for peer in peers {
            peer.outbox.abort();
        }
        self.lock.release();
        self.shared.request_stop();
    }
}

impl Drop for HostServer {
    fn drop(&mut self) {
        self.close();
    }
}

fn build_identity(
    options: &HostServerOptions,
    paths: &HostSocketPaths,
    instance_id: &str,
) -> HostRecord {
    record_from(json!({
        "type": "ready",
        "version": HOST_PROTOCOL_VERSION,
        "namespace": options.namespace,
        "instanceId": instance_id,
        "pid": std::process::id(),
        "uid": current_uid(),
        "rpcSocketPath": paths.rpc_socket_path,
        "hookSocketPath": paths.hook_socket_path,
        "buildHash": options.build_hash,
        "runtime": { "implementation": "rust", "platform": std::env::consts::OS, "arch": std::env::consts::ARCH },
        "capabilities": ["exec", "socket", "hooks", "pty"],
    }))
}

/// 남은 옛 소켓을 정리하고 rpc·hook 소켓을 0600으로 연다
fn bind_sockets(paths: &HostSocketPaths) -> Result<Vec<(PathBuf, UnixListener, u64)>, String> {
    let mut listeners = Vec::new();
    for socket_path in [&paths.rpc_socket_path, &paths.hook_socket_path] {
        let socket_path = PathBuf::from(socket_path);
        remove_stale_socket(&socket_path)?;
        let listener = UnixListener::bind(&socket_path).map_err(|error| error.to_string())?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
        let inode = fs::symlink_metadata(&socket_path)
            .map_err(|error| error.to_string())?
            .ino();
        listeners.push((socket_path, listener, inode));
    }
    Ok(listeners)
}

fn accept_until_closed(shared: &Arc<ServerShared>, listener: &UnixListener, is_hook: bool) {
    while !shared.is_closed.load(Ordering::SeqCst) {
        if !is_readable(listener.as_raw_fd()) {
            continue;
        }
        let Ok((stream, _)) = listener.accept() else {
            continue;
        };
        spawn_connection(shared, stream, is_hook);
    }
}

fn spawn_connection(shared: &Arc<ServerShared>, stream: UnixStream, is_hook: bool) {
    let shared = Arc::clone(shared);
    std::thread::spawn(move || match is_hook {
        true => hook_ingress::serve_hook_connection(shared, stream),
        false => peer::serve_connection(shared, stream),
    });
}

/// 닫힘 표시를 주기적으로 보도록 accept 전에 잠깐만 기다린다
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
