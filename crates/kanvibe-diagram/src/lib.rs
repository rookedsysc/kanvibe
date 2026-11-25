//! 네이티브 Mermaid 렌더러.
//!
//! Mermaid 11.15 기본 테마의 모양과 dagre 레이아웃을 Rust로 옮겼다. 정의 문자열을 [`scene::Scene`]
//! (경로·글자·색)으로 바꾸고, 같은 장면을 GPUI 화면과 모바일용 PNG·JPEG(tiny-skia) 양쪽에서 쓴다.
//!
//! - [`render_scene`]: 정의 → 장면(GPUI 뷰가 직접 그린다). 데스크톱 화면은 [`LabelMode::SvgText`],
//!   모바일 워커는 [`LabelMode::Html`]로 라벨 크기를 잰다
//! - [`service::NativeDiagramRenderer`]: 모바일 계약([`contract`])을 지키는 [`service::DiagramRenderer`] 구현

mod c4;
mod class;
pub mod contract;
pub mod dagre;
pub mod directive;
mod er;
mod flowchart;
mod gantt;
mod git_graph;
pub mod graph;
mod hinting;
mod journey;
pub mod label;
mod mindmap;
mod pie;
mod quadrant;
pub mod raster;
mod requirement;
mod sanitize;
pub mod scene;
mod sequence;
pub mod service;
mod state;
pub mod text;
pub mod theme;
mod timeline;

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

/// 라벨을 그리는 방식. TS 데스크톱 화면은 `htmlLabels: false`(SVG 글자), 모바일 워커는 HTML 라벨이다
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LabelMode {
    #[default]
    Html,
    SvgText,
}

/// 다이어그램 하나를 그리는 동안 쓰는 글꼴·색·라벨 방식
pub struct RenderContext<'a> {
    pub fonts: &'a FontBook,
    pub palette: &'a Palette,
    pub label_mode: LabelMode,
}

/// 다이어그램 종류. 정의의 첫 문장으로 정한다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramKind {
    Flowchart,
    Pie,
    Sequence,
    State,
    Class,
    Er,
    Gantt,
    Journey,
    GitGraph,
    Mindmap,
    Timeline,
    Quadrant,
    Requirement,
    C4,
}

/// 앞머리·지시문과 `%%` 주석, 빈 줄을 건너뛴 첫 낱말로 종류를 고른다
pub fn detect_kind(definition: &str) -> Result<DiagramKind, DiagramError> {
    let body = directive::diagram_body(definition);
    let first = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    let keyword = first
        .split(|character: char| character.is_whitespace() || character == ';' || character == ':')
        .next()
        .unwrap_or("");
    match keyword {
        "graph" | "flowchart" | "flowchart-v2" => Ok(DiagramKind::Flowchart),
        "pie" => Ok(DiagramKind::Pie),
        "sequenceDiagram" => Ok(DiagramKind::Sequence),
        "stateDiagram" | "stateDiagram-v2" => Ok(DiagramKind::State),
        "classDiagram" | "classDiagram-v2" => Ok(DiagramKind::Class),
        "erDiagram" => Ok(DiagramKind::Er),
        "gantt" => Ok(DiagramKind::Gantt),
        "journey" => Ok(DiagramKind::Journey),
        "gitGraph" => Ok(DiagramKind::GitGraph),
        "mindmap" => Ok(DiagramKind::Mindmap),
        "timeline" => Ok(DiagramKind::Timeline),
        "quadrantChart" => Ok(DiagramKind::Quadrant),
        "requirementDiagram" | "requirement" => Ok(DiagramKind::Requirement),
        "C4Context" | "C4Container" | "C4Component" | "C4Dynamic" | "C4Deployment" => {
            Ok(DiagramKind::C4)
        }
        other => Err(DiagramError::Unsupported(other.to_string())),
    }
}

/// 정의를 테마와 라벨 방식에 맞는 장면으로 만든다
pub fn render_scene(
    definition: &str,
    theme: Theme,
    label_mode: LabelMode,
    fonts: &FontBook,
) -> Result<Scene, DiagramError> {
    let palette = Palette::for_theme(theme);
    let context = RenderContext {
        fonts,
        palette: &palette,
        label_mode,
    };
    match detect_kind(definition)? {
        DiagramKind::Flowchart => flowchart::render(definition, &context),
        DiagramKind::Pie => pie::render(definition, &context),
        DiagramKind::Sequence => sequence::render(definition, &context),
        DiagramKind::State => state::render(definition, &context),
        DiagramKind::Class => class::render(definition, &context),
        DiagramKind::Er => er::render(definition, &context),
        DiagramKind::Gantt => gantt::render(definition, &context),
        DiagramKind::Journey => journey::render(definition, &context),
        DiagramKind::GitGraph => git_graph::render(definition, &context),
        DiagramKind::Mindmap => mindmap::render(definition, &context),
        DiagramKind::Timeline => timeline::render(definition, &context),
        DiagramKind::Quadrant => quadrant::render(definition, &context),
        DiagramKind::Requirement => requirement::render(definition, &context),
        DiagramKind::C4 => c4::render(definition, &context),
    }
}
