//! 첨부 data URL 해석(TS `attachmentContent.ts`). 다른 crate로 옮기는 같은 함수와 겹치지만, 그 crate에 기대지 않도록
//! 이 모듈 안에서만 쓰는 사본을 둔다. 오류 문구와 판정 순서는 TS와 바이트 단위로 같다.

use std::sync::LazyLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use kanvibe_ai::javascript::utf16_length;
use regex::Regex;

const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
const MAX_HEADER_UTF16_INDEX: usize = 200;

static DATA_URL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i-u)\Adata:([a-z0-9!#$&^_.+-]+/[a-z0-9!#$&^_.+-]+);base64\z").unwrap()
});
static BASE64_PAYLOAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[A-Za-z0-9+/]*={0,2}\z").unwrap());

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DecodedAttachment {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

/// 파일 앞 바이트로 실제 이미지 형식을 판정한다
fn detect_image_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[255, 216, 255]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

fn normalize_mime_type(mime_type: &str) -> String {
    let lowered = mime_type.to_lowercase();
    if lowered == "image/jpg" {
        "image/jpeg".to_string()
    } else {
        lowered
    }
}

fn is_payload_shape_valid(payload: &str) -> bool {
    let max_payload_length = MAX_ATTACHMENT_BYTES.div_ceil(3) * 4;
    !payload.is_empty()
        && payload.len() <= max_payload_length
        && payload.len().is_multiple_of(4)
        && BASE64_PAYLOAD.is_match(payload)
}

/// 표준 Base64로 다시 인코딩했을 때 입력과 같은 바이트만 받는다
fn decode_canonical_payload(payload: &str) -> Option<Vec<u8>> {
    let bytes = STANDARD.decode(payload).ok()?;
    let is_canonical = STANDARD.encode(&bytes) == payload;
    let is_size_valid = !bytes.is_empty() && bytes.len() <= MAX_ATTACHMENT_BYTES;
    (is_canonical && is_size_valid).then_some(bytes)
}

pub(super) fn decode_attachment_data_url(
    data_url: &str,
    expected_mime_type: &str,
) -> Result<DecodedAttachment, &'static str> {
    let comma = data_url
        .find(',')
        .filter(|comma| utf16_length(&data_url[..*comma]) <= MAX_HEADER_UTF16_INDEX)
        .ok_or("첨부 파일 형식이 올바르지 않습니다.")?;
    let header = DATA_URL_HEADER
        .captures(&data_url[..comma])
        .ok_or("Base64 첨부 파일만 지원합니다.")?;
    let mime_type = normalize_mime_type(&header[1]);
    if normalize_mime_type(expected_mime_type) != mime_type {
        return Err("첨부 파일 MIME 형식이 일치하지 않습니다.");
    }
    let payload = &data_url[comma + 1..];
    if !is_payload_shape_valid(payload) {
        return Err("첨부 파일이 비어 있거나 크기·인코딩이 올바르지 않습니다.");
    }
    let bytes =
        decode_canonical_payload(payload).ok_or("첨부 파일이 비어 있거나 10MiB를 초과합니다.")?;
    let detected = detect_image_mime_type(&bytes);
    if mime_type.starts_with("image/") && detected != Some(mime_type.as_str()) {
        return Err("지원하지 않거나 손상된 이미지입니다. PNG, JPEG, GIF, WebP를 사용해 주세요.");
    }
    let mime_type = match detected {
        Some(detected) if mime_type == "application/octet-stream" => detected.to_string(),
        _ => mime_type,
    };
    Ok(DecodedAttachment { bytes, mime_type })
}
