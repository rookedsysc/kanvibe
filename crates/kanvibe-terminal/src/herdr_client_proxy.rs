//! Herdr 클라이언트(TUI)와 Herdr 서버 사이의 Unix 소켓 프록시(TS `src/lib/herdrClientProxy.ts`).
//!
//! TUI는 서버 소켓 대신 이 프록시의 소켓에 붙는다. 프록시는 프레임을 원문 그대로 중계하면서 Herdr endpoint
//! generation 1의 bincode 프레이밍을 읽어, 이 태스크 workspace의 활성 pane·tab과 TUI surface 크기를 따라가고
//! `pane.focus`·`tab.focus`·`workspace.focus` 요청을 끼워 넣는다. 서버 boot가 바뀔 때마다 이 연결의 workspace를
//! 먼저 고르고, 그 전에 들어온 다른 task의 pane 입력은 버린다.
//!
//! TUI 연결은 한 번에 하나다. 새 TUI가 붙으면 이전 연결을 닫는다.

mod codec;
mod connection;
mod outbound;

#[cfg(test)]
mod tests;

use std::ffi::{CString, OsString};
use std::fs::{self, Permissions};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use serde_json::Value;

use crate::environment::TerminalHostEnvironment;
use crate::terminal_registry::{HerdrClientRelay, HerdrClientRelayFactory};
use connection::HerdrEndpointConnection;

const NOT_CONNECTED_MESSAGE: &str = "Herdr 클라이언트가 아직 연결되지 않았습니다.";

/// TUI가 알린 surface 크기(셀 단위)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HerdrSurfaceSize {
    pub cols: u32,
    pub rows: u32,
}

/// 지금 붙어 있는 TUI 연결과, 새 연결을 더 받는지
struct ConnectionSlot {
    connection: Option<Arc<HerdrEndpointConnection>>,
    is_accepting: bool,
}

pub struct HerdrClientProxy {
    socket_path: PathBuf,
    directory: PathBuf,
    slot: Arc<Mutex<ConnectionSlot>>,
    accept_thread: Mutex<Option<JoinHandle<()>>>,
    is_closed: Mutex<bool>,
}

impl HerdrClientProxy {
    /// `temp_root` 아래에 `kvhc-XXXXXX` 디렉터리(0700)를 만들고 그 안의 `client` 소켓(0600)에서 TUI를 받는다.
    /// `temp_root`는 TS `os.tmpdir()`에 해당하며, 소켓 경로 길이 제한 때문에 짧은 경로여야 한다
    pub fn create(
        upstream_socket_path: &Path,
        workspace_id: &str,
        temp_root: &Path,
    ) -> io::Result<Self> {
        let directory = make_private_temp_directory(temp_root)?;
        let socket_path = directory.join("client");
        let listener = match bind_private_socket(&socket_path) {
            Ok(listener) => listener,
            Err(error) => {
                let _ = fs::remove_dir_all(&directory);
                return Err(error);
            }
        };
        let slot = Arc::new(Mutex::new(ConnectionSlot {
            connection: None,
            is_accepting: true,
        }));
        let accept_slot = Arc::clone(&slot);
        let upstream_socket_path = upstream_socket_path.to_path_buf();
        let workspace_id = workspace_id.to_string();
        let accept_thread = thread::spawn(move || {
            accept_clients(listener, &accept_slot, &upstream_socket_path, &workspace_id);
        });
        Ok(Self {
            socket_path,
            directory,
            slot,
            accept_thread: Mutex::new(Some(accept_thread)),
            is_closed: Mutex::new(false),
        })
    }

    /// TUI가 붙을 소켓 경로
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// 이 workspace 안에서 TUI가 보고 있는 tab. 다른 workspace를 보고 있으면 `None`이다
    pub fn active_tab_id(&self) -> Option<String> {
        self.current_connection()?.active_tab_id()
    }

    /// 이 workspace 안에서 TUI가 보고 있는 pane. 다른 workspace를 보고 있으면 `None`이다
    pub fn active_pane_id(&self) -> Option<String> {
        self.current_connection()?.active_pane_id()
    }

    /// 초기화를 마친 TUI가 surface를 보이고 있을 때만 그 크기를 준다
    pub fn surface_size(&self) -> Option<HerdrSurfaceSize> {
        self.current_connection()?.surface_size()
    }

    /// endpoint 요청을 보내고 응답을 기다린다. 5초 안에 답이 없으면 실패한다.
    /// 성공하면 응답의 `result`를 준다(TS는 버린다)
    pub fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        match self.current_connection() {
            Some(connection) => connection.request(method, params),
            None => Err(NOT_CONNECTED_MESSAGE.to_string()),
        }
    }

    /// 연결을 닫고 더 받지 않으며 소켓 디렉터리를 지운다. 여러 번 불러도 한 번만 정리한다
    pub fn close(&self) {
        let mut is_closed = self
            .is_closed
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *is_closed {
            return;
        }
        *is_closed = true;
        let connection = {
            let mut slot = lock_slot(&self.slot);
            slot.is_accepting = false;
            slot.connection.clone()
        };
        if let Some(connection) = connection {
            connection.destroy(None);
        }
        self.stop_accepting();
        let _ = fs::remove_dir_all(&self.directory);
    }

    /// 막혀 있는 accept를 깨우려고 자기 소켓에 한 번 붙는다. 붙지 못하면 스레드가 이미 끝났거나 깨울 수 없으므로 기다리지 않는다
    fn stop_accepting(&self) {
        let accept_thread = self
            .accept_thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(accept_thread) = accept_thread else {
            return;
        };
        if UnixStream::connect(&self.socket_path).is_ok() {
            let _ = accept_thread.join();
        }
    }

    fn current_connection(&self) -> Option<Arc<HerdrEndpointConnection>> {
        lock_slot(&self.slot).connection.clone()
    }
}

