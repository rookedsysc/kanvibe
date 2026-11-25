//! PTY 공유·등록·해제. 같은 터미널을 동시에 여는 요청은 PTY 하나를 함께 쓰고, PTY가 끝나거나 마지막 클라이언트가 떠나면 정리한다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use kanvibe_ai::javascript::utf16_prefix;
use kanvibe_domain::task::SessionType;

use super::entry::{EntryOutput, RegisteredClient, TerminalEntry, ViewportOwnership, lock};
use super::herdr_client::HerdrTaskClient;
use super::{
    AttachResult, ClientClosure, CreationOutcome, Flight, PreparationFlight, RemoteHostPty,
    TerminalAttachment, TerminalClient, TerminalRegistry, closure,
};
use crate::pty::{PtyEventCallbacks, PtyExit, PtyProcess};
use crate::screen::TerminalScreen;

/// WebSocket 규격의 "서버 재시작" 닫힘 코드. 서버 교체로 닫는 화면에 사유와 함께 보낸다
const SERVICE_RESTART_CLOSE_CODE: u16 = 1012;
const ATTACH_FAILED_MESSAGE: &str = "터미널 연결 실패";

/// 이미 떠 있거나 만들어지는 중인 PTY를 찾은 결과
enum SharedTerminal {
    /// 떠 있는 PTY에 바로 붙였다
    Attached(Option<TerminalAttachment>),
    /// 다른 요청이 만드는 중이다
    Pending(Arc<Flight<CreationOutcome>>),
    /// 아무도 없어 이 요청이 만든다. 그 순간 같은 세션을 준비하던 작업이 있으면 함께 붙잡는다
    Claimed(Arc<Flight<CreationOutcome>>, Option<Arc<PreparationFlight>>),
}

/// PTY를 띄우기 전에 정해지는 항목 정보
pub(super) struct EntryBlueprint {
    pub session_type: SessionType,
    pub session_name: String,
    pub task_id: String,
    pub tab_id: Option<String>,
    pub ssh_host: Option<String>,
    pub herdr_client: Option<HerdrTaskClient>,
}

impl EntryBlueprint {
    /// Herdr client 중계는 `prepare_entry_blueprint`가 단다. `ssh_host`는 원격 Herdr 세션에만 준다
    pub fn new(
        session_type: SessionType,
        session_name: &str,
        task_id: &str,
        tab_id: Option<&str>,
        ssh_host: Option<&str>,
    ) -> Self {
        Self {
            session_type,
            session_name: session_name.to_string(),
            task_id: task_id.to_string(),
            tab_id: tab_id.map(str::to_string),
            ssh_host: ssh_host.map(str::to_string),
            herdr_client: None,
        }
    }

    pub fn into_entry(
        self,
        pty: Arc<dyn PtyProcess>,
        remote_pty: Option<Arc<dyn RemoteHostPty>>,
        output: Arc<EntryOutput>,
    ) -> TerminalEntry {
        TerminalEntry {
            pty,
            remote_pty,
            session_type: self.session_type,
            session_name: self.session_name,
            task_id: self.task_id,
            tab_id: self.tab_id,
            ssh_host: self.ssh_host,
            herdr_client: self.herdr_client,
            output,
            viewport: Mutex::new(ViewportOwnership::default()),
            close_reason: Mutex::new(None),
        }
    }

    /// PTY를 띄우지 못했거나 클라이언트가 떠나 쓰지 않게 된 Herdr 중계를 정리한다
    pub fn discard_herdr_client(&self) {
        close_herdr_client(self.herdr_client.as_ref());
    }
}

pub(super) fn close_herdr_client(herdr_client: Option<&HerdrTaskClient>) {
    if let Some(Err(error)) = herdr_client.map(HerdrTaskClient::close) {
        eprintln!("Herdr 클라이언트 정리 실패: {error}");
    }
}

impl TerminalRegistry {
    pub(super) fn allocate_client_id(&self) -> u64 {
        self.next_client_id.fetch_add(1, Ordering::Relaxed)
    }

