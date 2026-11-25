//! pane 조회. 대상을 두 번 확인한 스냅샷을 만들고, 구독 감시가 방금 읽은 결과는 같은 pane의 다른 조회가 잠깐 재사용한다.

use std::sync::{Arc, LazyLock};

use kanvibe_ai::pane_chat::process::PaneChatProcess;
use kanvibe_ai::pane_chat::session_bindings::PaneChatSessionBinding;
use kanvibe_ai::pane_chat::snapshot::{
    PaneChatAgentActivity, PaneChatComposerCapabilities, PaneChatInputState, PaneChatLayout,
    PaneChatModelPicker, PaneChatModelPickerMode, PaneChatSnapshot, PaneChatTranscript,
    PaneChatTranscriptStatus,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_domain::pane_chat::MAX_PANE_CHAT_ATTACHMENTS;
use kanvibe_domain::task::SessionType;
use kanvibe_process::worktree::quote_for_posix_shell;
use regex::Regex;

use super::conversation::{
    ReadTranscript, TRANSCRIPT_PAGE_LIMIT, conversation_identity, empty_transcript, model_provider,
    read_shown_prompt, reader_scope, snapshot_provider,
};
use super::input_state::{read_agent_activity, read_input_state};
use super::queue::SharedOutcome;
use super::target::{conversation_key_for, conversation_parts, identity_for};
use super::{
    PaneChatError, PaneChatReadOptions, PaneChatReading, PaneChatService, input_error, lock,
};
use crate::pane_chat::surface::PaneChatSurface;

/// 구독 감시가 읽은 결과를 같은 pane의 데스크톱·HTTP 조회가 재사용하는 최대 경과 시간. 구독 감시가 pane을 다시 읽는 주기와 같다
const SHARED_READING_MAX_AGE_MS: f64 = 1_000.0;
/// 첨부 파일 하나의 최대 크기(TS `MAX_ATTACHMENT_BYTES`)
const MAX_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;

static TRANSCRIPT_CURSOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]{1,9}$").expect("대화 위치 패턴"));

type ReadingOutcome = Result<PaneChatReading, PaneChatError>;

/// 구독 감시가 최근에 읽은 결과. 데스크톱 창과 휴대폰이 같은 pane을 볼 때 외부 명령을 두 번 실행하지 않으려고 남긴다
pub(super) struct SharedReading {
    task_id: String,
    started_at: f64,
    includes_screen: bool,
    reading: Arc<SharedOutcome<ReadingOutcome>>,
}

/// 스냅샷의 targetToken은 요청한 pane 식별자별로 발급되므로, 같은 식별자로 요청한 조회끼리만 공유한다
fn shared_reading_key(task_id: &str, pane_id: Option<&str>) -> String {
    serde_json::json!([task_id, pane_id]).to_string()
}

fn disabled_snapshot(task_id: &str) -> PaneChatSnapshot {
    PaneChatSnapshot {
        task_id: task_id.to_string(),
        pane_id: None,
        session_type: None,
        provider: None,
        target_token: None,
        conversation_key: None,
        can_send: false,
        input_state: PaneChatInputState::Unknown,
        agent_activity: PaneChatAgentActivity::Unknown,
        screen: String::new(),
        reason: Some("pane-unavailable".to_string()),
        transcript: empty_transcript("exact-session-unavailable"),
        model_picker: PaneChatModelPicker {
            is_supported: false,
            mode: PaneChatModelPickerMode::NativeCli,
            is_api_supported: None,
        },
        model_settings: None,
        prompt: None,
        composer: None,
    }
}

fn reading_of(snapshot: PaneChatSnapshot) -> PaneChatReading {
    PaneChatReading {
        snapshot,
        local_transcript_file: None,
    }
}

fn target_changed(task_id: &str) -> PaneChatReading {
    let mut snapshot = disabled_snapshot(task_id);
    snapshot.reason = Some("target-changed".to_string());
    reading_of(snapshot)
}

