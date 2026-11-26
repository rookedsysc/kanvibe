//! 대화 본문의 Mermaid 다이어그램(TS `MermaidDiagram`).
//!
//! TS는 mermaid 모듈을 비동기로 불러 그리는 동안 `Rendering diagram…`을 보였다. 대화를 열면 이 자리표시가 보이는
//! 상태로 맨 아래까지 스크롤한 뒤 그림이 커지므로, 읽던 위치는 그림 위쪽에 머문다. 여기서도 같은 순서를 지키려고
//! 백그라운드에서 `kanvibe-diagram`으로 장면을 만들고 래스터화한 뒤 다음 프레임에 바꿔 끼운다.
//! 그림은 viewBox 원래 너비로 그리고(TS `fixSvgToNaturalWidth`) 넘치면 틀 안에서만 옆으로 넘긴다.
//! 그리지 못하면 정의 원문을 코드 블록으로 보인다. 글꼴은 런타임의 다이어그램 렌더러와 같은 묶음을 쓴다.

use std::sync::Arc;

use gpui_kit::{
    App, AppContext, Context, ElementId, Image, ImageFormat, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Task, Window, div,
    img, px,
};
use kanvibe_diagram::raster::{Placement, draw_scene};
use kanvibe_diagram::text::FontBook;
use kanvibe_diagram::theme::Theme as DiagramTheme;
use resvg::tiny_skia::Pixmap;

use super::selectable_text::{RichTextSpans, SelectableRichText};
use crate::components::TailwindText;
use crate::fonts::mono_font;
use crate::services::AppServices;
use crate::theme::{ThemeMode, ThemeTokens, radius, spacing, text_scale};

/// TS 자리표시 문구(번역 없이 영어 그대로)
pub const RENDERING_DIAGRAM_LABEL: &str = "Rendering diagram…";

/// 래스터화한 다이어그램. 크기는 viewBox 원래 크기(논리 px)다
#[derive(Clone)]
pub struct RasterizedDiagram {
    pub image: Arc<Image>,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone)]
pub enum DiagramRender {
    Pending,
    Ready(RasterizedDiagram),
    Failed,
}

pub struct MermaidDiagramState {
    pub render: DiagramRender,
    _task: Task<()>,
}

impl MermaidDiagramState {
    /// 백그라운드에서 그리기 시작한다. 결과는 다음 갱신 때 끼운다
    pub fn start(
        definition: String,
        theme: DiagramTheme,
        scale_factor: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        let renderer = cx
            .global::<AppServices>()
            .runtime()
            .map(|runtime| Arc::clone(runtime.diagram_renderer()));
        let rasterize = cx.background_spawn(async move {
            let fonts = renderer.as_deref()?.fonts()?;
            rasterize_diagram(&definition, theme, scale_factor, fonts)
        });
        let task = cx.spawn(async move |this, cx| {
            let result = rasterize.await;
            let _ = this.update(cx, |state, cx| {
                state.render = match result {
                    Some(diagram) => DiagramRender::Ready(diagram),
                    None => DiagramRender::Failed,
                };
                cx.notify();
            });
        });
        Self {
            render: DiagramRender::Pending,
            _task: task,
        }
    }
}

/// 앱 테마가 라이트면 Mermaid `default`, 아니면 `dark`
pub fn diagram_theme(theme: &ThemeTokens) -> DiagramTheme {
    match theme.mode {
        ThemeMode::Light => DiagramTheme::Light,
        _ => DiagramTheme::Dark,
    }
}

/// 정의를 장면으로 만들고 화면 배율로 래스터화한다. 지원하지 않거나 틀린 정의면 `None`
pub fn rasterize_diagram(
    definition: &str,
    theme: DiagramTheme,
    scale_factor: f32,
    fonts: &FontBook,
) -> Option<RasterizedDiagram> {
    let scene = kanvibe_markdown::render_mermaid(definition, theme, fonts).ok()?;
    let scale = f64::from(scale_factor.max(1.0));
    let pixel_width = (scene.width * scale).ceil().max(1.0) as u32;
    let pixel_height = (scene.height * scale).ceil().max(1.0) as u32;
    let mut pixmap = Pixmap::new(pixel_width, pixel_height)?;
    let placement = Placement {
        scale,
        offset_x: 0.0,
        offset_y: 0.0,
    };
    draw_scene(&mut pixmap, &scene, fonts, placement);
    let png = pixmap.encode_png().ok()?;
    Some(RasterizedDiagram {
        image: Arc::new(Image::from_bytes(ImageFormat::Png, png)),
        width: scene.width as f32,
        height: scene.height as f32,
    })
}

