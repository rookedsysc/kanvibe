//! 모바일 다이어그램 요청 해석과 렌더러 경계(TS `mobileDiagramContract.parseMobileDiagramInput`, `mobileDiagramRenderService`).
//!
//! 실제 렌더링(요청 검증, 네이티브 Mermaid, 캐시, 동시 요청 상한, 이미지 검증)은 M8의 `kanvibe-diagram`
//! (`MobileDiagramService`)이 [`DiagramRenderer`]로 채운다. 렌더러가 없으면 요청을 TS와 같이 검증한 뒤,
//! TS가 데스크탑 렌더러를 쓸 수 없을 때 내던 503을 그대로 돌려준다.

use serde_json::{Map, Value};

use crate::bridge::BridgeError;

pub const DEFINITION_MAX_BYTES: usize = 65_536;
const INVALID_REQUEST: &str = "잘못된 다이어그램 요청입니다";
pub const RENDERER_UNAVAILABLE: &str = "Desktop 다이어그램 렌더러를 사용할 수 없습니다";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramTheme {
    Light,
    Dark,
}

/// 검증을 통과한 요청
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileDiagramInput {
    pub definition: String,
    pub theme: DiagramTheme,
    pub width: u32,
}

/// 요청 본문(JSON)을 검증해 이미지를 만드는 쪽. 결과는 `{imageBase64, width, height, mimeType, naturalWidth?}`이고
/// 실패는 [`BridgeError::Diagram`](400·422·429·500·503)으로 돌려준다
pub trait DiagramRenderer: Send + Sync {
    fn render(&self, request: &Value) -> Result<Map<String, Value>, BridgeError>;
}

/// 렌더러가 연결되지 않은 데스크탑
pub struct UnavailableDiagramRenderer;

impl DiagramRenderer for UnavailableDiagramRenderer {
    fn render(&self, request: &Value) -> Result<Map<String, Value>, BridgeError> {
        parse_mobile_diagram_input(request)?;
        Err(BridgeError::Diagram {
            status: 503,
            message: RENDERER_UNAVAILABLE.to_string(),
        })
    }
}

fn invalid(message: &str) -> BridgeError {
    BridgeError::Diagram {
        status: 400,
        message: message.to_string(),
    }
}

/// 요청 값은 렌더러의 보안·자원 한도·테마를 덮어쓸 수 없다. 설정 지시문(`%%{`)과 front matter(`---`)는 막는다
pub fn parse_mobile_diagram_input(value: &Value) -> Result<MobileDiagramInput, BridgeError> {
    let Value::Object(fields) = value else {
        return Err(invalid(INVALID_REQUEST));
    };
    let definition = fields
        .get("definition")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let theme = match fields.get("theme").and_then(Value::as_str) {
        Some("light") => Some(DiagramTheme::Light),
        Some("dark") => Some(DiagramTheme::Dark),
        _ => None,
    };
    let width = fields
        .get("width")
        .and_then(Value::as_f64)
        .filter(|width| width.fract() == 0.0 && (320.0..=1200.0).contains(width));
    let is_definition_valid = !kanvibe_ai::javascript::js_trim(definition).is_empty()
        && definition.len() <= DEFINITION_MAX_BYTES;
    let (Some(theme), Some(width), true) = (theme, width, is_definition_valid) else {
        return Err(invalid(INVALID_REQUEST));
    };
    if has_configuration_directive(definition) {
        return Err(invalid("다이어그램 설정 지시문은 지원하지 않습니다"));
    }
    Ok(MobileDiagramInput {
        definition: definition.to_string(),
        theme,
        width: width as u32,
    })
}

/// `/%%\s*\{|^\s*---/m`. JS `\s`는 줄바꿈도 포함하므로 `%%`와 `{` 사이에 줄이 바뀌어도 걸린다
fn has_configuration_directive(definition: &str) -> bool {
    let is_whitespace = kanvibe_ai::javascript::is_js_whitespace;
    let has_directive = definition.match_indices("%%").any(|(index, _)| {
        definition[index + 2..]
            .trim_start_matches(is_whitespace)
            .starts_with('{')
    });
    let line_starts = std::iter::once(0).chain(
        definition
            .match_indices(['\n', '\r', '\u{2028}', '\u{2029}'])
            .map(|(index, separator)| index + separator.len()),
    );
    has_directive
        || line_starts.into_iter().any(|start| {
            definition[start..]
                .trim_start_matches(is_whitespace)
                .starts_with("---")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_valid_requests_and_rejects_bad_theme_width_and_directives() {
        let input = parse_mobile_diagram_input(
            &json!({ "definition": "graph TD; A-->B", "theme": "light", "width": 400 }),
        )
        .unwrap();
        assert_eq!(input.width, 400);
        let bad_theme = parse_mobile_diagram_input(
            &json!({ "definition": "graph TD", "theme": "blue", "width": 400 }),
        );
        assert_eq!(bad_theme, Err(invalid(INVALID_REQUEST)));
        let narrow = parse_mobile_diagram_input(
            &json!({ "definition": "graph TD", "theme": "dark", "width": 319 }),
        );
        assert_eq!(narrow, Err(invalid(INVALID_REQUEST)));
        for definition in [
            "---\ntitle: x\n---\ngraph TD",
            "%%{init: {}}%%\ngraph TD",
            "graph TD\n  %% \n{",
        ] {
            let directive = parse_mobile_diagram_input(
                &json!({ "definition": definition, "theme": "dark", "width": 400 }),
            );
            assert_eq!(
                directive,
                Err(invalid("다이어그램 설정 지시문은 지원하지 않습니다"))
            );
        }
    }
}
