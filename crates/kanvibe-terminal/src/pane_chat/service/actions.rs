//! 확인한 대상에 입력을 넣는 동작. 모든 쓰기는 태스크의 입력 큐 안에서, 쓰기 직전의 대상 확인을 거친 뒤에만 한다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use kanvibe_ai::javascript::{JS_WHITESPACE_CLASS, js_trim, utf16_length};
use kanvibe_ai::pane_chat::claude_composer::{
    is_claude_composer_showing_paste, read_claude_composer_text,
};
use kanvibe_ai::pane_chat::model_settings::{
    PaneChatModelSettingChange, PaneChatModelSettings, build_pane_chat_model_command,
    parse_pane_chat_model_setting_change,
};
use kanvibe_ai::pane_chat::process::PaneChatProvider;
use kanvibe_ai::pane_chat::snapshot::PaneChatInputState;
use kanvibe_domain::pane_chat::{
    MAX_PANE_CHAT_CATALOG_SUGGESTIONS, MAX_PANE_CHAT_SUGGESTIONS, PaneChatKey,
};
use regex::Regex;
use serde_json::Value;

use super::conversation::{model_provider, reader_scope};
use super::dependencies::CodexThreadAddress;
use super::input_state::{EMPTY_CODEX_COMPOSER_HINT, read_input_state};
use super::target::{TargetLease, conversation_parts, identity_for};
use super::{
    PaneChatError, PaneChatReadOptions, PaneChatService, PaneChatSuggestions, input_error,
    unconfirmed_input_error,
};
use crate::pane_chat::attachments::{
    PaneChatAttachment, PaneChatAttachmentInput, PaneChatAttachmentOwner,
};
use crate::pane_chat::composer_catalog::{
    PaneChatCatalogContext, PaneChatCatalogProvider, PaneChatSuggestionKind,
};

/// 붙여넣기 처리가 끝난 뒤 Enter를 보내기까지 기다리는 시간. Orca CLI bridge와 같은 간격이다
pub(super) const PROMPT_SUBMIT_DELAY_MS: u64 = 500;
/// 에코를 확인하며 치는 명령의 글자 사이 간격
const PICKER_CHARACTER_DELAY_MS: u64 = 40;
const MAX_SUGGESTION_QUERY_LENGTH: usize = 256;

static SUGGESTION_QUERY_CONTROL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x00-\x1f\x7f]").expect("제어 문자 패턴"));
static FILE_MENTION_QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("[{JS_WHITESPACE_CLASS}@\"']")).expect("파일 참조 패턴"));
static PROCESS_CWD_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^n/").expect("cwd 패턴"));

/// 키 이름에 대응하는 터미널 바이트
pub(super) fn key_bytes(key: PaneChatKey) -> &'static str {
    match key {
        PaneChatKey::Up => "\x1b[A",
        PaneChatKey::Down => "\x1b[B",
        PaneChatKey::Left => "\x1b[D",
        PaneChatKey::Right => "\x1b[C",
        PaneChatKey::Enter => "\r",
        PaneChatKey::Escape => "\x1b",
    }
}

/// bracketed paste로 감싼다
pub(super) fn bracketed_paste(text: &str) -> String {
    format!("\x1b[200~{text}\x1b[201~")
}

pub(super) fn empty_codex_composer() -> Vec<String> {
    vec![String::new(), EMPTY_CODEX_COMPOSER_HINT.to_string()]
}

pub(super) fn format_file_mention(file_path: &str) -> String {
    match FILE_MENTION_QUOTED.is_match(file_path) {
        true => format!(
            "@\"{}\"",
            file_path.replace('\\', "\\\\").replace('"', "\\\"")
        ),
        false => format!("@{file_path}"),
    }
}

fn is_claude_or_codex(provider: PaneChatProvider) -> bool {
    model_provider(provider).is_some()
}

impl PaneChatService {
    /// 쓰기 직전 대상을 다시 확인하고, 그사이 lease가 회수되지 않았을 때만 쓴다
    pub(super) fn write_fenced(
        &self,
        task_id: &str,
        token: &str,
        bytes: &str,
        on_write: Option<&AtomicBool>,
        expected_composer: Option<&[String]>,
    ) -> Result<(), PaneChatError> {
        let lease = self.assert_target(task_id, token, expected_composer)?;
        if !self.is_current_lease(token, &lease) {
            return Err(PaneChatError::TargetChanged);
        }
        if let Some(attempted) = on_write {
            attempted.store(true, Ordering::SeqCst);
        }
        self.write_to_pane(task_id, &lease.pane_id, bytes)
    }