/// 대상 확인을 한 번 마친 상태. 두 번째 확인과 스냅샷 조립에 쓴다
struct VerifiedTarget<'a> {
    task_id: &'a str,
    requested_pane_id: Option<&'a str>,
    surface: PaneChatSurface,
    process: PaneChatProcess,
    identity: String,
    is_display: bool,
    binding: Option<PaneChatSessionBinding>,
}

impl PaneChatService {
    /// 구독 감시가 방금 읽은 같은 pane의 결과가 있으면 그것을, 없으면 새로 읽는다.
    /// 재사용하는 결과도 읽을 때 대상 확인을 두 번 거친 값이고, 전송은 여전히 `assert_target`이 다시 확인한다
    pub fn read_recent_pane_chat(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: bool,
    ) -> ReadingOutcome {
        let key = shared_reading_key(task_id, pane_id);
        let shared = {
            let now = self.now_ms();
            let readings = lock(&self.shared_readings);
            readings
                .iter()
                .find(|(candidate, _)| *candidate == key)
                .filter(|(_, shared)| now - shared.started_at <= SHARED_READING_MAX_AGE_MS)
                .filter(|(_, shared)| shared.includes_screen || !should_include_screen)
                .map(|(_, shared)| shared.reading.clone())
        };
        if let Some(reading) = shared.filter(|_| !self.mutations.is_active(task_id)) {
            return reading.wait();
        }
        let options = PaneChatReadOptions {
            should_include_screen,
            is_shared_with_other_viewers: false,
        };
        self.read_pane_chat(task_id, pane_id, options)
    }

    pub fn get_pane_chat_snapshot(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
    ) -> Result<PaneChatSnapshot, PaneChatError> {
        Ok(self
            .read_recent_pane_chat(task_id, pane_id, false)?
            .snapshot)
    }

    pub fn read_pane_chat(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        options: PaneChatReadOptions,
    ) -> ReadingOutcome {
        if !options.is_shared_with_other_viewers {
            return self.read_verified_pane_chat(task_id, pane_id, options.should_include_screen);
        }
        let outcome = SharedOutcome::new();
        let key = shared_reading_key(task_id, pane_id);
        self.share_reading(
            key.clone(),
            SharedReading {
                task_id: task_id.to_string(),
                started_at: self.now_ms(),
                includes_screen: options.should_include_screen,
                reading: outcome.clone(),
            },
        );
        let result = self.read_verified_pane_chat(task_id, pane_id, options.should_include_screen);
        if result.is_err() {
            lock(&self.shared_readings).retain(|(candidate, shared)| {
                *candidate != key || !Arc::ptr_eq(&shared.reading, &outcome)
            });
        }
        outcome.settle(result.clone());
        result
    }

    fn share_reading(&self, key: String, shared: SharedReading) {
        let mut readings = lock(&self.shared_readings);
        readings.retain(|(candidate, previous)| {
            shared.started_at - previous.started_at <= SHARED_READING_MAX_AGE_MS
                && *candidate != key
        });
        readings.push((key, shared));
    }

    pub(super) fn forget_shared_readings(&self, task_id: &str) {
        lock(&self.shared_readings).retain(|(_, shared)| shared.task_id != task_id);
    }
}

impl PaneChatService {
    fn read_verified_pane_chat(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: bool,
    ) -> ReadingOutcome {
        if task_id.is_empty() {
            return Err(input_error("태스크와 pane 식별자가 올바르지 않습니다."));
        }
        let disabled = || reading_of(disabled_snapshot(task_id));
        let Ok(Some(surface)) = self.surfaces.read_pane_chat_surface(task_id, pane_id, true) else {
            return Ok(disabled());
        };
        let mut snapshot = disabled_snapshot(task_id);
        snapshot.pane_id = Some(surface.pane_id.clone());
        snapshot.session_type = surface.task.session_type;
        let process = match self.inspect_process(&surface) {
            Ok(Some(process)) => process,
            Ok(None) => {
                return Ok(self.reading_with_screen(&surface, snapshot, "no-foreground-ai-cli"));
            }
            Err(_) => {
                let reason = "process-inspection-unavailable";
                return Ok(self.reading_with_screen(&surface, snapshot, reason));
            }
        };
        let is_display = self.codex_display_execution(&surface, &process).is_some();
        let identity = identity_for(&surface, &process);
        let binding = match self.resolve_conversation(&surface, &process) {
            Ok(binding) => binding,
            Err(PaneChatError::TargetChanged) => return Ok(target_changed(task_id)),
            Err(_) => {
                snapshot.provider = Some(snapshot_provider(process.provider));
                snapshot.transcript = empty_transcript("transcript-read-failed");
                return Ok(self.reading_with_screen(&surface, snapshot, "transcript-read-failed"));
            }
        };
        let target = VerifiedTarget {
            task_id,
            requested_pane_id: pane_id,
            surface,
            process,
            identity,
            is_display,
            binding,
        };
        Ok(self.read_verified_target(target, snapshot, should_include_screen))
    }

