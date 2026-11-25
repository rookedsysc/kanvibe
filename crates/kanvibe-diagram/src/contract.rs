//! 모바일 다이어그램 계약(`POST /api/mobile/diagrams/render`).
//!
//! `src/desktop/main/mobileDiagramContract.ts`를 옮겼다. 요청 검증, 이미지 크기 검사(PNG IHDR·JPEG SOF 해석),
//! 상태 코드와 한국어 오류 문구가 바이트 단위로 같다.

use base64::Engine;
use serde_json::Value;

use crate::theme::Theme;

pub const DEFINITION_BYTES_LIMIT: usize = 65_536;
pub const WIDTH_MIN: u32 = 320;
pub const WIDTH_MAX: u32 = 1200;
pub const HEIGHT_MAX: u32 = 2400;
pub const IMAGE_BYTES_LIMIT: usize = 4 * 1024 * 1024;
pub const PIXELS_LIMIT: u64 = 2_880_000;
pub const PENDING_LIMIT: usize = 4;
pub const CACHE_ENTRIES_LIMIT: usize = 16;
pub const CACHE_BYTES_LIMIT: usize = 16 * 1024 * 1024;

pub const INVALID_REQUEST_MESSAGE: &str = "잘못된 다이어그램 요청입니다";
pub const DIRECTIVE_MESSAGE: &str = "다이어그램 설정 지시문은 지원하지 않습니다";
pub const SYNTAX_MESSAGE: &str = "다이어그램 문법을 확인해 주세요";
pub const BUSY_MESSAGE: &str = "다이어그램 요청이 많습니다. 잠시 후 다시 시도해 주세요";
pub const IMAGE_FAILED_MESSAGE: &str = "다이어그램 이미지를 만들지 못했습니다";
pub const IMAGE_INVALID_MESSAGE: &str = "다이어그램 이미지가 올바르지 않습니다";
pub const IMAGE_SIZE_MESSAGE: &str = "다이어그램 이미지 크기가 올바르지 않습니다";

/// 라우트가 그대로 HTTP 상태와 `{ success: false, error }`로 옮기는 오류
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct MobileDiagramError {
    pub status_code: u16,
    pub message: String,
}

impl MobileDiagramError {
    pub fn new(status_code: u16, message: &str) -> Self {
        Self {
            status_code,
            message: message.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MobileDiagramInput {
    pub definition: String,
    pub theme: Theme,
    pub width: u32,
}

impl MobileDiagramInput {
    /// 캐시 키에 쓰는 정규화 JSON(TS `JSON.stringify(input)`과 같은 필드 순서)
    pub fn canonical_json(&self) -> String {
        let theme = match self.theme {
            Theme::Light => "light",
            Theme::Dark => "dark",
        };
        serde_json::json!({ "definition": self.definition, "theme": theme, "width": self.width })
            .to_string()
    }
}

/// 응답 본문의 이미지 부분. `natural_width`는 줄이기 전 다이어그램 폭이다
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MobileDiagramImage {
    pub image_base64: String,
    pub width: u32,
    pub height: u32,
    pub mime_type: &'static str,
    pub natural_width: Option<u32>,
}

impl MobileDiagramImage {
    /// `{ success: true, imageBase64, width, height, mimeType, naturalWidth? }`
    pub fn to_response_json(&self) -> Value {
        let mut body = serde_json::json!({
            "success": true,
            "imageBase64": self.image_base64,
            "width": self.width,
            "height": self.height,
            "mimeType": self.mime_type,
        });
        if let Some(natural_width) = self.natural_width {
            body["naturalWidth"] = Value::from(natural_width);
        }
        body
    }
}

fn invalid_request() -> MobileDiagramError {
    MobileDiagramError::new(400, INVALID_REQUEST_MESSAGE)
}

/// TS 정규식 `/%%\s*\{|^\s*---/m`: `%%{`는 어디에 있어도, `---`는 줄 머리(앞 공백 허용)에서 찾는다
fn has_directive(definition: &str) -> bool {
    let has_init = definition
        .match_indices("%%")
        .any(|(index, _)| definition[index + 2..].trim_start().starts_with('{'));
    has_init
        || definition
            .lines()
            .any(|line| line.trim_start().starts_with("---"))
}

pub fn parse_mobile_diagram_input(value: &Value) -> Result<MobileDiagramInput, MobileDiagramError> {
    let object = value.as_object().ok_or_else(invalid_request)?;
    let definition = object
        .get("definition")
        .and_then(Value::as_str)
        .ok_or_else(invalid_request)?;
    if definition.trim().is_empty() || definition.len() > DEFINITION_BYTES_LIMIT {
        return Err(invalid_request());
    }
    let theme = match object.get("theme").and_then(Value::as_str) {
        Some("light") => Theme::Light,
        Some("dark") => Theme::Dark,
        _ => return Err(invalid_request()),
    };
    let width = object
        .get("width")
        .and_then(integer_width)
        .ok_or_else(invalid_request)?;
    if !(WIDTH_MIN..=WIDTH_MAX).contains(&width) {
        return Err(invalid_request());
    }
    if has_directive(definition) {
        return Err(MobileDiagramError::new(400, DIRECTIVE_MESSAGE));
    }
    Ok(MobileDiagramInput {
        definition: definition.to_string(),
        theme,
        width,
    })
}

/// JS `Number.isInteger`: 1200.0처럼 소수부가 0인 실수도 정수다
fn integer_width(value: &Value) -> Option<u32> {
    let number = value.as_f64()?;
    if number.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&number) {
        return None;
    }
    Some(number as u32)
}

fn read_u16(image: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([image[offset], image[offset + 1]])
}

fn read_u32(image: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        image[offset],
        image[offset + 1],
        image[offset + 2],
        image[offset + 3],
    ])
}

