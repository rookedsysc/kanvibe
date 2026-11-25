//! 첨부 data URL 디코딩과 이미지 MIME 감지(TS `lib/attachmentContent.ts`).
//! 선언한 MIME과 실제 바이트가 다르거나 Base64가 표준 형태가 아니면 받지 않는다.

use std::sync::LazyLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use regex::Regex;

use crate::error::ServiceError;

pub const MAX_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
/// 헤더가 이 위치를 넘어서야 쉼표가 나오면 data URL로 보지 않는다
const MAX_HEADER_LENGTH: usize = 200;

static DATA_URL_HEADER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^data:([a-z0-9!#$&^_.+-]+/[a-z0-9!#$&^_.+-]+);base64$").expect("유효한 정규식")
});
static BASE64_PAYLOAD_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[A-Za-z0-9+/]*={0,2}$").expect("유효한 정규식"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedAttachment {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

pub fn detect_image_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&PNG_SIGNATURE) {
        return Some("image/png");
    }
    if bytes.starts_with(&[255, 216, 255]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// `image/jpg`는 `image/jpeg`와 같은 형식으로 본다
fn normalize_mime_type(mime_type: &str) -> String {
    let lowered = mime_type.to_lowercase();
    if lowered == "image/jpg" {
        "image/jpeg".into()
    } else {
        lowered
    }
}

/// 쉼표 앞 헤더에서 MIME을 읽고, 기대한 MIME이 있으면 같은지 확인한다
fn read_declared_mime_type(
    data_url: &str,
    expected_mime_type: Option<&str>,
) -> Result<(String, usize), ServiceError> {
    let comma = data_url
        .find(',')
        .filter(|&comma| data_url[..comma].encode_utf16().count() <= MAX_HEADER_LENGTH)
        .ok_or_else(|| ServiceError::new("첨부 파일 형식이 올바르지 않습니다."))?;
    let header = DATA_URL_HEADER_PATTERN
        .captures(&data_url[..comma])
        .ok_or_else(|| ServiceError::new("Base64 첨부 파일만 지원합니다."))?;
    let mime_type = normalize_mime_type(&header[1]);
    let expected = expected_mime_type.map(normalize_mime_type);
    if expected.is_some_and(|expected| expected != mime_type) {
        return Err(ServiceError::new(
            "첨부 파일 MIME 형식이 일치하지 않습니다.",
        ));
    }
    Ok((mime_type, comma))
}

/// 표준 Base64이고 디코딩한 크기가 상한 안이며 다시 인코딩해도 같은 문자열일 때만 바이트를 돌려준다
fn decode_canonical_payload(payload: &str) -> Result<Vec<u8>, ServiceError> {
    let max_payload_length = MAX_ATTACHMENT_BYTES.div_ceil(3) * 4;
    let is_well_formed = !payload.is_empty()
        && payload.len() <= max_payload_length
        && payload.len().is_multiple_of(4)
        && BASE64_PAYLOAD_PATTERN.is_match(payload);
    if !is_well_formed {
        return Err(ServiceError::new(
            "첨부 파일이 비어 있거나 크기·인코딩이 올바르지 않습니다.",
        ));
    }
    let too_large = || ServiceError::new("첨부 파일이 비어 있거나 10MiB를 초과합니다.");
    let bytes = STANDARD.decode(payload).map_err(|_| too_large())?;
    if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT_BYTES || STANDARD.encode(&bytes) != payload
    {
        return Err(too_large());
    }
    Ok(bytes)
}

pub fn decode_attachment_data_url(
    data_url: &str,
    expected_mime_type: Option<&str>,
) -> Result<DecodedAttachment, ServiceError> {
    let (mime_type, comma) = read_declared_mime_type(data_url, expected_mime_type)?;
    let bytes = decode_canonical_payload(&data_url[comma + 1..])?;
    let detected = detect_image_mime_type(&bytes);
    if mime_type.starts_with("image/") && detected != Some(mime_type.as_str()) {
        return Err(ServiceError::new(
            "지원하지 않거나 손상된 이미지입니다. PNG, JPEG, GIF, WebP를 사용해 주세요.",
        ));
    }
    let mime_type = match detected {
        Some(detected) if mime_type == "application/octet-stream" => detected.to_string(),
        _ => mime_type,
    };
    Ok(DecodedAttachment { bytes, mime_type })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aOK0AAAAASUVORK5CYII=";

    // parity: TS-lib-attachmentContent-b86292
    #[test]
    fn decodes_actual_images_and_ordinary_file_bytes_with_their_mime() {
        let png = STANDARD.decode(PNG_BASE64).unwrap();

        let image = decode_attachment_data_url(
            &format!("data:image/png;base64,{PNG_BASE64}"),
            Some("image/png"),
        )
        .unwrap();
        let text = decode_attachment_data_url("data:text/plain;base64,aGVsbG8=", None).unwrap();

        assert_eq!(
            image,
            DecodedAttachment {
                bytes: png.clone(),
                mime_type: "image/png".into()
            }
        );
        assert_eq!(
            text,
            DecodedAttachment {
                bytes: b"hello".to_vec(),
                mime_type: "text/plain".into()
            }
        );
        assert_eq!(detect_image_mime_type(&png), Some("image/png"));
        assert_eq!(detect_image_mime_type(b"not an image"), None);
    }

    // parity: TS-lib-attachmentContent-137974
    #[test]
    fn rejects_malformed_or_falsely_declared_bytes() {
        for value in [
            "data:image/png;base64,aGVsbG8=",
            "data:text/plain;base64,@@@@",
            "data:text/plain;base64,aGVsbG8",
            "data:text/plain;base64,",
            "https://example.com/photo.png",
        ] {
            assert!(decode_attachment_data_url(value, None).is_err(), "{value}");
        }
    }

    // parity: TS-lib-attachmentContent-0f7977
    #[test]
    fn rejects_mime_disagreement_and_decoded_files_over_the_limit() {
        let oversized = STANDARD.encode(vec![0u8; MAX_ATTACHMENT_BYTES + 1]);

        assert!(
            decode_attachment_data_url("data:text/plain;base64,aGVsbG8=", Some("image/png"))
                .is_err()
        );
        assert!(
            decode_attachment_data_url(
                &format!("data:application/octet-stream;base64,{oversized}"),
                None
            )
            .is_err()
        );
    }
}