    /// 대화 기록을 보여 주지 못하는 상태에서는 CLI 화면이 유일하게 보여 줄 내용이라 함께 읽는다
    fn reading_with_screen(
        &self,
        surface: &PaneChatSurface,
        mut unavailable: PaneChatSnapshot,
        reason: &str,
    ) -> PaneChatReading {
        match self.surfaces.read_pane_chat_screen(surface) {
            Ok(screen) => {
                unavailable.reason = Some(reason.to_string());
                unavailable.screen = screen;
                reading_of(unavailable)
            }
            Err(_) => reading_of(disabled_snapshot(&unavailable.task_id)),
        }
    }

    /// 대화와 모델 설정은 함께 읽는다(TS `Promise.all`)
    fn read_transcript_and_settings(
        &self,
        target: &VerifiedTarget<'_>,
    ) -> (
        ReadTranscript,
        Option<kanvibe_ai::pane_chat::model_settings::PaneChatModelSettings>,
    ) {
        let (surface, process) = (&target.surface, &target.process);
        let binding = target.binding.as_ref();
        let settings_source = binding.zip(model_provider(process.provider));
        std::thread::scope(|scope| {
            let settings = settings_source.map(|(binding, provider)| {
                scope.spawn(move || {
                    self.read_model_settings(
                        provider,
                        &binding.source_ref,
                        surface.task.ssh_host.as_deref(),
                        binding.is_pending,
                    )
                    .ok()
                })
            });
            let transcript = self
                .read_transcript(surface, process, binding)
                .unwrap_or_else(|_| ReadTranscript::failed());
            let settings = settings.and_then(|handle| handle.join().ok().flatten());
            (transcript, settings)
        })
    }

    fn read_verified_target(
        &self,
        target: VerifiedTarget<'_>,
        snapshot: PaneChatSnapshot,
        should_include_screen: bool,
    ) -> PaneChatReading {
        let conversation = conversation_identity(target.binding.as_ref());
        let (transcript_read, model_settings) = self.read_transcript_and_settings(&target);
        let mut transcript = transcript_read.transcript;
        if target.is_display && target.binding.is_none() {
            let display = self.codex_display_execution(&target.surface, &target.process);
            let reason = display.map(|display| lock(&display).reason.clone());
            transcript = empty_transcript(&reason.unwrap_or_default());
        }
        // 화면은 두 번째 확인보다 먼저 읽어, 읽는 사이 대상이 바뀌었으면 그 화면도 함께 버려지게 한다
        let mut screen = String::new();
        if should_include_screen || transcript.status != PaneChatTranscriptStatus::Available {
            match self.surfaces.read_pane_chat_screen(&target.surface) {
                Ok(read) => screen = read,
                Err(_) => return reading_of(disabled_snapshot(target.task_id)),
            }
        }
        if !self.is_same_target_again(&target, conversation.as_deref()) {
            return target_changed(target.task_id);
        }
        let surface = &target.surface;
        self.retire_changed_leases(
            target.task_id,
            &surface.pane_id,
            &target.identity,
            conversation.as_deref(),
        );
        let mut snapshot = snapshot;
        snapshot.screen = screen;
        snapshot.model_settings = model_settings;
        let local_transcript_file = match surface.task.ssh_host() {
            Some(_) => None,
            None => transcript_read.source_ref.clone(),
        };
        self.fill_verified_snapshot(&mut snapshot, &target, transcript, conversation);
        PaneChatReading {
            snapshot,
            local_transcript_file,
        }
    }