fn image_invalid() -> MobileDiagramError {
    MobileDiagramError::new(500, IMAGE_INVALID_MESSAGE)
}

/// JPEG 표식을 따라가며 baseline·progressive SOF의 크기를 읽고 SOS까지 구조를 확인한다
fn jpeg_dimensions(image: &[u8]) -> Result<(u32, u32), MobileDiagramError> {
    let length = image.len();
    if length < 4
        || image[0] != 0xff
        || image[1] != 0xd8
        || image[length - 2] != 0xff
        || image[length - 1] != 0xd9
    {
        return Err(image_invalid());
    }
    let mut offset = 2;
    let mut dimensions: Option<(u32, u32)> = None;
    while offset < length - 2 {
        if image[offset] != 0xff {
            return Err(image_invalid());
        }
        offset += 1;
        while offset < length && image[offset] == 0xff {
            offset += 1;
        }
        let marker = *image.get(offset).ok_or_else(image_invalid)?;
        offset += 1;
        let is_invalid_marker = marker == 0
            || marker == 0xd8
            || marker == 0xd9
            || marker == 0x01
            || (0xd0..=0xd7).contains(&marker);
        if is_invalid_marker || offset + 2 > length - 2 {
            return Err(image_invalid());
        }
        let segment = usize::from(read_u16(image, offset));
        if segment < 2 || offset + segment > length - 2 {
            return Err(image_invalid());
        }
        if let Some(found) = read_frame_header(image, offset, marker, segment, dimensions)? {
            dimensions = Some(found);
        }
        if marker == 0xda {
            return check_scan_header(image, offset, segment, dimensions);
        }
        offset += segment;
    }
    Err(image_invalid())
}

fn read_frame_header(
    image: &[u8],
    offset: usize,
    marker: u8,
    segment: usize,
    dimensions: Option<(u32, u32)>,
) -> Result<Option<(u32, u32)>, MobileDiagramError> {
    let is_frame = (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker);
    let is_supported = [0xc0, 0xc1, 0xc2].contains(&marker);
    if is_frame && !is_supported {
        return Err(image_invalid());
    }
    if !is_supported {
        return Ok(None);
    }
    if dimensions.is_some() || segment < 11 || image[offset + 2] != 8 {
        return Err(image_invalid());
    }
    let components = image[offset + 7];
    if ![1, 3].contains(&components) || segment != 8 + usize::from(components) * 3 {
        return Err(image_invalid());
    }
    let height = u32::from(read_u16(image, offset + 3));
    let width = u32::from(read_u16(image, offset + 5));
    Ok(Some((width, height)))
}

fn check_scan_header(
    image: &[u8],
    offset: usize,
    segment: usize,
    dimensions: Option<(u32, u32)>,
) -> Result<(u32, u32), MobileDiagramError> {
    let components = usize::from(image[offset + 2]);
    let Some(dimensions) = dimensions else {
        return Err(image_invalid());
    };
    if segment < 6
        || !(1..=3).contains(&components)
        || segment != 6 + components * 2
        || offset + segment >= image.len() - 2
    {
        return Err(image_invalid());
    }
    Ok(dimensions)
}

pub fn validate_mobile_diagram_image(
    image: &[u8],
    expected_width: u32,
) -> Result<MobileDiagramImage, MobileDiagramError> {
    if image.is_empty() || image.len() > IMAGE_BYTES_LIMIT {
        return Err(MobileDiagramError::new(500, IMAGE_FAILED_MESSAGE));
    }
    let is_png = image.len() >= 33
        && image[..8] == [137, 80, 78, 71, 13, 10, 26, 10]
        && &image[12..16] == b"IHDR";
    let (width, height) = if is_png {
        (read_u32(image, 16), read_u32(image, 20))
    } else {
        jpeg_dimensions(image)?
    };
    let pixels = u64::from(width) * u64::from(height);
    if width != expected_width || !(1..=HEIGHT_MAX).contains(&height) || pixels > PIXELS_LIMIT {
        return Err(MobileDiagramError::new(500, IMAGE_SIZE_MESSAGE));
    }
    Ok(MobileDiagramImage {
        image_base64: base64::engine::general_purpose::STANDARD.encode(image),
        width,
        height,
        mime_type: if is_png { "image/png" } else { "image/jpeg" },
        natural_width: None,
    })
}
