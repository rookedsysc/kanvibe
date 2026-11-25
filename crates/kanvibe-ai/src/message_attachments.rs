//! 기록에 남은 이미지·파일 첨부. 본문 data URL, 절대 경로, 불러올 수 없는 외부 주소를 가려 메타데이터와 원천을 만든다.

use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

use crate::javascript::{is_present_truthy, js_string};
use crate::node_path;
use crate::types::{AiAttachmentKind, AiMessageAttachment};

/// 첨부 내용을 어디서 가져오는지
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentSource {
    Data { data_url: String },
    Path { path: String },
    Unavailable { reference: String },
}

impl AttachmentSource {
    fn reference(&self) -> &str {
        match self {
            Self::Data { data_url } => data_url,
            Self::Path { path } => path,
            Self::Unavailable { reference } => reference,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedAttachment {
    kind: AiAttachmentKind,
    name: String,
    mime_type: Option<String>,
    source: AttachmentSource,
}

/// 찾은 첨부와 그 원천
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundAttachment {
    pub attachment: AiMessageAttachment,
    pub source: AttachmentSource,
}

static DATA_URL_MIME_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^data:([^;,]+)").expect("data URL 패턴"));

fn attachment_kind(part_type: &str) -> Option<AiAttachmentKind> {
    match part_type {
        "image" | "input_image" | "local_image" | "image_url" => Some(AiAttachmentKind::Image),
        "document" | "file" | "input_file" => Some(AiAttachmentKind::File),
        _ => None,
    }
}

fn extension_mime_type(file_path: &str) -> Option<String> {
    let mime_type = match node_path::extname(file_path).to_lowercase().as_str() {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        ".pdf" => "application/pdf",
        ".txt" => "text/plain",
        _ => return None,
    };
    Some(mime_type.to_string())
}

fn string_field<'a>(part: &'a Value, key: &str) -> Option<&'a str> {
    part.get(key).and_then(Value::as_str)
}

fn recorded_attachments(content: &Value) -> Vec<RecordedAttachment> {
    match content {
        Value::Array(items) => items.iter().flat_map(recorded_attachments).collect(),
        Value::Object(_) => {
            let part_type = content
                .get("type")
                .map_or_else(|| "undefined".to_string(), js_string);
            match attachment_kind(&part_type) {
                Some(kind) => vec![recorded_attachment(content, kind)],
                None if is_present_truthy(content.get("content")) => {
                    recorded_attachments(&content["content"])
                }
                None => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

fn recorded_attachment(part: &Value, kind: AiAttachmentKind) -> RecordedAttachment {
    let source = part.get("source").filter(|source| source.is_object());
    let media_type = source
        .and_then(|source| string_field(source, "media_type"))
        .or_else(|| string_field(part, "mime_type"))
        .map(str::to_string);
    let image_url = match part.get("image_url") {
        Some(Value::String(url)) => Some(url.as_str()),
        Some(object @ Value::Object(_)) => object.get("url").and_then(Value::as_str),
        _ => None,
    };
    let data_url = recorded_data_url(part, source, media_type.as_deref(), image_url);
    let file_path = string_field(part, "path").or_else(|| string_field(part, "file_path"));
    let mime_type = media_type.clone().or_else(|| match (&data_url, file_path) {
        (Some(url), _) => DATA_URL_MIME_PATTERN
            .captures(url)
            .map(|found| found[1].to_string()),
        (None, Some(path)) => extension_mime_type(path),
        (None, None) => None,
    });
    let raw_name = string_field(part, "name")
        .or_else(|| string_field(part, "filename"))
        .or(file_path)
        .filter(|name| !name.is_empty());
    let name = match raw_name {
        Some(raw) => node_path::basename(&raw.replace('\\', "/")),
        None if kind == AiAttachmentKind::Image => {
            format!("image.{}", image_extension(mime_type.as_deref()))
        }
        None => "attachment".to_string(),
    };
    let source = match (data_url, file_path) {
        (Some(data_url), _) => AttachmentSource::Data { data_url },
        (None, Some(path)) if !path.is_empty() && node_path::is_absolute(path) => {
            AttachmentSource::Path {
                path: path.to_string(),
            }
        }
        _ => AttachmentSource::Unavailable {
            reference: image_url.unwrap_or_default().to_string(),
        },
    };
    RecordedAttachment {
        kind,
        name,
        mime_type,
        source,
    }
}

/// TS `mimeType?.split("/")[1] ?? "png"`
fn image_extension(mime_type: Option<&str>) -> String {
    mime_type
        .and_then(|mime| mime.split('/').nth(1))
        .unwrap_or("png")
        .to_string()
}

fn recorded_data_url(
    part: &Value,
    source: Option<&Value>,
    media_type: Option<&str>,
    image_url: Option<&str>,
) -> Option<String> {
    let base64_data = source
        .filter(|source| source.get("type").and_then(Value::as_str) == Some("base64"))
        .and_then(|source| string_field(source, "data"));
    let data_url = match base64_data {
        Some(data) => format!(
            "data:{};base64,{data}",
            media_type.unwrap_or("application/octet-stream")
        ),
        None => image_url
            .filter(|url| url.starts_with("data:"))
            .or_else(|| string_field(part, "file_data").filter(|data| data.starts_with("data:")))?
            .to_string(),
    };
    (!data_url.is_empty()).then_some(data_url)
}

fn attachment_metadata(
    record: &RecordedAttachment,
    source_ref: &str,
    index: usize,
) -> AiMessageAttachment {
    let mut hasher = Sha256::new();
    hasher.update(source_ref.as_bytes());
    hasher.update(b"\0");
    hasher.update(index.to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(record.source.reference().as_bytes());
    let id = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    AiMessageAttachment {
        id,
        kind: record.kind,
        name: record.name.clone(),
        mime_type: record.mime_type.clone(),
        size_bytes: estimated_size_bytes(&record.source),
    }
}

/// base64 본문 길이로 원래 바이트 수를 어림한다
fn estimated_size_bytes(source: &AttachmentSource) -> Option<i64> {
    let AttachmentSource::Data { data_url } = source else {
        return None;
    };
    let payload = data_url
        .split(',')
        .nth(1)
        .filter(|payload| !payload.is_empty())?;
    let length = crate::javascript::utf16_length(payload) as i64;
    let padding = if payload.ends_with("==") {
        2
    } else if payload.ends_with('=') {
        1
    } else {
        0
    };
    Some(length * 3 / 4 - padding)
}

/// 메시지 본문에서 첨부 메타데이터를 뽑는다. id는 기록 경로·순서·원천으로 만든 sha256이다
pub fn extract_message_attachments(content: &Value, source_ref: &str) -> Vec<AiMessageAttachment> {
    recorded_attachments(content)
        .iter()
        .enumerate()
        .map(|(index, record)| attachment_metadata(record, source_ref, index))
        .collect()
}

pub fn find_message_attachment(
    content: &Value,
    source_ref: &str,
    attachment_id: &str,
) -> Option<FoundAttachment> {
    recorded_attachments(content)
        .into_iter()
        .enumerate()
        .find_map(|(index, record)| {
            let attachment = attachment_metadata(&record, source_ref, index);
            (attachment.id == attachment_id).then_some(FoundAttachment {
                attachment,
                source: record.source,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_base64_image_with_estimated_size() {
        let content = json!([{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}}]);
        let attachments = extract_message_attachments(&content, "/s.jsonl");
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "image.png");
        assert_eq!(attachments[0].mime_type.as_deref(), Some("image/png"));
        assert_eq!(attachments[0].size_bytes, Some(5));
        assert_eq!(attachments[0].id.len(), 64);
        let found = find_message_attachment(&content, "/s.jsonl", &attachments[0].id).unwrap();
        assert_eq!(
            found.source,
            AttachmentSource::Data {
                data_url: "data:image/png;base64,aGVsbG8=".to_string()
            }
        );
    }

    #[test]
    fn local_paths_and_external_urls_keep_their_source() {
        let content = json!({"content": [
            {"type": "local_image", "path": "/tmp/shot.JPG"},
            {"type": "input_image", "image_url": "https://example.com/a.png"},
            {"type": "file", "file_path": "relative.txt"}
        ]});
        let attachments = extract_message_attachments(&content, "ref");
        assert_eq!(attachments[0].name, "shot.JPG");
        assert_eq!(attachments[0].mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(attachments[1].name, "image.png");
        assert_eq!(attachments[1].mime_type, None);
        assert_eq!(attachments[2].mime_type.as_deref(), Some("text/plain"));
        assert_eq!(
            find_message_attachment(&content, "ref", &attachments[1].id)
                .unwrap()
                .source,
            AttachmentSource::Unavailable {
                reference: "https://example.com/a.png".to_string()
            }
        );
    }
}
