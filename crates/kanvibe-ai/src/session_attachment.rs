//! 기록에 남은 첨부(이미지·파일) 내용을 불러온다. 이 Task worktree의 세션에서 실제로 기록된 첨부만, 크기를 제한해 읽는다.
//! 외부 URL은 가져오지 않고, FIFO 같은 일반 파일이 아닌 경로는 쓰는 쪽을 기다리지 않고 거절한다.

use std::fs::File;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

use crate::aggregate::get_ai_session_detail;
use crate::error::{AiError, AiResult};
use crate::host::{CommandOptions, quote_shell_argument};
use crate::jsonl::read_json_lines;
use crate::message_attachments::{AttachmentSource, find_message_attachment};
use crate::parse_cache::{ParseCacheKind, cached_or_parse};
use crate::providers::AiSessionProvider;
use crate::session_listing::determine_match_scope;
use crate::types::{AiAttachmentKind, AiSessionAttachmentContent, AiSessionReaderContext};

/// TS `MAX_ATTACHMENT_BYTES`
pub const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const REMOTE_READ_TIMEOUT_MS: u64 = 15_000;
const LOCAL_READ_CHUNK_BYTES: usize = 64 * 1024;

const REMOTE_READ_SCRIPT: &str = "import base64,os,stat,sys\nf=os.fdopen(os.open(sys.argv[1],os.O_RDONLY|os.O_NONBLOCK),'rb')\ns=os.fstat(f.fileno())\nassert stat.S_ISREG(s.st_mode) and s.st_size<=int(sys.argv[2]),'Attachment is not a bounded regular file'\nb=f.read(int(sys.argv[2])+1)\nassert len(b)<=int(sys.argv[2]),'Attachment is too large'\nprint(base64.b64encode(b).decode('ascii'))";

static ATTACHMENT_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[a-f0-9]{64}$").expect("첨부 id 패턴"));

static DATA_URL_HEADER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^data:([a-z0-9!#$&^_.+-]+/[a-z0-9!#$&^_.+-]+);base64$")
        .expect("data URL 머리 패턴")
});

static BASE64_PAYLOAD_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[A-Za-z0-9+/]*={0,2}$").expect("base64 본문 패턴"));

/// Node `Buffer.from(text, "base64")`처럼 알파벳 밖 글자를 건너뛰고 패딩이 없어도 읽는 디코더
const LENIENT_BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// 파일 앞부분 서명으로 이미지 형식을 알아낸다
pub fn detect_image_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[255, 216, 255]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// TS `decodeAttachmentDataUrl`(기대 MIME 없이). base64 data URL을 바이트와 MIME으로 풀고 크기·인코딩·이미지 서명을 검사한다
pub fn decode_attachment_data_url(data_url: &str) -> AiResult<(Vec<u8>, String)> {
    let comma = data_url
        .find(',')
        .filter(|index| crate::javascript::utf16_length(&data_url[..*index]) <= 200)
        .ok_or_else(|| AiError::message("첨부 파일 형식이 올바르지 않습니다."))?;
    let header = DATA_URL_HEADER_PATTERN
        .captures(&data_url[..comma])
        .ok_or_else(|| AiError::message("Base64 첨부 파일만 지원합니다."))?;
    let lowered = header[1].to_lowercase();
    let mime_type = if lowered == "image/jpg" {
        "image/jpeg".to_string()
    } else {
        lowered
    };
    let payload = &data_url[comma + 1..];
    let max_payload_length = MAX_ATTACHMENT_BYTES.div_ceil(3) * 4;
    let is_payload_shape_valid = !payload.is_empty()
        && payload.len() <= max_payload_length
        && payload.len().is_multiple_of(4)
        && BASE64_PAYLOAD_PATTERN.is_match(payload);
    if !is_payload_shape_valid {
        return Err(AiError::message(
            "첨부 파일이 비어 있거나 크기·인코딩이 올바르지 않습니다.",
        ));
    }
    let bytes = LENIENT_BASE64.decode(payload).unwrap_or_default();
    if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT_BYTES || STANDARD.encode(&bytes) != payload
    {
        return Err(AiError::message(
            "첨부 파일이 비어 있거나 10MiB를 초과합니다.",
        ));
    }
    let detected = detect_image_mime_type(&bytes);
    if mime_type.starts_with("image/") && detected != Some(mime_type.as_str()) {
        return Err(AiError::message(
            "지원하지 않거나 손상된 이미지입니다. PNG, JPEG, GIF, WebP를 사용해 주세요.",
        ));
    }
    let resolved_mime = match detected {
        Some(image_mime) if mime_type == "application/octet-stream" => image_mime.to_string(),
        _ => mime_type,
    };
    Ok((bytes, resolved_mime))
}

