//! 채팅 메시지 전송. Codex 표시 대상은 주소로 고정한 대화에 app-server로, 나머지는 빈 입력란에 붙여넣고 Enter로 보낸다.
//!
//! 요청 식별자가 같은 재요청은 처음 결과를 그대로 돌려받는다. 쓰기를 시도한 뒤의 실패는 CLI가 받았을 수 있으므로
//! "결과를 알 수 없음"으로 알리고 같은 요청을 다시 보내지 않는다.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use kanvibe_ai::codex_thread::CodexUserInput;
use kanvibe_ai::javascript::{JS_WHITESPACE_CLASS, js_trim, stringify};
use kanvibe_ai::pane_chat::codex_display::expect_codex_composer_texts;
use kanvibe_ai::pane_chat::process::PaneChatProvider;
use kanvibe_domain::pane_chat::MAX_PANE_CHAT_ATTACHMENTS;
use regex::Regex;
use serde_json::json;

use super::actions::{
    PROMPT_SUBMIT_DELAY_MS, bracketed_paste, empty_codex_composer, format_file_mention,
};
use super::conversation::reader_scope;
use super::dependencies::{CodexThreadAddress, CodexThreadDelivery};
use super::queue::{Receipt, SharedOutcome};
use super::target::{TargetLease, conversation_parts, sha256_hex};
use super::{PaneChatError, PaneChatService, input_error, lock};
use crate::pane_chat::attachments::{
    PaneChatAttachmentKind, PaneChatAttachmentOwner, ResolvedPaneChatAttachment,
};

const MAX_PROMPT_BYTES: usize = 64 * 1024;
const MAX_REQUEST_ID_BYTES: usize = 128;
const IMAGE_ATTACHMENT_SETTLE_MS: u64 = 300;

fn compile(pattern: &str) -> Regex {
    Regex::new(&pattern.replace(r"\s", &format!("[{JS_WHITESPACE_CLASS}]"))).expect("전송 패턴")
}