/// FNV-1a. 같은 정의는 같은 상태를 다시 찾는다(TS `createMermaidElementId`의 해시 자리)
fn definition_hash(definition: &str) -> u64 {
    definition
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

#[derive(IntoElement)]
pub struct MermaidDiagram {
    pub scope: SharedString,
    pub index: usize,
    pub definition: String,
    pub theme: ThemeTokens,
}

impl MermaidDiagram {
    pub(super) fn state_id(&self) -> ElementId {
        let hash = definition_hash(&self.definition);
        ElementId::Name(format!("{}/mermaid-{}-{hash:x}", self.scope, self.index).into())
    }
}

impl RenderOnce for MermaidDiagram {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state_id = self.state_id();
        let definition = self.definition.clone();
        let theme = diagram_theme(&self.theme);
        let state = window.use_keyed_state(state_id.clone(), cx, |window, cx| {
            MermaidDiagramState::start(definition, theme, window.scale_factor(), cx)
        });
        let render = state.read(cx).render.clone();
        let tokens = self.theme;
        match render {
            DiagramRender::Failed => render_fallback(&self, &tokens).into_any_element(),
            DiagramRender::Pending => diagram_frame(&self, &tokens)
                .child(
                    div()
                        .text_scale(text_scale::XS)
                        .text_color(tokens.text.muted)
                        .child(RENDERING_DIAGRAM_LABEL),
                )
                .into_any_element(),
            DiagramRender::Ready(diagram) => diagram_frame(&self, &tokens)
                .child(
                    img(diagram.image)
                        .flex_none()
                        .w(px(diagram.width))
                        .h(px(diagram.height)),
                )
                .into_any_element(),
        }
    }
}

/// `my-3 overflow-x-auto rounded-lg border border-border-subtle bg-bg-page p-3`
fn diagram_frame(
    diagram: &MermaidDiagram,
    theme: &ThemeTokens,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(ElementId::Name(
            format!("{}/mermaid-frame-{}", diagram.scope, diagram.index).into(),
        ))
        .flex()
        .overflow_x_scroll()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.subtle)
        .bg(theme.background.page)
        .p(spacing(3.0))
}

/// 그리지 못한 정의를 그대로 보이는 코드 블록(`ai-session-mermaid-fallback`)
fn render_fallback(diagram: &MermaidDiagram, theme: &ThemeTokens) -> impl IntoElement {
    let mut spans = RichTextSpans::default();
    let run = gpui_kit::TextRun {
        len: 0,
        font: mono_font(400),
        color: theme.text.primary.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    spans.push(&diagram.definition, &run);
    diagram_frame(diagram, theme)
        .text_scale(text_scale::XS)
        .whitespace_nowrap()
        .child(SelectableRichText::new(
            ElementId::Name(format!("{}/mermaid-code-{}", diagram.scope, diagram.index).into()),
            spans,
            theme.brand.muted.into(),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::BUNDLED_FONT_FILES;

    /// 앱이 런타임에 넘기는 것과 같은 번들 글꼴 묶음
    fn bundled_fonts() -> FontBook {
        let files = BUNDLED_FONT_FILES
            .iter()
            .map(|bytes| bytes.to_vec())
            .collect();
        FontBook::from_font_files(files).expect("글꼴")
    }

    #[test]
    fn wide_diagrams_keep_their_natural_width_instead_of_shrinking_to_the_column() {
        let definition = "graph LR\n  A[아주 긴 첫 번째 단계 이름] --> B[아주 긴 두 번째 단계 이름] --> C[아주 긴 세 번째 단계 이름] --> D[아주 긴 네 번째 단계 이름] --> E[아주 긴 다섯 번째 단계 이름]";
        let fonts = bundled_fonts();
        let diagram = rasterize_diagram(definition, DiagramTheme::Dark, 1.0, &fonts).expect("그림");
        let scene =
            kanvibe_markdown::render_mermaid(definition, DiagramTheme::Dark, &fonts).expect("장면");
        // 대화 칸(최대 768px)보다 넓어도 viewBox 너비 그대로 그려 틀 안에서 옆으로 넘긴다
        assert!(diagram.width > 768.0);
        assert_eq!(diagram.width, scene.width as f32);
        assert_eq!(diagram.height, scene.height as f32);
    }

    #[test]
    fn unknown_diagrams_fall_back_to_their_source() {
        let fonts = bundled_fonts();
        assert!(rasterize_diagram("notADiagram\n A B", DiagramTheme::Light, 1.0, &fonts).is_none());
    }
}
