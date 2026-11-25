//! PTY 세션 하나의 열기, 출력·입력·종료 스레드.

use std::sync::Weak;

use super::*;

impl PtyRegistry {
    pub(super) fn open(
        self: &Arc<Self>,
        peer: &Arc<dyn PtyPeer>,
        record: &HostRecord,
        id: &str,
    ) -> Result<(), String> {
        let (columns, rows) = dimensions(record)?;
        let mut state = self.state.lock().unwrap();
        if state.routes.contains_key(&(peer_key(peer), id.to_string())) {
            return Err("Duplicate PTY attachment id".into());
        }
        let key = record_text(record, "key").filter(|key| !key.is_empty() && key.len() <= 2048);
        let command = record_text(record, "command")
            .filter(|command| !command.is_empty() && command.len() <= HOST_RECORD_BYTES / 2);
        let (Some(key), Some(command)) = (key, command) else {
            return Err("Invalid PTY key or command".into());
        };
        let session = match find_session(&state, peer, record, key)? {
            Some(session) => session,
            None => self.start_session(&mut state, peer, key, command, (columns, rows))?,
        };
        let token = self.next_token.fetch_add(1, Ordering::SeqCst);
        session.add_attachment(token, peer, id)?;
        state.routes.insert(
            (peer_key(peer), id.to_string()),
            Route {
                session: Arc::clone(&session),
                token,
            },
        );
        drop(state);
        let (columns, rows) = *session.size.lock().unwrap();
        peer.send(record_from(json!({
            "type": "pty.ready", "id": id, "sessionId": session.id, "pid": session.pid, "cols": columns, "rows": rows,
        })));
        // 제한된 현재 화면을 먼저 보낸 다음, 이후 출력을 순서대로 받게 한다
        if let Some(removed) = session.send_snapshot(token) {
            self.remove_routes(&[removed]);
        }
        Ok(())
    }

    fn start_session(
        self: &Arc<Self>,
        state: &mut RegistryState,
        peer: &Arc<dyn PtyPeer>,
        key: &str,
        command: &str,
        (columns, rows): (u16, u16),
    ) -> Result<Arc<PtySession>, String> {
        let owned = state
            .sessions
            .values()
            .filter(|session| session.owner == peer.client_id())
            .count();
        if state.sessions.len() >= MAX_SESSIONS || owned >= MAX_OWNER_SESSIONS {
            return Err("PTY session capacity exceeded".into());
        }
        let LaunchedPty {
            pid,
            reader,
            writer,
            control,
            waiter,
        } = self.launcher.launch(command, columns, rows)?;
        let session = Arc::new(PtySession {
            id: uuid::Uuid::new_v4().to_string(),
            key: key.to_string(),
            owner: peer.client_id().to_string(),
            pid,
            size: Mutex::new((columns, rows)),
            control,
            phase: Mutex::new(SessionPhase::Running),
            output: Mutex::new(SessionOutput {
                screen: DaemonScreen::new(columns, rows),
                attachments: Vec::new(),
            }),
            input: Mutex::new(InputQueue::default()),
            input_changed: Condvar::new(),
            reader_finished: Mutex::new(false),
            reader_changed: Condvar::new(),
        });
        state
            .sessions
            .insert(session.id.clone(), Arc::clone(&session));
        let registry = Arc::downgrade(self);
        spawn_session_threads(&registry, &session, reader, writer, waiter);
        Ok(session)
    }

    /// 세션이 끝났다. 레지스트리에서 빼고 붙어 있던 attachment마다 종료를 알린다
    fn finish_session(&self, session: &Arc<PtySession>, exit_code: i64) {
        {
            let mut state = self.state.lock().unwrap();
            state.sessions.remove(&session.id);
            state
                .routes
                .retain(|_, route| !Arc::ptr_eq(&route.session, session));
        }
        let attachments = std::mem::take(&mut session.output.lock().unwrap().attachments);
        for attachment in attachments {
            attachment.peer.send(record_from(json!({
                "type": "pty.exit", "id": attachment.id, "sessionId": session.id, "exitCode": exit_code,
            })));
        }
    }
}

fn find_session(
    state: &RegistryState,
    peer: &Arc<dyn PtyPeer>,
    record: &HostRecord,
    key: &str,
) -> Result<Option<Arc<PtySession>>, String> {
    let session = match record.get("sessionId") {
        Some(session_id) => {
            let found = state
                .sessions
                .get(&value_text(session_id))
                .filter(|session| session.owner == peer.client_id())
                .ok_or("Unknown or unowned PTY session")?;
            Some(Arc::clone(found))
        }
        None => state
            .sessions
            .values()
            .find(|session| session.owner == peer.client_id() && session.key == key)
            .cloned(),
    };
    if session
        .as_ref()
        .is_some_and(|session| session.phase() != SessionPhase::Running)
    {
        return Err("PTY session is closing or exited".into());
    }
    Ok(session)
}