    pub(super) fn write_to_pane(
        &self,
        task_id: &str,
        pane_id: &str,
        bytes: &str,
    ) -> Result<(), PaneChatError> {
        self.mobile_bridge
            .write_to_pane(task_id, pane_id, bytes)
            .map_err(PaneChatError::Other)
    }

    /// 같은 동작 안에서 방금 확인한 대상에 pane을 다시 조회하지 않고 쓴다.
    /// 그사이 다른 조회가 대상 변경을 알아채 lease를 회수했으면 쓰지 않는다
    pub(super) fn write_to_verified_target(
        &self,
        task_id: &str,
        token: &str,
        lease: &Arc<TargetLease>,
        bytes: &str,
    ) -> Result<(), PaneChatError> {
        if !self.is_current_lease(token, lease) {
            return Err(PaneChatError::TargetChanged);
        }
        self.write_to_pane(task_id, &lease.pane_id, bytes)
    }

    /// 대상 확인은 pane·프로세스·대화를 다시 조회하는 비싼 일이라, 동작마다 여기서 한 번 하고 그 결과를 동작에 넘긴다
    pub(super) fn serialize_action(
        &self,
        task_id: &str,
        token: &str,
        action: impl FnOnce(Arc<TargetLease>) -> Result<(), PaneChatError>,
    ) -> Result<(), PaneChatError> {
        self.queue_mutation(task_id, || {
            action(self.assert_target(task_id, token, None)?)
        })
    }

    pub(super) fn require_ready_input(
        &self,
        task_id: &str,
        lease: &TargetLease,
    ) -> Result<(), PaneChatError> {
        let surface = self.surfaces.read_pane_chat_surface(
            task_id,
            lease.requested_pane_id.as_deref(),
            true,
        )?;
        let is_ready = surface.is_some_and(|surface| {
            let is_display = self
                .codex_display_execution(&surface, &lease.process)
                .is_some();
            read_input_state(&surface, &lease.process, is_display) == PaneChatInputState::Ready
        });
        if !is_ready {
            return Err(input_error(
                "CLI 입력란이 비어 있지 않거나 선택 화면이 열려 있습니다. 터미널에서 현재 상태를 확인해 주세요.",
            ));
        }
        Ok(())
    }

    pub(super) fn attachment_owner(
        &self,
        task_id: &str,
        target_token: &str,
    ) -> Result<PaneChatAttachmentOwner, PaneChatError> {
        let lease = self.assert_target(task_id, target_token, None)?;
        if !is_claude_or_codex(lease.process.provider) {
            return Err(input_error("이 CLI의 첨부 입력은 지원되지 않습니다."));
        }
        let surface = self
            .surfaces
            .read_pane_chat_surface(task_id, lease.requested_pane_id.as_deref(), false)?
            .filter(|surface| identity_for(surface, &lease.process) == lease.identity)
            .ok_or(PaneChatError::TargetChanged)?;
        Ok(PaneChatAttachmentOwner {
            task_id: task_id.to_string(),
            target_token: target_token.to_string(),
            identity: lease.identity.clone(),
            ssh_host: surface.task.ssh_host().map(str::to_string),
        })
    }

    pub fn stage_pane_chat_attachment(
        &self,
        task_id: &str,
        target_token: &str,
        input: &PaneChatAttachmentInput,
    ) -> Result<PaneChatAttachment, PaneChatError> {
        let owner = self.attachment_owner(task_id, target_token)?;
        let attachment = self.attachments.store_pane_chat_attachment(&owner, input)?;
        if let Err(error) = self.assert_target(task_id, target_token, None) {
            // 대상이 바뀐 사이에 올린 첨부는 남기지 않는다. 지우지 못해도 원래 오류를 알린다
            let _ = self.attachments.discard_pane_chat_attachment(
                task_id,
                target_token,
                &attachment.id,
            );
            return Err(error);
        }
        Ok(attachment)
    }

    pub fn remove_pane_chat_attachment(
        &self,
        task_id: &str,
        target_token: &str,
        id: &str,
    ) -> Result<(), PaneChatError> {
        if task_id.is_empty() || target_token.is_empty() {
            return Err(input_error("태스크와 CLI 대상이 올바르지 않습니다."));
        }
        Ok(self
            .attachments
            .discard_pane_chat_attachment(task_id, target_token, id)?)
    }