    /// 떠 있는 PTY에 붙거나, 만들어지는 중인 PTY를 기다렸다 붙거나, 직접 만든다.
    /// 먼저 만들던 쪽이 실패했으면 기다리던 쪽은 1011로 닫히고, 만들던 쪽 클라이언트가 떠나 PTY가 없으면 다시 시도한다
    pub(super) fn attach_or_spawn(
        &self,
        terminal_key: &str,
        preparation_key: &str,
        client: &Arc<dyn TerminalClient>,
        spawn: impl Fn() -> AttachResult,
    ) -> AttachResult {
        loop {
            let shared_terminal = self.find_shared_terminal(terminal_key, preparation_key, client);
            let creation = match shared_terminal {
                SharedTerminal::Attached(attachment) => return Ok(attachment),
                SharedTerminal::Claimed(creation, pending_preparation) => {
                    let spawn_after_preparation = || {
                        self.spawn_after_preparation(pending_preparation.as_ref(), client, &spawn)
                    };
                    return self.track_terminal_creation(
                        terminal_key,
                        &creation,
                        &spawn_after_preparation,
                    );
                }
                SharedTerminal::Pending(creation) => creation,
            };
            match creation.wait() {
                CreationOutcome::Settled(None) if client.is_open() => continue,
                CreationOutcome::Settled(Some(entry)) => {
                    if let Some(attachment) =
                        self.attach_if_still_active(terminal_key, &entry, client)
                    {
                        return Ok(attachment);
                    }
                }
                _ => {}
            }
            client.close(closure(1011, ATTACH_FAILED_MESSAGE));
            return Err(ATTACH_FAILED_MESSAGE.to_string());
        }
    }

    /// 조회와 생성 등록을 한 잠금 안에서 해, 두 요청이 모두 "아직 없음"을 보고 PTY를 하나씩 띄우지 않게 한다.
    /// 진행 중인 세션 준비도 같은 잠금 안에서 붙잡는다. 생성을 올린 뒤 시작한 준비는 이 생성을 기다리므로,
    /// 여기서 그 준비를 다시 기다리면 서로를 기다리게 된다(TS는 같은 동기 구간에서 확인했다)
    fn find_shared_terminal(
        &self,
        terminal_key: &str,
        preparation_key: &str,
        client: &Arc<dyn TerminalClient>,
    ) -> SharedTerminal {
        let mut state = self.lock_state();
        if let Some(entry) = state.active_terminals.get(terminal_key) {
            return SharedTerminal::Attached(self.attach_client_to_entry(
                terminal_key,
                entry,
                client,
            ));
        }
        if let Some(creation) = state.pending_creations.get(terminal_key) {
            return SharedTerminal::Pending(Arc::clone(creation));
        }
        let creation = Flight::new();
        state
            .pending_creations
            .insert(terminal_key.to_string(), Arc::clone(&creation));
        let pending_preparation = state.pending_preparations.get(preparation_key).cloned();
        SharedTerminal::Claimed(creation, pending_preparation)
    }

    /// 같은 세션을 CLI가 준비하는 중이었으면 끝나기를 기다린다. 그 사이 클라이언트가 떠났으면 만들지 않는다
    fn spawn_after_preparation(
        &self,
        pending_preparation: Option<&Arc<PreparationFlight>>,
        client: &Arc<dyn TerminalClient>,
        spawn: &impl Fn() -> AttachResult,
    ) -> AttachResult {
        if let Some(pending_preparation) = pending_preparation {
            let _ = pending_preparation.wait();
            if !client.is_open() {
                return Ok(None);
            }
        }
        spawn()
    }

    fn track_terminal_creation(
        &self,
        terminal_key: &str,
        creation: &Arc<Flight<CreationOutcome>>,
        spawn: &impl Fn() -> AttachResult,
    ) -> AttachResult {
        let result = spawn();
        let outcome = {
            let mut state = self.lock_state();
            if state
                .pending_creations
                .get(terminal_key)
                .is_some_and(|pending| Arc::ptr_eq(pending, creation))
            {
                state.pending_creations.remove(terminal_key);
            }
            match &result {
                Ok(_) => {
                    CreationOutcome::Settled(state.active_terminals.get(terminal_key).cloned())
                }
                Err(_) => CreationOutcome::Failed,
            }
        };
        creation.finish(outcome);
        result
    }