impl Drop for HerdrClientProxy {
    fn drop(&mut self) {
        self.close();
    }
}

impl HerdrClientRelay for HerdrClientProxy {
    fn socket_path(&self) -> String {
        self.socket_path.to_string_lossy().into_owned()
    }

    fn active_pane_id(&self) -> Option<String> {
        HerdrClientProxy::active_pane_id(self)
    }

    fn active_tab_id(&self) -> Option<String> {
        HerdrClientProxy::active_tab_id(self)
    }

    /// 터미널 크기 축은 u16이다. 그 범위를 넘는 크기는 투영하지 않는다
    fn surface_size(&self) -> Option<(u16, u16)> {
        let surface_size = HerdrClientProxy::surface_size(self)?;
        let cols = u16::try_from(surface_size.cols).ok()?;
        let rows = u16::try_from(surface_size.rows).ok()?;
        Some((cols, rows))
    }

    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        HerdrClientProxy::request(self, method, params)
    }

    fn close(&self) -> Result<(), String> {
        HerdrClientProxy::close(self);
        Ok(())
    }
}

/// 실제 Unix 소켓 프록시를 만드는 팩토리. 임시 디렉터리 위치(TS `os.tmpdir()`)를 만들 때 한 번 정해 둔다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeHerdrClientRelayFactory {
    temp_root: PathBuf,
}

impl NativeHerdrClientRelayFactory {
    /// Node `os.tmpdir()`처럼 `TMPDIR`, `TMP`, `TEMP`, `/tmp` 순으로 고르고 끝의 `/` 하나를 뗀다
    pub fn from_host_environment(environment: &TerminalHostEnvironment) -> Self {
        let configured = ["TMPDIR", "TMP", "TEMP"]
            .into_iter()
            .find_map(|key| environment.variable(key))
            .unwrap_or("/tmp");
        let trimmed = match configured.strip_suffix('/') {
            Some(stripped) if configured.len() > 1 => stripped,
            _ => configured,
        };
        Self {
            temp_root: PathBuf::from(trimmed),
        }
    }
}

impl HerdrClientRelayFactory for NativeHerdrClientRelayFactory {
    fn create(
        &self,
        upstream_socket_path: &str,
        workspace_id: &str,
    ) -> Result<Arc<dyn HerdrClientRelay>, String> {
        let proxy = HerdrClientProxy::create(
            Path::new(upstream_socket_path),
            workspace_id,
            &self.temp_root,
        )
        .map_err(|error| error.to_string())?;
        Ok(Arc::new(proxy))
    }
}

fn accept_clients(
    listener: UnixListener,
    slot: &Mutex<ConnectionSlot>,
    upstream_socket_path: &Path,
    workspace_id: &str,
) {
    for incoming in listener.incoming() {
        let mut slot = lock_slot(slot);
        if !slot.is_accepting {
            return;
        }
        let Ok(client) = incoming else {
            continue;
        };
        if let Some(previous) = slot.connection.take() {
            previous.destroy(None);
        }
        slot.connection = Some(HerdrEndpointConnection::start(
            client,
            upstream_socket_path,
            workspace_id,
        ));
    }
}

fn lock_slot(slot: &Mutex<ConnectionSlot>) -> MutexGuard<'_, ConnectionSlot> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Node `fs.mkdtemp`와 같은 `mkdtemp(3)`. 이름 끝 여섯 글자를 무작위로 채우고 0700으로 만든다
fn make_private_temp_directory(temp_root: &Path) -> io::Result<PathBuf> {
    let template = temp_root.join("kvhc-XXXXXX");
    let mut template_bytes = CString::new(template.as_os_str().as_bytes())?.into_bytes_with_nul();
    // template_bytes는 NUL로 끝나는 쓰기 가능한 버퍼이고 mkdtemp는 그 안의 X만 바꾼다
    let created = unsafe { libc::mkdtemp(template_bytes.as_mut_ptr().cast()) };
    if created.is_null() {
        return Err(io::Error::last_os_error());
    }
    template_bytes.pop();
    Ok(PathBuf::from(OsString::from_vec(template_bytes)))
}

fn bind_private_socket(socket_path: &Path) -> io::Result<UnixListener> {
    let listener = UnixListener::bind(socket_path)?;
    fs::set_permissions(socket_path, Permissions::from_mode(0o600))?;
    Ok(listener)
}