    pub fn get_pane_chat_suggestions(
        &self,
        task_id: &str,
        target_token: &str,
        kind: PaneChatSuggestionKind,
        query: &str,
        limit: Option<usize>,
    ) -> Result<PaneChatSuggestions, PaneChatError> {
        if utf16_length(query) > MAX_SUGGESTION_QUERY_LENGTH
            || SUGGESTION_QUERY_CONTROL.is_match(query)
        {
            return Err(input_error("검색 종류·검색어가 올바르지 않습니다."));
        }
        if limit.is_some_and(|limit| !(1..=MAX_PANE_CHAT_CATALOG_SUGGESTIONS).contains(&limit)) {
            return Err(input_error("제안 개수 상한이 올바르지 않습니다."));
        }
        let lease = self.assert_target(task_id, target_token, None)?;
        let empty = || PaneChatSuggestions {
            target_token: target_token.to_string(),
            items: Vec::new(),
            is_truncated: false,
        };
        let Some(context) = self.suggestion_context(task_id, &lease, kind)? else {
            return Ok(empty());
        };
        let limit = limit.unwrap_or(MAX_PANE_CHAT_SUGGESTIONS);
        let result = self
            .suggestions
            .read_pane_chat_suggestions(&context, kind, query, limit)
            .map_err(PaneChatError::Other)?;
        self.assert_target(task_id, target_token, None)?;
        Ok(PaneChatSuggestions {
            target_token: target_token.to_string(),
            items: result.items,
            is_truncated: result.is_truncated,
        })
    }

    /// 제안을 찾을 CLI의 위치. skill·명령은 CLI 프로세스의 실제 cwd를 우선한다. 제안을 지원하지 않는 CLI면 `None`이다
    fn suggestion_context(
        &self,
        task_id: &str,
        lease: &TargetLease,
        kind: PaneChatSuggestionKind,
    ) -> Result<Option<PaneChatCatalogContext>, PaneChatError> {
        let provider = match lease.process.provider {
            PaneChatProvider::Claude => PaneChatCatalogProvider::Claude,
            PaneChatProvider::Codex => PaneChatCatalogProvider::Codex,
            _ => return Ok(None),
        };
        let surface = self
            .surfaces
            .read_pane_chat_surface(task_id, lease.requested_pane_id.as_deref(), false)?
            .filter(|surface| identity_for(surface, &lease.process) == lease.identity)
            .ok_or(PaneChatError::TargetChanged)?;
        let task = &surface.task;
        let workspace_path = task
            .worktree_path
            .clone()
            .or_else(|| task.project_repo_path.clone())
            .ok_or_else(|| input_error("현재 CLI의 프로젝트 경로를 확인할 수 없습니다."))?;
        let mut cwd = workspace_path.clone();
        if kind != PaneChatSuggestionKind::File {
            // 프로세스 cwd 조회가 막혀도 task의 검증된 worktree 카탈로그는 사용할 수 있다.
            let command = format!("lsof -a -p {} -d cwd -Fn", lease.process.pid);
            if let Ok(output) = self.exec_probe(&command, task.ssh_host())
                && let Some(line) = output
                    .split('\n')
                    .find(|line| PROCESS_CWD_LINE.is_match(line))
            {
                cwd = line[1..].to_string();
            }
        }
        Ok(Some(PaneChatCatalogContext {
            provider,
            cwd,
            ssh_host: task.ssh_host().map(str::to_string),
            home_directory: None,
            provider_home: None,
            process_id: Some(lease.process.pid),
            workspace_path: Some(workspace_path),
        }))
    }

    pub fn interrupt_pane_chat(
        &self,
        task_id: &str,
        target_token: &str,
    ) -> Result<(), PaneChatError> {
        self.serialize_action(task_id, target_token, |lease| {
            self.write_to_verified_target(task_id, target_token, &lease, "\x03")
        })
    }

    /// 선택기에서 쓰는 키만 보낸다. 다른 이름은 받지 않는다
    pub fn send_pane_chat_key(
        &self,
        task_id: &str,
        target_token: &str,
        key: &str,
    ) -> Result<(), PaneChatError> {
        let key: PaneChatKey = serde_json::from_value(Value::String(key.to_string()))
            .map_err(|_| input_error("허용되지 않는 선택기 키입니다."))?;
        self.serialize_action(task_id, target_token, |lease| {
            self.write_to_verified_target(task_id, target_token, &lease, key_bytes(key))
        })
    }