    /// 기다리던 PTY가 아직 그 키의 항목이면 붙는다. 그 사이 셸이 끝났으면 `None`이다
    fn attach_if_still_active(
        &self,
        terminal_key: &str,
        entry: &Arc<TerminalEntry>,
        client: &Arc<dyn TerminalClient>,
    ) -> Option<Option<TerminalAttachment>> {
        let state = self.lock_state();
        let active = state.active_terminals.get(terminal_key)?;
        Arc::ptr_eq(active, entry).then(|| self.attach_client_to_entry(terminal_key, entry, client))
    }

    /// 준비하는 사이 다른 경로가 같은 키의 PTY를 올렸으면 그것에 붙는다
    pub(super) fn attach_if_connected_during_bootstrap(
        &self,
        terminal_key: &str,
        client: &Arc<dyn TerminalClient>,
    ) -> Option<Option<TerminalAttachment>> {
        let state = self.lock_state();
        let entry = state.active_terminals.get(terminal_key)?;
        Some(self.attach_client_to_entry(terminal_key, entry, client))
    }

    /// 클라이언트 하나를 PTY에 붙인다. 같은 PTY를 여러 창이 공유할 수 있다. 닫힌 클라이언트는 붙이지 않는다
    pub(super) fn attach_client_to_entry(
        &self,
        terminal_key: &str,
        entry: &TerminalEntry,
        client: &Arc<dyn TerminalClient>,
    ) -> Option<TerminalAttachment> {
        if !client.is_open() {
            return None;
        }
        let client_id = {
            let mut sink = lock(&entry.output.sink);
            // 모바일이 먼저 연 멀티플렉서 세션에 데스크탑 뷰가 붙으면 그릴 화면을 그때 만든다(다음 다시 그리기부터 채워진다)
            if sink.screen.is_none() && client.answers_terminal_queries() {
                sink.screen = Some(TerminalScreen::new(entry.pty.columns(), entry.pty.rows()));
            }
            sink.register(client, || self.allocate_client_id())
        };
        Some(TerminalAttachment {
            terminal_key: terminal_key.to_string(),
            entry_generation: entry.generation(),
            client_id,
        })
    }

    /// PTY보다 먼저 만드는 출력 자리. terminal 세션은 나중에 붙는 클라이언트에게 지금 화면을 줄 headless 화면을 함께 둔다
    pub(super) fn create_entry_output(
        &self,
        session_type: SessionType,
        columns: u16,
        rows: u16,
        first_client: Option<&Arc<dyn TerminalClient>>,
    ) -> Arc<EntryOutput> {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        // 데스크탑 뷰는 이 화면을 그대로 그리므로 멀티플렉서 세션도 데스크탑이 처음 열면 화면을 둔다
        let is_desktop_view = first_client.is_some_and(|client| client.answers_terminal_queries());
        let screen = (session_type == SessionType::Terminal || is_desktop_view)
            .then(|| TerminalScreen::new(columns, rows));
        let first_client = first_client.map(|client| RegisteredClient {
            client_id: self.allocate_client_id(),
            client: Arc::clone(client),
        });
        EntryOutput::new(generation, screen, first_client)
    }

    /// PTY 출력은 화면과 클라이언트로 흘리고, 종료는 레지스트리에 알린다. 레지스트리가 이미 없으면 종료를 무시한다
    pub(super) fn build_pty_callbacks(
        self: &Arc<Self>,
        terminal_key: &str,
        output: &Arc<EntryOutput>,
        debug_label: &'static str,
    ) -> PtyEventCallbacks {
        let output_for_data = Arc::clone(output);
        let output_for_exit = Arc::clone(output);
        let registry = Arc::downgrade(self);
        let first_output_log = FirstOutputLog {
            terminal_key: terminal_key.to_string(),
            debug_label,
            is_debug_logging_enabled: self.is_debug_logging_enabled,
            registered_at: Instant::now(),
            has_logged: AtomicBool::new(false),
        };
        let terminal_key = terminal_key.to_string();
        PtyEventCallbacks {
            on_output: Box::new(move |data| {
                first_output_log.record(data);
                output_for_data.deliver(data);
            }),
            on_exit: Box::new(move |exit| {
                if !output_for_exit.record_exit(exit) {
                    return;
                }
                if let Some(registry) = registry.upgrade() {
                    registry.handle_pty_exit(
                        &terminal_key,
                        output_for_exit.generation,
                        exit,
                        debug_label,
                    );
                }
            }),
        }
    }