static PROMPT_CONTROL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x00-\x08\x0b-\x1f\x7f]").expect("제어 문자 패턴"));
static NATIVE_COMMAND: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?i)^/[a-z][a-z0-9-]*(?:\s|$)"));
static TRAILING_FILE_MENTION: LazyLock<Regex> =
    LazyLock::new(|| compile(r#"(?:^|\s)@(?:"(?:[^"\\]|\\[^\n\r\x{2028}\x{2029}])+"|[^\s"]+)$"#));
static TRAILING_CODEX_SKILL: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?:^|\s)\$[A-Za-z0-9_.-]+$"));
static TRAILING_WHITESPACE: LazyLock<Regex> = LazyLock::new(|| compile(r"\s$"));

/// 붙여넣을 이미지 경로에서 셸이 다르게 읽을 글자를 역슬래시로 막는다. U+0080 이상은 그대로 둔다
fn escape_image_path(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for character in path.chars() {
        let is_plain = character.is_ascii_alphanumeric()
            || matches!(character, '_' | '.' | '/' | '@' | ':' | '-')
            || u32::from(character) >= 0x80;
        if !is_plain {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

/// 전송 요청 하나
struct PromptRequest<'a> {
    task_id: &'a str,
    token: &'a str,
    text: &'a str,
    attachment_ids: &'a [String],
    request_id: Option<&'a str>,
    write_attempted: &'a AtomicBool,
}

fn is_valid_prompt(text: &str, attachment_ids: &[String], request_id: Option<&str>) -> bool {
    let has_content = !js_trim(text).is_empty() || !attachment_ids.is_empty();
    let is_valid_request_id =
        request_id.is_none_or(|id| !id.is_empty() && id.len() <= MAX_REQUEST_ID_BYTES);
    has_content
        && text.len() <= MAX_PROMPT_BYTES
        && !PROMPT_CONTROL.is_match(text)
        && attachment_ids.len() <= MAX_PANE_CHAT_ATTACHMENTS
        && is_valid_request_id
}

impl PaneChatService {
    pub fn send_pane_chat_prompt(
        &self,
        task_id: &str,
        target_token: &str,
        text: &str,
        attachment_ids: &[String],
        request_id: Option<&str>,
    ) -> Result<(), PaneChatError> {
        if !is_valid_prompt(text, attachment_ids, request_id) {
            return Err(input_error(
                "메시지·첨부 파일·요청 식별자가 올바르지 않습니다.",
            ));
        }
        let (receipt_owner, fingerprint) =
            self.identify_prompt(task_id, target_token, text, attachment_ids)?;
        let outcome = match self.admit_receipt(&receipt_owner, request_id, &fingerprint)? {
            Admission::Replay(previous) => return previous.wait(),
            Admission::New(outcome) => outcome,
        };
        let write_attempted = AtomicBool::new(false);
        let request = PromptRequest {
            task_id,
            token: target_token,
            text,
            attachment_ids,
            request_id,
            write_attempted: &write_attempted,
        };
        let result = self
            .serialize_action(task_id, target_token, |lease| {
                self.deliver_prompt(&request, &lease)
            })
            .map_err(|error| {
                let was_written = write_attempted.load(Ordering::SeqCst);
                if !was_written && let Some(request_id) = request_id {
                    self.forget_receipt(&receipt_owner, request_id);
                }
                classify_delivery_failure(was_written, error)
            });
        if let Some(outcome) = outcome {
            outcome.settle(result.clone());
        }
        result
    }

    /// 영수증을 묶을 주인과 요청 내용의 지문.
    /// Codex operation은 대화에 속한다. lease 갱신·X→Y→X가 이미 수락되거나 불명확한 요청을 새 전송으로 바꾸면 안 된다.
    fn identify_prompt(
        &self,
        task_id: &str,
        target_token: &str,
        text: &str,
        attachment_ids: &[String],
    ) -> Result<(String, String), PaneChatError> {
        let initial = self.current_lease(target_token).filter(|lease| {
            lease.task_id == task_id && lease.process.provider == PaneChatProvider::Codex
        });
        let codex_lease = initial.filter(|lease| lease.conversation_key.is_some());
        let Some(lease) = codex_lease else {
            let fingerprint = sha256_hex(&stringify(&json!([task_id, text, attachment_ids])));
            return Ok((target_token.to_string(), fingerprint));
        };
        let attachments =
            self.fingerprint_attachments(task_id, target_token, &lease, attachment_ids)?;
        let fingerprint = sha256_hex(&stringify(&json!([task_id, text, attachments])));
        let conversation_key = lease.conversation_key.clone().unwrap_or_default();
        Ok((format!("conversation:{conversation_key}"), fingerprint))
    }

    /// 쓰기 전에 거절된 요청은 같은 식별자로 다시 보낼 수 있게 영수증을 지운다
    fn forget_receipt(&self, receipt_owner: &str, request_id: &str) {
        if let Some(owned) = lock(&self.receipts).get_mut(receipt_owner) {
            owned.remove(request_id);
        }
    }

    fn fingerprint_attachments(
        &self,
        task_id: &str,
        target_token: &str,
        lease: &TargetLease,
        attachment_ids: &[String],
    ) -> Result<String, PaneChatError> {
        let owner = PaneChatAttachmentOwner {
            task_id: task_id.to_string(),
            target_token: target_token.to_string(),
            identity: lease.identity.clone(),
            ssh_host: lease.ssh_host.clone().filter(|host| !host.is_empty()),
        };
        Ok(self
            .attachments
            .fingerprint_pane_chat_attachments(&owner, attachment_ids)?)
    }

    /// 같은 요청 식별자가 이미 있으면 그 결과를, 없으면 새 결과 자리를 등록한다. 식별자가 없으면 기록하지 않는다
    fn admit_receipt(
        &self,
        receipt_owner: &str,
        request_id: Option<&str>,
        fingerprint: &str,
    ) -> Result<Admission, PaneChatError> {
        let Some(request_id) = request_id else {
            return Ok(Admission::New(None));
        };
        let mut receipts = lock(&self.receipts);
        let owned = receipts.entry(receipt_owner.to_string()).or_default();
        if let Some(previous) = owned.get(request_id) {
            return match previous.fingerprint == fingerprint {
                true => Ok(Admission::Replay(previous.outcome.clone())),
                false => Err(input_error(
                    "동일한 요청 식별자로 다른 메시지를 보낼 수 없습니다.",
                )),
            };
        }
        let outcome = SharedOutcome::new();
        let receipt = Receipt {
            fingerprint: fingerprint.to_string(),
            outcome: outcome.clone(),
        };
        owned.insert(request_id.to_string(), receipt);
        Ok(Admission::New(Some(outcome)))
    }

    fn deliver_prompt(
        &self,
        request: &PromptRequest<'_>,
        lease: &Arc<TargetLease>,
    ) -> Result<(), PaneChatError> {
        let (task_id, token) = (request.task_id, request.token);
        let owner = match request.attachment_ids.is_empty() {
            true => None,
            false => Some(self.attachment_owner(task_id, token)?),
        };
        let attachments = match &owner {
            Some(owner) => self
                .attachments
                .resolve_pane_chat_attachments(owner, request.attachment_ids)?,
            None => Vec::new(),
        };
        let is_display = self.existing_codex_display(&lease.identity).is_some();
        let (body, is_native_command) = compose_prompt_body(
            request.text,
            &attachments,
            lease.process.provider,
            is_display,
        );
        if let Some(owner) = &owner {
            self.attachments
                .retain_pane_chat_attachments(owner, request.attachment_ids)?;
        }
        let images: Vec<&ResolvedPaneChatAttachment> = attachments
            .iter()
            .filter(|entry| entry.kind == PaneChatAttachmentKind::Image)
            .collect();
        if is_display
            && !is_native_command
            && let Some(conversation) = &lease.conversation
        {
            return self.deliver_codex_thread_input(request, lease, conversation, &body, &images);
        }
        self.paste_prompt(request, lease, &body, &images)
    }

    /// 주소로 고정한 Codex 대화에 app-server로 보낸다. 보내기 직전 대상을 다시 확인하고, 응답을 잃어도 다시 보내지 않는다
    fn deliver_codex_thread_input(
        &self,
        request: &PromptRequest<'_>,
        lease: &TargetLease,
        conversation: &str,
        body: &str,
        images: &[&ResolvedPaneChatAttachment],
    ) -> Result<(), PaneChatError> {
        let (task_id, token) = (request.task_id, request.token);
        let (thread_id, source_ref) = conversation_parts(conversation);
        let surface = self
            .surfaces
            .read_pane_chat_surface(task_id, lease.requested_pane_id.as_deref(), false)?
            .ok_or(PaneChatError::TargetChanged)?;
        let mut input: Vec<CodexUserInput> = images
            .iter()
            .map(|image| CodexUserInput::LocalImage {
                path: image.path.clone(),
            })
            .collect();
        if !body.is_empty() {
            input.push(CodexUserInput::Text {
                text: body.to_string(),
            });
        }
        let message_seed = request
            .request_id
            .map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_string);
        let delivery = CodexThreadDelivery {
            address: CodexThreadAddress {
                scope: reader_scope(&surface.task),
                socket: lease.process.codex_server_socket.clone(),
                thread_id,
                source_ref,
            },
            input,
            client_user_message_id: sha256_hex(&stringify(&json!([token, message_seed]))),
        };
        let mut before_send = || self.assert_target(task_id, token, None).map(|_| ());
        let mut on_write = || request.write_attempted.store(true, Ordering::SeqCst);
        self.sessions
            .send_codex_thread_input(&delivery, &mut before_send, &mut on_write)
    }

    /// 빈 입력란을 확인하고 이미지 경로와 글을 붙여넣은 뒤, 기다렸다가 입력란을 다시 확인하고 Enter를 보낸다
    fn paste_prompt(
        &self,
        request: &PromptRequest<'_>,
        lease: &TargetLease,
        body: &str,
        images: &[&ResolvedPaneChatAttachment],
    ) -> Result<(), PaneChatError> {
        let (task_id, token) = (request.task_id, request.token);
        let write = |bytes: &str, expected: &[String]| {
            self.write_fenced(
                task_id,
                token,
                bytes,
                Some(request.write_attempted),
                Some(expected),
            )
        };
        self.require_ready_input(task_id, lease)?;
        let mut expected_composer = empty_codex_composer();
        for (index, image) in images.iter().enumerate() {
            write(
                &bracketed_paste(&escape_image_path(&image.path)),
                &expected_composer,
            )?;
            expected_composer = expect_codex_composer_texts("", index + 1);
            self.sleep_ms(IMAGE_ATTACHMENT_SETTLE_MS);
        }
        if !body.is_empty() {
            write(&bracketed_paste(body), &expected_composer)?;
        }
        self.sleep_ms(PROMPT_SUBMIT_DELAY_MS);
        if lease.process.provider == PaneChatProvider::Claude {
            self.require_claude_composer_shows_paste(task_id, lease, body, images.len())?;
        }
        write("\r", &expect_codex_composer_texts(body, images.len()))
    }
}

/// 쓰기를 시도하기 전의 실패와 대상 변경은 그대로 알리고, 쓰기를 시도한 뒤의 실패는 결과를 알 수 없다고 알린다
fn classify_delivery_failure(was_written: bool, error: PaneChatError) -> PaneChatError {
    match (was_written, error) {
        (false, error) | (true, error @ PaneChatError::TargetChanged) => error,
        (true, cause) => PaneChatError::DeliveryUncertain {
            cause: Box::new(cause),
        },
    }
}

/// 같은 요청 식별자의 처리 방법
enum Admission {
    /// 앞선 같은 요청의 결과를 기다린다
    Replay(Arc<SharedOutcome<Result<(), PaneChatError>>>),
    /// 새로 보낸다. 요청 식별자가 있으면 결과를 남길 자리가 함께 온다
    New(Option<Arc<SharedOutcome<Result<(), PaneChatError>>>>),
}

/// 글과 첨부 파일 참조를 합친 본문, 그리고 그 본문이 CLI 고유 슬래시 명령인지
fn compose_prompt_body(
    text: &str,
    attachments: &[ResolvedPaneChatAttachment],
    provider: PaneChatProvider,
    is_display: bool,
) -> (String, bool) {
    let files: Vec<String> = attachments
        .iter()
        .filter(|entry| entry.kind == PaneChatAttachmentKind::File)
        .map(|entry| format_file_mention(&entry.path))
        .collect();
    let parts: Vec<&str> = std::iter::once(text)
        .chain(files.iter().map(String::as_str))
        .filter(|part| !part.is_empty())
        .collect();
    let mut body = parts.join("\n\n");
    let is_native_command = attachments.is_empty() && NATIVE_COMMAND.is_match(&body);
    let is_open_ended = !TRAILING_WHITESPACE.is_match(&body);
    // Claude 파일 참조 팝업이 전송 Enter를 소비하지 않도록 마지막 토큰을 공백으로 닫는다.
    let closes_claude_mention = provider == PaneChatProvider::Claude
        && (!files.is_empty() || TRAILING_FILE_MENTION.is_match(&body));
    let closes_codex_skill = provider == PaneChatProvider::Codex
        && (!is_display || is_native_command)
        && TRAILING_CODEX_SKILL.is_match(&body);
    if is_open_ended && (closes_claude_mention || closes_codex_skill) {
        body.push(' ');
    }
    (body, is_native_command)
}