    /// Enter 직전에 Claude 입력란이 방금 붙여넣은 메시지 그대로인지 확인한다.
    /// 붙여넣고 기다리는 사이 터미널에서 입력란을 지우고 다른 글을 쳤거나 선택 화면이 떴으면 Enter가 그것을 제출하므로 누르지 않는다
    pub(super) fn require_claude_composer_shows_paste(
        &self,
        task_id: &str,
        lease: &TargetLease,
        pasted_text: &str,
        image_count: usize,
    ) -> Result<(), PaneChatError> {
        let surface = self.surfaces.read_pane_chat_surface(
            task_id,
            lease.requested_pane_id.as_deref(),
            true,
        )?;
        let composer_text = surface.and_then(|surface| {
            read_claude_composer_text(surface.viewport_lines.as_ref()?, surface.cursor_row?)
        });
        match composer_text {
            Some(text) if is_claude_composer_showing_paste(&text, pasted_text, image_count) => {
                Ok(())
            }
            _ => Err(unconfirmed_input_error()),
        }
    }

    /// Enter 직전에 Claude 입력란이 방금 넣은 명령 그대로인지 확인한다.
    /// 붙여넣고 기다리는 사이 입력란의 글이 바뀌었거나 선택 화면이 떴으면 Enter가 다른 것을 제출하므로 누르지 않는다
    fn require_claude_composer_text(
        &self,
        task_id: &str,
        lease: &TargetLease,
        expected_text: &str,
    ) -> Result<(), PaneChatError> {
        let surface = self.surfaces.read_pane_chat_surface(
            task_id,
            lease.requested_pane_id.as_deref(),
            true,
        )?;
        let input_line = surface
            .and_then(|surface| surface.input_line)
            .unwrap_or_default();
        let is_showing_expected = input_line.find('❯').is_some_and(|marker_index| {
            js_trim(&input_line[..marker_index]).is_empty()
                && js_trim(&input_line[marker_index + '❯'.len_utf8()..]) == expected_text
        });
        if !is_showing_expected {
            return Err(unconfirmed_input_error());
        }
        Ok(())
    }

    /// 슬래시 명령을 CLI 입력란에 넣고 Enter로 실행한다.
    /// 입력란의 글을 화면에서 읽을 수 있는 Codex에는 글자마다 에코를 확인하며 치고, 그 밖의 CLI에는 한 번에 붙여넣는다.
    /// 어느 쪽이든 Enter는 기다린 뒤 대상을 다시 확인하고 보낸다. Claude는 입력란에 넣은 명령이 그대로 있는지도 마지막에 확인한다
    fn submit_pane_chat_command(
        &self,
        task_id: &str,
        token: &str,
        lease: &Arc<TargetLease>,
        command: &str,
    ) -> Result<(), PaneChatError> {
        if self.existing_codex_display(&lease.identity).is_some() {
            let mut echoed = String::new();
            for character in command.chars() {
                let expected = match echoed.is_empty() {
                    true => empty_codex_composer(),
                    false => vec![echoed.clone()],
                };
                self.write_fenced(
                    task_id,
                    token,
                    &character.to_string(),
                    None,
                    Some(&expected),
                )?;
                echoed.push(character);
                self.sleep_ms(PICKER_CHARACTER_DELAY_MS);
            }
        } else {
            self.write_to_verified_target(task_id, token, lease, &bracketed_paste(command))?;
        }
        self.sleep_ms(PROMPT_SUBMIT_DELAY_MS);
        let verified = self.assert_target(task_id, token, Some(&[command.to_string()]))?;
        if verified.process.provider == PaneChatProvider::Claude {
            self.require_claude_composer_text(task_id, &verified, command)?;
        }
        self.write_to_verified_target(task_id, token, &verified, "\r")
    }

    pub fn open_pane_chat_model_picker(
        &self,
        task_id: &str,
        target_token: &str,
    ) -> Result<(), PaneChatError> {
        self.serialize_action(task_id, target_token, |lease| {
            if !is_claude_or_codex(lease.process.provider) {
                return Err(input_error("이 CLI의 모델 선택기는 지원되지 않습니다."));
            }
            self.require_ready_input(task_id, &lease)?;
            self.submit_pane_chat_command(task_id, target_token, &lease, "/model")
        })
    }

    /// 모델 설정 API로 다룰 Codex 대화. 표시 대상이 아니거나 대화가 없으면 지원하지 않는다
    fn codex_settings_target(
        &self,
        task_id: &str,
        target_token: &str,
    ) -> Result<CodexThreadAddress, PaneChatError> {
        let lease = self.assert_target(task_id, target_token, None)?;
        let unsupported = || input_error("이 CLI는 모델 설정 API를 지원하지 않습니다.");
        let surface = self
            .surfaces
            .read_pane_chat_surface(task_id, lease.requested_pane_id.as_deref(), false)?
            .ok_or_else(unsupported)?;
        let conversation = lease.conversation.as_deref().ok_or_else(unsupported)?;
        let is_display = lease.process.provider == PaneChatProvider::Codex
            && self
                .codex_display_execution(&surface, &lease.process)
                .is_some();
        if !is_display {
            return Err(unsupported());
        }
        let (thread_id, source_ref) = conversation_parts(conversation);
        Ok(CodexThreadAddress {
            scope: reader_scope(&surface.task),
            socket: lease.process.codex_server_socket.clone(),
            thread_id,
            source_ref,
        })
    }