    /// 같은 pane을 다시 읽어 같은 실행·같은 대화인지 확인한다
    fn is_same_target_again(
        &self,
        target: &VerifiedTarget<'_>,
        conversation: Option<&str>,
    ) -> bool {
        let check = || -> Result<bool, PaneChatError> {
            let latest = self.surfaces.read_pane_chat_surface(
                target.task_id,
                target.requested_pane_id,
                target.is_display,
            )?;
            let Some(latest) = latest else {
                return Ok(false);
            };
            let Some(latest_process) = self.inspect_process(&latest)? else {
                return Ok(false);
            };
            if identity_for(&latest, &latest_process) != target.identity {
                return Ok(false);
            }
            let binding = self.resolve_conversation(&latest, &latest_process)?;
            Ok(conversation_identity(binding.as_ref()).as_deref() == conversation)
        };
        check().unwrap_or(false)
    }

    /// 보낼 수 있는 대상에 lease token과 draft 키, 화면의 선택 화면을 싣는다
    fn fill_send_target(
        &self,
        snapshot: &mut PaneChatSnapshot,
        target: &VerifiedTarget<'_>,
        conversation: Option<&str>,
    ) {
        let (surface, process) = (&target.surface, &target.process);
        let token = self.lease_target(surface, process, target.requested_pane_id, conversation);
        snapshot.target_token = Some(token);
        if target.binding.is_some() {
            snapshot.conversation_key = conversation_key_for(surface, process, conversation);
        }
        let shown = read_shown_prompt(surface, process);
        snapshot.prompt = match (shown, target.binding.as_ref()) {
            (Some(prompt), Some(binding)) => Some(self.show_asked_question_text(
                prompt,
                &binding.source_ref,
                surface.task.ssh_host.as_deref(),
            )),
            (shown, _) => shown,
        };
    }

    fn fill_verified_snapshot(
        &self,
        snapshot: &mut PaneChatSnapshot,
        target: &VerifiedTarget<'_>,
        transcript: PaneChatTranscript,
        conversation: Option<String>,
    ) {
        let (surface, process) = (&target.surface, &target.process);
        let can_send = transcript.status == PaneChatTranscriptStatus::Available;
        let supports_composer = model_provider(process.provider).is_some();
        if can_send {
            self.fill_send_target(snapshot, target, conversation.as_deref());
        }
        snapshot.provider = Some(snapshot_provider(process.provider));
        snapshot.can_send = can_send;
        snapshot.input_state = read_input_state(surface, process, target.is_display);
        snapshot.agent_activity = read_agent_activity(surface, process);
        snapshot.reason = if can_send {
            None
        } else {
            transcript.reason.clone()
        };
        snapshot.transcript = transcript;
        snapshot.model_picker = PaneChatModelPicker {
            is_supported: supports_composer,
            mode: PaneChatModelPickerMode::NativeCli,
            is_api_supported: Some(target.is_display),
        };
        snapshot.composer = Some(PaneChatComposerCapabilities {
            can_attach: supports_composer,
            can_use_skills: supports_composer,
            can_mention_files: supports_composer,
            can_send_while_busy: Some(target.is_display),
            max_attachment_bytes: MAX_ATTACHMENT_BYTES,
            max_attachments: MAX_PANE_CHAT_ATTACHMENTS as u64,
        });
    }
}

impl PaneChatService {
    pub fn get_pane_chat_layout(
        &self,
        task_id: &str,
        terminal_tab_id: Option<&str>,
    ) -> Result<PaneChatLayout, PaneChatError> {
        if task_id.is_empty() || terminal_tab_id == Some("") {
            return Err(input_error("태스크와 터미널 식별자가 올바르지 않습니다."));
        }
        // 이전 좌표를 남기면 바뀐 레이아웃의 다른 pane을 가릴 수 있다.
        Ok(self
            .surfaces
            .read_pane_chat_layout(task_id, terminal_tab_id)
            .unwrap_or(PaneChatLayout {
                active_pane_id: None,
                panes: Vec::new(),
            }))
    }