    /// 항목을 레지스트리에 올리고 첫 클라이언트를 붙인다. 올리기 전에 PTY가 끝났으면 바로 정리한다
    pub(super) fn register_terminal_entry(
        &self,
        terminal_key: &str,
        entry: TerminalEntry,
        client: &Arc<dyn TerminalClient>,
        debug_label: &'static str,
    ) -> Option<TerminalAttachment> {
        sync_screen_size(&entry);
        entry.output.set_query_reply_writer(&entry.pty);
        let entry = Arc::new(entry);
        let attachment = {
            let mut state = self.lock_state();
            state
                .active_terminals
                .insert(terminal_key, Arc::clone(&entry));
            self.attach_client_to_entry(terminal_key, &entry, client)
        };
        if let Some(exit) = entry.output.mark_registered() {
            self.handle_pty_exit(terminal_key, entry.generation(), exit, debug_label);
        }
        attachment
    }

    /// PTY가 끝났다. 지금 그 키의 항목일 때만 정리하고, 셸이 스스로 끝난 탭은 목록에서도 지운다.
    /// 서버 교체로 끊긴 탭과 daemon 연결만 끊긴 탭은 다시 연결되므로 남긴다
    pub(super) fn handle_pty_exit(
        &self,
        terminal_key: &str,
        generation: u64,
        exit: PtyExit,
        debug_label: &str,
    ) {
        let entry = {
            let mut state = self.lock_state();
            match state.active_terminals.get(terminal_key) {
                Some(entry) if entry.generation() == generation => {
                    state.active_terminals.remove(terminal_key)
                }
                _ => None,
            }
        };
        let Some(entry) = entry else {
            return;
        };
        self.debug_log(
            &format!("{debug_label} PTY onExit"),
            serde_json::json!({ "terminalKey": terminal_key, "exitCode": exit.exit_code, "signal": exit.signal }),
        );
        self.finish_destroy(
            &entry,
            terminal_key,
            Some(&format!("{debug_label}-pty-exit")),
            false,
        );
        let has_close_reason = lock(&entry.close_reason).is_some();
        if let Some(tab_id) = entry
            .tab_id
            .as_deref()
            .filter(|_| !has_close_reason && !exit.is_detached)
        {
            self.lock_state()
                .local_terminal_tabs
                .remove_tab(&entry.task_id, tab_id);
        }
    }

    /// PTY를 죽이고 붙어 있던 클라이언트를 모두 닫는다
    pub(super) fn destroy_terminal(
        &self,
        terminal_key: &str,
        trigger_label: Option<&str>,
        should_close_owned_session: bool,
    ) {
        let entry = self.lock_state().active_terminals.remove(terminal_key);
        let Some(entry) = entry else {
            self.debug_log(
                "destroyTerminal 호출 (entry 없음)",
                serde_json::json!({ "terminalKey": terminal_key, "triggerLabel": trigger_label }),
            );
            return;
        };
        self.finish_destroy(
            &entry,
            terminal_key,
            trigger_label,
            should_close_owned_session,
        );
    }

    /// 레지스트리에서 내린 항목의 PTY와 클라이언트, Herdr 중계를 정리한다
    fn finish_destroy(
        &self,
        entry: &TerminalEntry,
        terminal_key: &str,
        trigger_label: Option<&str>,
        should_close_owned_session: bool,
    ) {
        let (clients, _discarded_screen) = {
            let mut sink = lock(&entry.output.sink);
            (std::mem::take(&mut sink.clients), sink.screen.take())
        };
        self.debug_log(
            "destroyTerminal 진입",
            serde_json::json!({
                "terminalKey": terminal_key,
                "triggerLabel": trigger_label,
                "sessionName": entry.session_name,
                "clients": clients.len(),
            }),
        );
        // 이미 종료된 PTY는 실패해도 무시한다. 소유 세션 닫기가 실패하면 TS처럼 kill도 건너뛴다
        let _ =
            close_owned_session(entry, should_close_owned_session).and_then(|_| entry.pty.kill());
        let close_reason = lock(&entry.close_reason).clone();
        for registered in clients
            .iter()
            .filter(|registered| registered.client.is_open())
        {
            let closure = close_reason.as_ref().map(|reason| ClientClosure {
                code: SERVICE_RESTART_CLOSE_CODE,
                reason: reason.clone(),
            });
            registered.client.close(closure);
        }
        close_herdr_client(entry.herdr_client.as_ref());
    }

