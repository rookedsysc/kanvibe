//! 모바일 다이어그램 라우트와 런타임 렌더러(`DesktopRuntime::diagram_renderer`, TS `mobileDiagramRenderService`)를 잇는
//! 작은 어댑터. 요청 검증·네이티브 Mermaid·캐시·동시 요청 상한은 `kanvibe-diagram`이 맡고, 여기서는 결과를 라우트 경계의
//! JSON 필드와 [`BridgeError::Diagram`]으로 옮기기만 한다.

use kanvibe_diagram::service::DiagramRenderer;
use serde_json::{Map, Value};

use crate::bridge::{BridgeError, BridgeResult};

/// `{imageBase64, width, height, mimeType, naturalWidth?}`. 실패는 렌더러가 정한 상태 코드(400·422·429·500·503)와 문구다
pub fn render_mobile_diagram(
    renderer: &dyn DiagramRenderer,
    request: &Value,
) -> BridgeResult<Map<String, Value>> {
    let image = renderer
        .render_mobile_diagram(request)
        .map_err(|error| BridgeError::Diagram {
            status: error.status_code,
            message: error.message,
        })?;
    let Value::Object(mut fields) = image.to_response_json() else {
        return Err(BridgeError::Internal(
            "diagram response is not an object".to_string(),
        ));
    };
    // 라우트가 `success`를 맨 앞에 붙인다
    fields.shift_remove("success");
    Ok(fields)
}

#[cfg(test)]
mod tests {
    use kanvibe_diagram::contract::{MobileDiagramError, MobileDiagramImage};
    use serde_json::json;

    use super::*;

    struct FixedRenderer(Result<MobileDiagramImage, MobileDiagramError>);

    impl DiagramRenderer for FixedRenderer {
        fn render_mobile_diagram(
            &self,
            _request: &Value,
        ) -> Result<MobileDiagramImage, MobileDiagramError> {
            self.0.clone()
        }
    }

    #[test]
    fn images_become_route_fields_without_the_success_flag() {
        let renderer = FixedRenderer(Ok(MobileDiagramImage {
            image_base64: "AAAA".into(),
            width: 400,
            height: 120,
            mime_type: "image/jpeg",
            natural_width: Some(512),
        }));

        let fields = render_mobile_diagram(&renderer, &json!({})).unwrap();

        assert_eq!(
            Value::Object(fields),
            json!({ "imageBase64": "AAAA", "width": 400, "height": 120, "mimeType": "image/jpeg", "naturalWidth": 512 })
        );
    }

    #[test]
    fn renderer_errors_keep_their_status_and_message() {
        let renderer = FixedRenderer(Err(MobileDiagramError::new(429, "busy")));

        let error = render_mobile_diagram(&renderer, &json!({})).unwrap_err();

        assert_eq!(
            error,
            BridgeError::Diagram {
                status: 429,
                message: "busy".into()
            }
        );
    }
}