    pub fn get_pane_chat_model_settings(
        &self,
        task_id: &str,
        target_token: &str,
    ) -> Result<PaneChatModelSettings, PaneChatError> {
        let lease = self.assert_target(task_id, target_token, None)?;
        let unsupported = || input_error("이 CLI의 모델 설정은 지원되지 않습니다.");
        let provider = model_provider(lease.process.provider).ok_or_else(unsupported)?;
        let conversation = lease.conversation.as_deref().ok_or_else(unsupported)?;
        let (_, source_ref) = conversation_parts(conversation);
        let is_codex_display = lease.process.provider == PaneChatProvider::Codex
            && self.existing_codex_display(&lease.identity).is_some();
        // 대화 ID 표시 대상의 대화는 화면 하단에서만 읽힌다. 화면 없이 다시 확인하면 대화가 사라진 것으로 판정돼 연결이 끊긴다.
        let surface = self
            .surfaces
            .read_pane_chat_surface(
                task_id,
                lease.requested_pane_id.as_deref(),
                is_codex_display,
            )?
            .ok_or(PaneChatError::TargetChanged)?;
        let binding = self.resolve_conversation(&surface, &lease.process)?;
        if super::conversation::conversation_identity(binding.as_ref()) != lease.conversation {
            return Err(PaneChatError::TargetChanged);
        }
        let settings = match is_codex_display {
            true => {
                let address = self.codex_settings_target(task_id, target_token)?;
                self.sessions
                    .read_codex_thread_settings(&address)
                    .map_err(PaneChatError::Other)?
            }
            false => {
                let is_pending = binding.is_some_and(|binding| binding.is_pending);
                self.read_model_settings(
                    provider,
                    &source_ref,
                    surface.task.ssh_host.as_deref(),
                    is_pending,
                )?
            }
        };
        self.assert_target(task_id, target_token, None)?;
        Ok(settings)
    }

    /// 모델·effort·빠른 모드를 CLI의 슬래시 명령으로 바꾼다.
    ///
    /// 명령은 지금 읽은 선택지 목록으로만 만든다. Claude가 모델 전환을 되물으면 CLI 화면에 그 확인이 뜨고, 답은 사용자가 한다
    pub fn change_pane_chat_model_setting(
        &self,
        task_id: &str,
        target_token: &str,
        requested_change: &Value,
    ) -> Result<(), PaneChatError> {
        let change = parse_pane_chat_model_setting_change(requested_change)
            .ok_or_else(|| input_error("모델 설정 변경 형식이 올바르지 않습니다."))?;
        self.serialize_action(task_id, target_token, |lease| {
            self.apply_model_setting_change(task_id, target_token, &lease, &change)
        })
    }

    fn apply_model_setting_change(
        &self,
        task_id: &str,
        target_token: &str,
        lease: &Arc<TargetLease>,
        change: &PaneChatModelSettingChange,
    ) -> Result<(), PaneChatError> {
        let provider = model_provider(lease.process.provider)
            .ok_or_else(|| input_error("이 CLI의 모델 설정은 지원되지 않습니다."))?;
        let is_codex = lease.process.provider == PaneChatProvider::Codex;
        if is_codex && self.existing_codex_display(&lease.identity).is_some() {
            let address = self.codex_settings_target(task_id, target_token)?;
            let mut before_write = || self.assert_target(task_id, target_token, None).map(|_| ());
            return self
                .sessions
                .update_codex_thread_settings(&address, change, &mut before_write);
        }
        self.require_ready_input(task_id, lease)?;
        let reading = self.read_pane_chat(
            task_id,
            lease.requested_pane_id.as_deref(),
            PaneChatReadOptions::default(),
        )?;
        let command = reading
            .snapshot
            .model_settings
            .and_then(|settings| build_pane_chat_model_command(provider, &settings, change))
            .ok_or_else(|| {
                input_error(
                    "지금은 이 설정을 바꿀 수 없습니다. 터미널에서 현재 상태를 확인해 주세요.",
                )
            })?;
        self.submit_pane_chat_command(task_id, target_token, lease, &command)
    }
}