/// 로컬 첨부를 읽는다. 쓰는 쪽이 없는 FIFO에서 멈추지 않도록 논블로킹으로 열고 일반 파일인지 먼저 본다
fn read_local_recorded_file(file_path: &str) -> AiResult<Vec<u8>> {
    let mut file = File::options()
        .read(true)
        .custom_flags(libc_nonblock())
        .open(file_path)
        .map_err(|error| AiError::from_io(&error, file_path))?;
    let metadata = file
        .metadata()
        .map_err(|error| AiError::from_io(&error, file_path))?;
    if !metadata.is_file() || metadata.len() > MAX_ATTACHMENT_BYTES as u64 {
        return Err(AiError::message("첨부 파일이 없거나 10MiB를 초과합니다."));
    }
    let mut bytes = Vec::new();
    let mut chunk = vec![0_u8; LOCAL_READ_CHUNK_BYTES];
    while bytes.len() <= MAX_ATTACHMENT_BYTES {
        let wanted = chunk.len().min(MAX_ATTACHMENT_BYTES + 1 - bytes.len());
        let read = file
            .read(&mut chunk[..wanted])
            .map_err(|error| AiError::from_io(&error, file_path))?;
        if read == 0 {
            break;
        }
        if bytes.len() + read > MAX_ATTACHMENT_BYTES {
            return Err(AiError::message("첨부 파일이 10MiB를 초과합니다."));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(bytes)
}

/// `O_NONBLOCK`. Linux와 macOS 값이 다르다
const fn libc_nonblock() -> i32 {
    if cfg!(target_os = "macos") {
        0x0004
    } else {
        0o4000
    }
}

fn read_recorded_file(context: &AiSessionReaderContext, file_path: &str) -> AiResult<Vec<u8>> {
    if !context.host.is_remote() {
        return read_local_recorded_file(file_path);
    }
    let command = format!(
        "python3 -c {} {} {MAX_ATTACHMENT_BYTES}",
        quote_shell_argument(REMOTE_READ_SCRIPT),
        quote_shell_argument(file_path)
    );
    let encoded = context.host.run(
        &command,
        CommandOptions {
            max_buffer_bytes: Some(MAX_ATTACHMENT_BYTES.div_ceil(3) * 4 + 4096),
            timeout_ms: Some(REMOTE_READ_TIMEOUT_MS),
        },
    )?;
    Ok(LENIENT_BASE64
        .decode(
            crate::javascript::js_trim(&encoded)
                .replace(|character: char| !is_base64_character(character), ""),
        )
        .unwrap_or_default())
}

fn is_base64_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '-' | '_')
}

/// 이 Task worktree의 세션에서 기록된 첨부 하나를 data URL로 돌려준다. 남의 세션·다른 worktree·잘못된 id면 `None`이다
pub fn read_ai_session_attachment(
    context: &AiSessionReaderContext,
    provider: AiSessionProvider,
    session_id: &str,
    attachment_id: &str,
    source_ref: Option<&str>,
) -> AiResult<Option<AiSessionAttachmentContent>> {
    let is_supported_provider = matches!(
        provider,
        AiSessionProvider::Claude | AiSessionProvider::Codex
    );
    if !ATTACHMENT_ID_PATTERN.is_match(attachment_id) || !is_supported_provider {
        return Ok(None);
    }
    let Some(detail) = get_ai_session_detail(
        context,
        provider,
        session_id,
        source_ref,
        None,
        Some(usize::MAX),
    )?
    else {
        return Ok(None);
    };
    if determine_match_scope(detail.matched_path.as_deref(), context).is_none() {
        return Ok(None);
    }
    let attachment = detail
        .messages
        .iter()
        .flat_map(|message| message.attachment_list())
        .find(|candidate| candidate.id == attachment_id);
    let (Some(attachment), Some(recorded_source)) = (attachment, detail.source_ref.as_deref())
    else {
        return Ok(None);
    };
    let events = cached_or_parse(ParseCacheKind::Full, &context.host, recorded_source, || {
        read_json_lines(&context.host, recorded_source)
    })?;
    for event in events.iter() {
        let content = recorded_content(event);
        let Some(recorded) = find_message_attachment(content, recorded_source, attachment_id)
        else {
            continue;
        };
        let Some((bytes, mime_type)) =
            decode_recorded_source(context, &recorded.source, attachment.mime_type.as_deref())?
        else {
            return Ok(None);
        };
        if attachment.kind == AiAttachmentKind::Image && detect_image_mime_type(&bytes).is_none() {
            return Err(AiError::message("지원하지 않거나 손상된 이미지입니다."));
        }
        return Ok(Some(AiSessionAttachmentContent {
            id: attachment.id.clone(),
            name: attachment.name.clone(),
            data_url: format!("data:{mime_type};base64,{}", STANDARD.encode(&bytes)),
            mime_type,
        }));
    }
    Ok(None)
}

/// TS `event.message?.content ?? event.payload?.content`
fn recorded_content(event: &Value) -> &Value {
    let message_content = event
        .get("message")
        .and_then(|message| message.get("content"));
    match message_content {
        Some(content) if !content.is_null() => content,
        _ => event
            .get("payload")
            .and_then(|payload| payload.get("content"))
            .unwrap_or(&Value::Null),
    }
}

/// 원천에서 바이트와 MIME을 얻는다. 기록된 로컬 파일이 사라졌으면 `None`이다
fn decode_recorded_source(
    context: &AiSessionReaderContext,
    source: &AttachmentSource,
    recorded_mime_type: Option<&str>,
) -> AiResult<Option<(Vec<u8>, String)>> {
    match source {
        AttachmentSource::Data { data_url } => decode_attachment_data_url(data_url).map(Some),
        AttachmentSource::Path { path } => {
            let bytes = match read_recorded_file(context, path) {
                Ok(bytes) => bytes,
                Err(error) if error.is_not_found() => return Ok(None),
                Err(error) => return Err(error),
            };
            let mime_type = detect_image_mime_type(&bytes)
                .or(recorded_mime_type)
                .unwrap_or("application/octet-stream");
            decode_attachment_data_url(&format!(
                "data:{mime_type};base64,{}",
                STANDARD.encode(&bytes)
            ))
            .map(Some)
        }
        AttachmentSource::Unavailable { .. } => Err(AiError::message(
            "이 기록의 외부 이미지 주소는 불러올 수 없습니다.",
        )),
    }
}

#[cfg(test)]
mod tests;
