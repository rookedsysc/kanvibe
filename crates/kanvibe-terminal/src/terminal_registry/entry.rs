//! 레지스트리에 올라간 PTY 하나와, PTY 읽기 스레드가 출력을 흘려 보내는 자리.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use kanvibe_domain::task::SessionType;

use super::herdr_client::HerdrTaskClient;
use super::{ClientClosure, RemoteHostPty, TerminalClient};
use crate::pty::{PtyExit, PtyProcess};
use crate::screen::TerminalScreen;

/// 클라이언트 쪽에 쌓인 출력이 이 크기를 넘으면 그 클라이언트를 닫는다
const CLIENT_BACKPRESSURE_LIMIT_BYTES: usize = 8 * 1024 * 1024;
const BACKPRESSURE_CLOSE_CODE: u16 = 1013;
const BACKPRESSURE_CLOSE_REASON: &str = "Terminal output backpressure limit";

pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) struct RegisteredClient {
    pub client_id: u64,
    pub client: Arc<dyn TerminalClient>,
}

/// 화면과 클라이언트 목록. 출력 반영과 클라이언트 등록이 같은 잠금을 써서, 붙는 순간의 화면과 그 뒤 출력이 빠지거나 겹치지 않는다
pub(super) struct OutputSink {
    /// terminal 세션에만 있다. 멀티플렉서 세션은 멀티플렉서가 화면을 들고 있다
    pub screen: Option<TerminalScreen>,
    pub clients: Vec<RegisteredClient>,
}

impl OutputSink {
    /// 이미 붙은 클라이언트면 그 식별자를 돌려준다(TS `Set`에 같은 WebSocket을 두 번 넣는 것과 같다)
    pub fn register(
        &mut self,
        client: &Arc<dyn TerminalClient>,
        next_client_id: impl FnOnce() -> u64,
    ) -> u64 {
        if let Some(registered) = self
            .clients
            .iter()
            .find(|registered| Arc::ptr_eq(&registered.client, client))
        {
            return registered.client_id;
        }
        let client_id = next_client_id();
        self.clients.push(RegisteredClient {
            client_id,
            client: Arc::clone(client),
        });
        client_id
    }
}

/// PTY가 레지스트리에 오르기 전에 끝난 경우를 놓치지 않으려고 종료와 등록을 같은 잠금으로 맞춘다
#[derive(Default)]
struct ExitWatch {
    is_registered: bool,
    exit_before_registration: Option<PtyExit>,
}

/// PTY 콜백과 레지스트리 항목이 함께 쓰는 부분. PTY를 띄우기 전에 만들어 첫 출력부터 받는다
pub(super) struct EntryOutput {
    pub generation: u64,
    pub sink: Mutex<OutputSink>,
    exit_watch: Mutex<ExitWatch>,
}

impl EntryOutput {
    pub fn new(
        generation: u64,
        screen: Option<TerminalScreen>,
        first_client: Option<RegisteredClient>,
    ) -> Arc<Self> {
        Arc::new(Self {
            generation,
            sink: Mutex::new(OutputSink {
                screen,
                clients: first_client.into_iter().collect(),
            }),
            exit_watch: Mutex::new(ExitWatch::default()),
        })
    }

    /// PTY 출력을 화면에 반영하고 열린 클라이언트마다 보낸다. 밀린 클라이언트는 보내지 않고 1013으로 닫는다
    pub fn deliver(&self, data: &str) {
        let mut overloaded_clients = Vec::new();
        {
            let mut sink = lock(&self.sink);
            if let Some(screen) = sink.screen.as_mut() {
                screen.write(data.as_bytes());
            }
            for registered in sink
                .clients
                .iter()
                .filter(|registered| registered.client.is_open())
            {
                if registered.client.buffered_amount() > CLIENT_BACKPRESSURE_LIMIT_BYTES {
                    overloaded_clients.push(Arc::clone(&registered.client));
                    continue;
                }
                registered.client.send(data);
            }
        }
        for client in overloaded_clients {
            client.close(Some(ClientClosure {
                code: BACKPRESSURE_CLOSE_CODE,
                reason: BACKPRESSURE_CLOSE_REASON.to_string(),
            }));
        }
    }

