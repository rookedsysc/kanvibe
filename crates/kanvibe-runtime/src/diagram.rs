//! 앱과 모바일 다이어그램 라우트가 함께 쓰는 Mermaid 렌더러 하나(16개·16 MiB 캐시, 동시 4건).
//!
//! 글꼴(CJK 포함 수십 MB)은 첫 렌더 요청 때 한 번만 읽는다. 다이어그램을 쓰지 않는 실행이 시작 때 그 메모리를
//! 미리 차지하지 않게 하려는 것이다.

use std::sync::{Arc, OnceLock};

use kanvibe_diagram::contract::{IMAGE_FAILED_MESSAGE, MobileDiagramError, MobileDiagramImage};
use kanvibe_diagram::service::{DiagramRenderer, NativeDiagramRenderer};
use kanvibe_diagram::text::FontBook;
use serde_json::Value;

/// 글꼴 파일 바이트를 돌려주는 함수. 앱은 번들 글꼴을, 창 없는 실행은 글꼴 디렉터리를 읽어 넘긴다
pub type DiagramFontFiles = Arc<dyn Fn() -> Vec<Vec<u8>> + Send + Sync>;

pub struct SharedDiagramRenderer {
    font_files: DiagramFontFiles,
    renderer: OnceLock<Option<NativeDiagramRenderer>>,
}

impl SharedDiagramRenderer {
    pub(crate) fn new(font_files: DiagramFontFiles) -> Self {
        Self {
            font_files,
            renderer: OnceLock::new(),
        }
    }

    fn renderer(&self) -> Option<&NativeDiagramRenderer> {
        self.renderer
            .get_or_init(|| match FontBook::from_font_files((self.font_files)()) {
                Ok(fonts) => Some(NativeDiagramRenderer::with_fonts(fonts)),
                Err(error) => {
                    eprintln!("[kanvibe] diagram fonts unavailable: {error}");
                    None
                }
            })
            .as_ref()
    }
}

impl DiagramRenderer for SharedDiagramRenderer {
    fn render_mobile_diagram(
        &self,
        request: &Value,
    ) -> Result<MobileDiagramImage, MobileDiagramError> {
        match self.renderer() {
            Some(renderer) => renderer.render_mobile_diagram(request),
            None => Err(MobileDiagramError::new(500, IMAGE_FAILED_MESSAGE)),
        }
    }
}
