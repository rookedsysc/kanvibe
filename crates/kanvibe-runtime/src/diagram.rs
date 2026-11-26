//! 앱과 모바일 다이어그램 라우트가 함께 쓰는 Mermaid 렌더러 하나(16개·16 MiB 캐시, 동시 4건).
//! 작업 상세 대화의 Mermaid 그림도 같은 글꼴 묶음([`SharedDiagramRenderer::fonts`])으로 그린다.
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
    fonts: OnceLock<Option<FontBook>>,
    renderer: OnceLock<Option<NativeDiagramRenderer>>,
}

impl SharedDiagramRenderer {
    pub(crate) fn new(font_files: DiagramFontFiles) -> Self {
        Self {
            font_files,
            fonts: OnceLock::new(),
            renderer: OnceLock::new(),
        }
    }

    /// 다이어그램 글꼴 묶음. 처음 부를 때 한 번 읽고, 읽지 못하면 None이다
    pub fn fonts(&self) -> Option<&FontBook> {
        self.fonts
            .get_or_init(|| match FontBook::from_font_files((self.font_files)()) {
                Ok(fonts) => Some(fonts),
                Err(error) => {
                    eprintln!("[kanvibe] diagram fonts unavailable: {error}");
                    None
                }
            })
            .as_ref()
    }

    fn renderer(&self) -> Option<&NativeDiagramRenderer> {
        self.renderer
            .get_or_init(|| self.fonts().cloned().map(NativeDiagramRenderer::with_fonts))
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