fn spawn_session_threads(
    registry: &Weak<PtyRegistry>,
    session: &Arc<PtySession>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    waiter: Box<dyn PtyWaiter>,
) {
    let (output_registry, output_session) = (registry.clone(), Arc::clone(session));
    std::thread::spawn(move || pump_output(&output_registry, &output_session, reader));
    let (input_registry, input_session) = (registry.clone(), Arc::clone(session));
    std::thread::spawn(move || pump_input(&input_registry, &input_session, writer));
    let (exit_registry, exit_session) = (registry.clone(), Arc::clone(session));
    std::thread::spawn(move || await_exit(&exit_registry, &exit_session, waiter));
}

fn pump_output(
    registry: &Weak<PtyRegistry>,
    session: &Arc<PtySession>,
    mut reader: Box<dyn Read + Send>,
) {
    let mut buffer = vec![0; HOST_CHUNK_BYTES];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                let removed = session.render(&buffer[..count]);
                if let (false, Some(registry)) = (removed.is_empty(), registry.upgrade()) {
                    registry.remove_routes(&removed);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    *session.reader_finished.lock().unwrap() = true;
    session.reader_changed.notify_all();
}

fn pump_input(
    registry: &Weak<PtyRegistry>,
    session: &Arc<PtySession>,
    mut writer: Box<dyn Write + Send>,
) {
    while let Some(chunk) = session.next_input() {
        let is_written = writer
            .write_all(&chunk.bytes)
            .and_then(|()| writer.flush())
            .is_ok();
        session.finish_input(&chunk);
        if is_written {
            session.notify_attachment(
                chunk.token,
                |id| json!({ "type": "pty.input-ready", "id": id }),
            );
            continue;
        }
        if session.phase() != SessionPhase::Running {
            return;
        }
        let removed = session.notify_attachment(chunk.token, |id| {
            json!({ "type": "pty.detached", "id": id, "error": "PTY input failed; outcome may be unknown" })
        });
        if let (Some(removed), Some(registry)) = (removed, registry.upgrade()) {
            registry.remove_routes(&[removed]);
            session.drop_attachment(chunk.token);
        }
    }
}

fn await_exit(registry: &Weak<PtyRegistry>, session: &Arc<PtySession>, waiter: Box<dyn PtyWaiter>) {
    let status = waiter.wait();
    {
        let mut phase = session.phase.lock().unwrap();
        if *phase == SessionPhase::Exited {
            return;
        }
        *phase = SessionPhase::Closing;
    }
    let finished = session.reader_finished.lock().unwrap();
    let _finished = session
        .reader_changed
        .wait_timeout_while(finished, EXIT_FLUSH_LIMIT, |finished| !*finished)
        .unwrap();
    drop(_finished);
    *session.phase.lock().unwrap() = SessionPhase::Exited;
    session.stop_input();
    if let Some(registry) = registry.upgrade() {
        registry.finish_session(session, status.exit_code);
    }
}

impl PtySession {
    pub(super) fn phase(&self) -> SessionPhase {
        *self.phase.lock().unwrap()
    }

    fn add_attachment(&self, token: u64, peer: &Arc<dyn PtyPeer>, id: &str) -> Result<(), String> {
        let mut output = self.output.lock().unwrap();
        if output.attachments.len() >= MAX_SESSION_ATTACHMENTS {
            return Err("PTY attachment capacity exceeded".into());
        }
        output.attachments.push(AttachmentState {
            token,
            peer: Arc::clone(peer),
            id: id.to_string(),
            pending_output: 0,
            is_ready: false,
        });
        Ok(())
    }

    /// 지금 화면을 보내고 attachment를 출력 구독 상태로 바꾼다. 흐름 제어로 떨어지면 뗀 자리를 돌려준다
    fn send_snapshot(&self, token: u64) -> Option<(PeerKey, String, u64)> {
        let mut output = self.output.lock().unwrap();
        let snapshot = output.screen.serialize();
        let index = output
            .attachments
            .iter()
            .position(|attachment| attachment.token == token)?;
        if !deliver(&mut output.attachments[index], snapshot.as_bytes()) {
            let removed = output.attachments.remove(index);
            return Some((peer_key(&removed.peer), removed.id, token));
        }
        output.attachments[index].is_ready = true;
        None
    }

    /// 출력 한 조각을 화면에 반영하고 구독 중인 attachment에 보낸다. 흐름 제어로 뗀 attachment를 돌려준다
    fn render(&self, bytes: &[u8]) -> Vec<(PeerKey, String, u64)> {
        let mut output = self.output.lock().unwrap();
        if self.phase() == SessionPhase::Exited {
            return Vec::new();
        }
        output.screen.write(bytes);
        let mut removed = Vec::new();
        output.attachments.retain_mut(|attachment| {
            let should_keep = !attachment.is_ready || deliver(attachment, bytes);
            if !should_keep {
                removed.push((
                    peer_key(&attachment.peer),
                    attachment.id.clone(),
                    attachment.token,
                ));
            }
            should_keep
        });
        removed
    }

    pub(super) fn acknowledge(&self, token: u64, bytes: Option<i64>) -> Result<(), String> {
        let mut output = self.output.lock().unwrap();
        let Some(attachment) = output
            .attachments
            .iter_mut()
            .find(|attachment| attachment.token == token)
        else {
            return Ok(());
        };
        let bytes = bytes
            .filter(|bytes| *bytes >= 0 && *bytes as usize <= attachment.pending_output)
            .ok_or("Invalid PTY output acknowledgement")?;
        attachment.pending_output -= bytes as usize;
        Ok(())
    }

    pub(super) fn queue_input(&self, token: u64, bytes: Vec<u8>) -> Result<(), String> {
        let mut input = self.input.lock().unwrap();
        if input.queued_bytes + bytes.len() > HOST_BUFFER_BYTES {
            return Err("PTY input backpressure limit".into());
        }
        input.queued_bytes += bytes.len();
        input.chunks.push_back(InputChunk { token, bytes });
        self.input_changed.notify_all();
        Ok(())
    }

    fn next_input(&self) -> Option<InputChunk> {
        let input = self.input.lock().unwrap();
        let mut input = self
            .input_changed
            .wait_while(input, |input| input.chunks.is_empty() && !input.is_stopped)
            .unwrap();
        if input.is_stopped {
            return None;
        }
        input.is_writing = true;
        input.chunks.pop_front()
    }

    fn finish_input(&self, chunk: &InputChunk) {
        let mut input = self.input.lock().unwrap();
        input.is_writing = false;
        input.queued_bytes = input.queued_bytes.saturating_sub(chunk.bytes.len());
    }

    /// attachment가 아직 붙어 있으면 레코드를 보낸다. 오류 알림이면 attachment를 떼고 그 자리를 돌려준다
    fn notify_attachment(
        &self,
        token: u64,
        record: impl Fn(&str) -> Value,
    ) -> Option<(PeerKey, String, u64)> {
        let output = self.output.lock().unwrap();
        let attachment = output
            .attachments
            .iter()
            .find(|attachment| attachment.token == token)?;
        attachment.peer.send(record_from(record(&attachment.id)));
        Some((peer_key(&attachment.peer), attachment.id.clone(), token))
    }

    /// attachment를 떼고, 그 attachment가 넣어 둔 입력 가운데 아직 쓰기 시작하지 않은 것을 버린다
    pub(super) fn drop_attachment(&self, token: u64) {
        self.output
            .lock()
            .unwrap()
            .attachments
            .retain(|attachment| attachment.token != token);
        let mut input = self.input.lock().unwrap();
        let dropped: usize = input
            .chunks
            .iter()
            .filter(|chunk| chunk.token == token)
            .map(|chunk| chunk.bytes.len())
            .sum();
        input.chunks.retain(|chunk| chunk.token != token);
        input.queued_bytes = input.queued_bytes.saturating_sub(dropped);
    }

    pub(super) fn stop_input(&self) {
        let mut input = self.input.lock().unwrap();
        input.is_stopped = true;
        input.chunks.clear();
        input.queued_bytes = 0;
        self.input_changed.notify_all();
    }
}

/// 출력 창(1 MiB)을 넘으면 `pty.detached`로 알리고 false를 돌려준다
fn deliver(attachment: &mut AttachmentState, bytes: &[u8]) -> bool {
    if attachment.pending_output + bytes.len() > OUTPUT_WINDOW_BYTES {
        attachment.peer.send(record_from(json!({
            "type": "pty.detached", "id": attachment.id,
            "error": "PTY output backpressure limit; reattach for current screen",
        })));
        return false;
    }
    attachment.pending_output += bytes.len();
    for chunk in bytes.chunks(HOST_CHUNK_BYTES) {
        attachment.peer.send(record_from(
            json!({ "type": "pty.data", "id": attachment.id, "data": encode_host_bytes(chunk) }),
        ));
    }
    true
}