    /// 종료를 기록한다. 이미 레지스트리에 올랐으면 참이고 호출자가 종료를 처리한다
    pub fn record_exit(&self, exit: PtyExit) -> bool {
        let mut watch = lock(&self.exit_watch);
        if watch.is_registered {
            return true;
        }
        watch.exit_before_registration = Some(exit);
        false
    }

    /// 레지스트리에 올랐다고 표시한다. 그 전에 PTY가 끝났으면 그 종료를 돌려줘 호출자가 처리한다
    pub fn mark_registered(&self) -> Option<PtyExit> {
        let mut watch = lock(&self.exit_watch);
        watch.is_registered = true;
        watch.exit_before_registration.take()
    }
}

/// 데스크탑 창과 모바일 화면이 각각 원하는 크기. 모바일이 붙어 있는 동안은 모바일 크기가 이긴다
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct ViewportOwnership {
    pub desktop_size: Option<(u16, u16)>,
    pub mobile_size: Option<(u16, u16)>,
}

pub(super) struct TerminalEntry {
    pub pty: Arc<dyn PtyProcess>,
    pub remote_pty: Option<Arc<dyn RemoteHostPty>>,
    pub session_type: SessionType,
    pub session_name: String,
    pub task_id: String,
    pub tab_id: Option<String>,
    /// 원격 Herdr 세션에만 있다. 서버를 교체할 때 그 서버에 붙은 화면을 찾는 데 쓴다
    pub ssh_host: Option<String>,
    pub herdr_client: Option<HerdrTaskClient>,
    pub output: Arc<EntryOutput>,
    pub viewport: Mutex<ViewportOwnership>,
    /// 서버 교체로 닫는 화면에 보낼 사유. 있으면 화면은 오류 대신 다시 연결한다
    pub close_reason: Mutex<Option<String>>,
}

impl TerminalEntry {
    pub fn generation(&self) -> u64 {
        self.output.generation
    }

    /// PTY와 화면의 크기를 함께 바꾼다. PTY가 거절하면 화면은 그대로 둔다
    pub fn resize(&self, columns: u16, rows: u16) -> Result<(), String> {
        self.pty.resize(columns, rows)?;
        if let Some(screen) = lock(&self.output.sink).screen.as_mut() {
            screen.resize(columns, rows);
        }
        Ok(())
    }
}

/// 활성 PTY 목록. JS `Map`처럼 넣은 순서를 지켜 "태스크의 첫 Herdr 클라이언트" 같은 조회가 TS와 같은 항목을 고른다
#[derive(Default)]
pub(super) struct ActiveTerminals {
    entries: Vec<(String, Arc<TerminalEntry>)>,
}

impl ActiveTerminals {
    pub fn get(&self, terminal_key: &str) -> Option<&Arc<TerminalEntry>> {
        self.entries
            .iter()
            .find(|(key, _)| key == terminal_key)
            .map(|(_, entry)| entry)
    }

    /// 같은 키가 있으면 그 자리에서 바꾼다(JS `Map.set`)
    pub fn insert(&mut self, terminal_key: &str, entry: Arc<TerminalEntry>) {
        match self.entries.iter_mut().find(|(key, _)| key == terminal_key) {
            Some(slot) => slot.1 = entry,
            None => self.entries.push((terminal_key.to_string(), entry)),
        }
    }

    pub fn remove(&mut self, terminal_key: &str) -> Option<Arc<TerminalEntry>> {
        let index = self
            .entries
            .iter()
            .position(|(key, _)| key == terminal_key)?;
        Some(self.entries.remove(index).1)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Arc<TerminalEntry>)> {
        self.entries.iter().map(|(key, entry)| (key, entry))
    }

    pub fn keys(&self) -> Vec<String> {
        self.entries.iter().map(|(key, _)| key.clone()).collect()
    }
}
