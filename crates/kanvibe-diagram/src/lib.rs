//! 네이티브 Mermaid 렌더러.
//!
//! Mermaid 11.15 기본 테마의 모양과 dagre 레이아웃을 Rust로 옮겼다. 정의 문자열을 [`scene::Scene`]
//! (경로·글자·색)으로 바꾸고, 같은 장면을 GPUI 화면과 모바일용 PNG·JPEG(tiny-skia) 양쪽에서 쓴다.
//!
//! - [`render_scene`]: 정의 → 장면(GPUI 뷰가 직접 그린다)
//! - [`service::NativeDiagramRenderer`]: 모바일 계약([`contract`])을 지키는 [`service::DiagramRenderer`] 구현

pub mod contract;
pub mod dagre;
mod flowchart;
pub mod graph;
mod pie;
pub mod raster;
pub mod scene;
pub mod service;
pub mod text;
pub mod theme;

use scene::Scene;
use text::FontBook;
use theme::{Palette, Theme};

/// Mermaid `maxEdges`
pub const MAX_EDGES: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DiagramError {
    #[error("다이어그램 문법 오류: {0}")]
    Syntax(String),
    #[error("아직 지원하지 않는 다이어그램 종류: {0}")]
    Unsupported(String),
}

/// 다이어그램 종류. 정의의 첫 문장으로 정한다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramKind {
    Flowchart,
    Pie,
}

/// `%%` 주석과 빈 줄을 건너뛴 첫 낱말로 종류를 고른다
pub fn detect_kind(definition: &str) -> Result<DiagramKind, DiagramError> {
    let first = definition
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    let keyword = first
        .split(|character: char| character.is_whitespace() || character == ';')
        .next()
        .unwrap_or("");
    match keyword {
        "graph" | "flowchart" | "flowchart-v2" => Ok(DiagramKind::Flowchart),
        "pie" => Ok(DiagramKind::Pie),
        other => Err(DiagramError::Unsupported(other.to_string())),
    }
}

/// 정의를 테마에 맞는 장면으로 만든다
pub fn render_scene(
    definition: &str,
    theme: Theme,
    fonts: &FontBook,
) -> Result<Scene, DiagramError> {
    let palette = Palette::for_theme(theme);
    match detect_kind(definition)? {
        DiagramKind::Flowchart => flowchart::render(definition, fonts, &palette),
        DiagramKind::Pie => pie::render(definition, fonts, &palette),
    }
}