    /// 클라이언트 하나를 떼어낸다. 마지막 클라이언트가 떠나도 terminal 세션은 살려 두고, 멀티플렉서 세션만 PTY를 끊는다.
    /// 다른 PTY로 바뀐 뒤 옛 클라이언트의 닫힘은 무시한다
    pub(super) fn release_terminal_client(
        &self,
        attachment: &TerminalAttachment,
        trigger_label: &str,
    ) {
        let terminal_key = attachment.terminal_key.as_str();
        let entry = {
            let mut state = self.lock_state();
            let Some(entry) = state.active_terminals.get(terminal_key) else {
                return;
            };
            if entry.generation() != attachment.entry_generation {
                return;
            }
            let remaining_clients = {
                let mut sink = lock(&entry.output.sink);
                sink.clients
                    .retain(|registered| registered.client_id != attachment.client_id);
                sink.clients.len()
            };
            if remaining_clients > 0 {
                return;
            }
            if entry.session_type == SessionType::Terminal {
                self.debug_log(
                    "클라이언트가 모두 떠났지만 terminal 세션은 유지한다",
                    serde_json::json!({ "terminalKey": terminal_key, "triggerLabel": trigger_label }),
                );
                return;
            }
            state.active_terminals.remove(terminal_key)
        };
        if let Some(entry) = entry {
            self.finish_destroy(&entry, terminal_key, Some(trigger_label), false);
        }
    }

    /// 지금 그 키에 붙어 있는 같은 세대의 항목
    pub(super) fn attached_entry(
        &self,
        attachment: &TerminalAttachment,
    ) -> Option<Arc<TerminalEntry>> {
        self.active_entry(&attachment.terminal_key)
            .filter(|entry| entry.generation() == attachment.entry_generation)
    }
}

fn close_owned_session(
    entry: &TerminalEntry,
    should_close_owned_session: bool,
) -> Result<(), String> {
    match (&entry.remote_pty, should_close_owned_session) {
        (Some(remote_pty), true) => remote_pty.close_session(),
        _ => Ok(()),
    }
}

/// 화면은 요청한 크기로 먼저 만들었다. PTY가 다른 크기로 떴으면(daemon이 기존 세션을 이어 준 경우) PTY 크기에 맞춘다
fn sync_screen_size(entry: &TerminalEntry) {
    let (columns, rows) = (entry.pty.columns(), entry.pty.rows());
    if let Some(screen) = lock(&entry.output.sink).screen.as_mut()
        && (screen.columns() != usize::from(columns) || screen.rows() != usize::from(rows))
    {
        screen.resize(columns, rows);
    }
}

/// 첫 출력 진단 로그. 원격은 PTY를 띄운 뒤에도 SSH 인증과 세션 attach가 남아 있어 준비 시간과 따로 잰다
struct FirstOutputLog {
    terminal_key: String,
    debug_label: &'static str,
    is_debug_logging_enabled: bool,
    registered_at: Instant,
    has_logged: AtomicBool,
}

impl FirstOutputLog {
    fn record(&self, data: &str) {
        if self.has_logged.swap(true, Ordering::Relaxed) {
            return;
        }
        if self.is_debug_logging_enabled {
            let payload = serde_json::json!({ "terminalKey": self.terminal_key, "sample": utf16_prefix(data, 200) });
            eprintln!(
                "[터미널-진단] {} PTY 첫 데이터 수신 {payload}",
                self.debug_label
            );
        }
        if self.debug_label == "Remote" {
            let since_spawn_ms = self.registered_at.elapsed().as_millis();
            let payload = serde_json::json!({ "terminalKey": self.terminal_key, "sinceSpawnMs": since_spawn_ms });
            eprintln!("[터미널] 원격 PTY 첫 출력 {payload}");
        }
    }
}