    /// 채팅을 닫고 터미널로 돌아갈 때 그 pane에 focus를 돌려준다. 다른 pane으로 대신 옮기지 않는다
    pub fn focus_pane_chat_terminal(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> Result<(), PaneChatError> {
        if task_id.is_empty() || pane_id.is_empty() {
            return Err(input_error("태스크와 pane 식별자가 올바르지 않습니다."));
        }
        let surface = self
            .surfaces
            .read_pane_chat_surface(task_id, Some(pane_id), false)?
            .filter(|surface| surface.pane_id == pane_id)
            .ok_or(PaneChatError::TargetChanged)?;
        let task = &surface.task;
        let command = match task.session_type {
            Some(SessionType::Herdr) => {
                return self
                    .terminals
                    .focus_herdr_task_pane(task_id, pane_id)
                    .map_err(PaneChatError::Other);
            }
            Some(SessionType::Terminal) => {
                let tabs = self.terminals.list_local_terminal_tabs(task_id);
                let is_active = tabs.iter().any(|tab| tab.id == pane_id && tab.is_active);
                return if is_active {
                    Ok(())
                } else {
                    Err(PaneChatError::TargetChanged)
                };
            }
            Some(SessionType::Tmux) => {
                format!("tmux select-pane -t {}", quote_for_posix_shell(pane_id))
            }
            Some(SessionType::Zellij) => format!(
                "zellij --session {} action focus-pane-id {}",
                quote_for_posix_shell(task.session_name.as_deref().unwrap_or_default()),
                quote_for_posix_shell(pane_id)
            ),
            None => return Err(PaneChatError::TargetChanged),
        };
        self.exec_probe(&command, task.ssh_host())?;
        Ok(())
    }

    /// 현재 검증된 pane 대화의 이전 기록만 읽고, 조회 도중 대상이 바뀌면 결과를 버린다
    pub fn get_pane_chat_transcript_page(
        &self,
        task_id: &str,
        target_token: &str,
        cursor: &str,
    ) -> Result<PaneChatTranscript, PaneChatError> {
        if !TRANSCRIPT_CURSOR.is_match(cursor) {
            return Err(input_error("이전 대화 위치가 올바르지 않습니다."));
        }
        let lease = self.assert_target(task_id, target_token, None)?;
        let conversation = lease
            .conversation
            .as_deref()
            .ok_or(PaneChatError::TargetChanged)?;
        let surface = self
            .surfaces
            .read_pane_chat_surface(task_id, lease.requested_pane_id.as_deref(), false)?
            .ok_or(PaneChatError::TargetChanged)?;
        let (session_id, source_ref) = conversation_parts(conversation);
        let detail = self
            .sessions
            .read_session_detail(
                lease.process.provider,
                &reader_scope(&surface.task),
                &session_id,
                &source_ref,
                Some(cursor),
                TRANSCRIPT_PAGE_LIMIT,
            )
            .map_err(PaneChatError::Other)?;
        self.assert_target(task_id, target_token, None)?;
        let detail = detail
            .filter(|detail| detail.session_id == session_id)
            .ok_or(PaneChatError::TargetChanged)?;
        Ok(PaneChatTranscript {
            status: PaneChatTranscriptStatus::Available,
            reason: None,
            session_id: Some(session_id),
            messages: detail.messages,
            next_cursor: detail.next_cursor,
            epoch: None,
            seq: None,
        })
    }

    /// pane에서 돌고 있는 에이전트 CLI의 이름만 확인한다. 화면과 대화 기록은 읽지 않는다
    pub fn get_pane_chat_agent_provider(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> Option<AiSessionProvider> {
        let surface = self
            .surfaces
            .read_pane_chat_surface(task_id, Some(pane_id), false)
            .ok()??;
        let process: Option<PaneChatProcess> = self.inspect_process(&surface).ok()?;
        process.map(|process| snapshot_provider(process.provider))
    }
}
