//! AI 대화 본문(TS `AiSessionMessageContent`).
//!
//! `kanvibe_markdown::parse_message`가 원문을 Markdown·Mermaid 조각으로 나누고, Markdown 조각은 블록 모델로 바꾼다.
//! 조각 사이는 `space-y-3`(12px)이고 Mermaid 틀은 `my-3`이다. 같은 원문이면 해석 결과를 다시 쓰므로 다시 그려도
//! 글자 요소 id가 그대로라 고른 글자와 다이어그램이 유지된다. 사용자 요청은 해석하지 않고 쓴 그대로 둔다.

mod markdown_inline;
mod markdown_table;
mod markdown_view;
mod mermaid_view;
pub(super) mod selectable_text;

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    AnyElement, App, ClipboardItem, ElementId, Global, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_ai::types::AiMessageRole;
use kanvibe_domain::i18n::Translator;
use kanvibe_markdown::{Block, Segment};

pub(super) use markdown_view::highlighted_code_spans;

use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing};
use markdown_inline::{InlineStyle, styled_spans};
use markdown_view::{MarkdownBlocks, MarkdownContext};
use mermaid_view::MermaidDiagram;
use selectable_text::SelectableRichText;

/// 복사 버튼이 결과를 보여 주는 시간(TS `COPY_FEEDBACK_DURATION_MS`)
pub const COPY_FEEDBACK_DURATION: Duration = Duration::from_millis(1_500);
/// 조각 사이 간격(`space-y-3`)과 Mermaid 틀 위아래 여백(`my-3`)
const SEGMENT_GAP: f32 = 12.0;

/// 코드 블록 복사 버튼 문구. 대화 화면을 열 때 화면 언어로 채운다
#[derive(Debug, Clone)]
pub struct MessageCopyLabels {
    pub copy: SharedString,
    pub copied: SharedString,
}

impl Global for MessageCopyLabels {}

impl MessageCopyLabels {
    pub fn from_translator(translator: &Translator) -> Self {
        Self {
            copy: translator.translate("aiSessions.copyMessage").into(),
            copied: translator.translate("aiSessions.copiedMessage").into(),
        }
    }
}

/// 해석한 조각. Markdown 블록은 다시 그릴 때 복사하지 않게 나눠 쓴다
#[derive(Clone)]
pub enum MessageSegment {
    Markdown(Arc<Vec<Block>>),
    Mermaid(String),
}

/// 원문을 조각으로 나눈다. 비어 있는 Markdown 조각은 그리지 않는다(TS `markup.__html.trim()`이 빈 경우)
pub fn message_segments(content: &str) -> Vec<MessageSegment> {
    kanvibe_markdown::parse_message(content)
        .into_iter()
        .filter_map(|segment| match segment {
            Segment::Markdown(blocks) if blocks.is_empty() => None,
            Segment::Markdown(blocks) => Some(MessageSegment::Markdown(Arc::new(blocks))),
            Segment::Mermaid { definition } => Some(MessageSegment::Mermaid(definition)),
        })
        .collect()
}

/// 같은 원문의 해석 결과를 프레임 사이에 다시 쓴다
struct ParsedMessage {
    content: String,
    segments: Vec<MessageSegment>,
}

/// 대화 본문 하나. `scope`는 화면 안에서 이 본문을 가리키는 고정 키다(대화 행 키 등)
pub fn render_message(
    scope: &str,
    content: &str,
    role: AiMessageRole,
    theme: &ThemeTokens,
) -> AnyElement {
    MessageContent {
        scope: scope.to_string().into(),
        content: content.to_string(),
        role,
        theme: *theme,
    }
    .into_any_element()
}

#[derive(IntoElement)]
struct MessageContent {
    scope: SharedString,
    content: String,
    role: AiMessageRole,
    theme: ThemeTokens,
}

impl RenderOnce for MessageContent {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.role == AiMessageRole::User {
            return self.render_plain().into_any_element();
        }
        let segments = self.parsed_segments(window, cx);
        let labels = cx
            .try_global::<MessageCopyLabels>()
            .cloned()
            .unwrap_or(MessageCopyLabels {
                copy: "Copy".into(),
                copied: "Copied".into(),
            });
        let context = MarkdownContext {
            theme: self.theme,
            labels,
            scope: self.scope.clone(),
        };
        render_segments(&segments, &context).into_any_element()
    }
}

impl MessageContent {
    fn parsed_segments(&self, window: &mut Window, cx: &mut App) -> Vec<MessageSegment> {
        let key = ElementId::Name(format!("{}/parsed", self.scope).into());
        let content = self.content.clone();
        let parsed = window.use_keyed_state(key, cx, |_, _| ParsedMessage {
            segments: message_segments(&content),
            content,
        });
        if parsed.read(cx).content != self.content {
            let content = self.content.clone();
            parsed.update(cx, |parsed, _| {
                parsed.segments = message_segments(&content);
                parsed.content = content;
            });
        }
        parsed.read(cx).segments.clone()
    }

    /// `<p class="whitespace-pre-wrap break-words">`
    fn render_plain(&self) -> impl IntoElement {
        let style = InlineStyle::plain(self.theme.text.primary);
        let spans = styled_spans(&self.content, &style);
        SelectableRichText::new(
            ElementId::Name(format!("{}/plain", self.scope).into()),
            spans,
            self.theme.brand.muted.into(),
        )
    }
}

/// 조각 사이 12px. Mermaid가 맨 앞·맨 뒤면 그 `my-3`이 본문 바깥 여백으로 남는다
fn render_segments(segments: &[MessageSegment], context: &MarkdownContext) -> gpui_kit::Div {
    let mut column = div().flex().flex_col().min_w_0();
    let last = segments.len().saturating_sub(1);
    for (index, segment) in segments.iter().enumerate() {
        let is_mermaid = matches!(segment, MessageSegment::Mermaid(_));
        let top = match (index, is_mermaid) {
            (0, false) => 0.0,
            _ => SEGMENT_GAP,
        };
        let element = match segment {
            MessageSegment::Markdown(blocks) => MarkdownBlocks {
                blocks: blocks.clone(),
                context: context.clone(),
                path: format!("s{index}"),
            }
            .into_any_element(),
            MessageSegment::Mermaid(definition) => MermaidDiagram {
                scope: context.scope.clone(),
                index,
                definition: definition.clone(),
                theme: context.theme,
            }
            .into_any_element(),
        };
        // 마지막 Mermaid의 `my-3`은 뒤따르는 메타 줄의 `mt-1`(4px)과 겹친다
        let bottom = if is_mermaid && index == last {
            SEGMENT_GAP - 4.0
        } else {
            0.0
        };
        column = column.child(div().mt(px(top)).mb(px(bottom)).min_w_0().child(element));
    }
    column
}

/// 코드 블록 복사 버튼. 누르면 그 블록의 코드만 복사하고 잠시 결과를 보인다
#[derive(IntoElement)]
pub struct CodeCopyButton {
    pub id: ElementId,
    pub code: String,
    pub labels: MessageCopyLabels,
    pub theme: ThemeTokens,
}

impl RenderOnce for CodeCopyButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let is_copied = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let label = match *is_copied.read(cx) {
            true => self.labels.copied.clone(),
            false => self.labels.copy.clone(),
        };
        let hover_background = self.theme.background.surface;
        let hover_text = self.theme.text.primary;
        let code = self.code;
        div()
            .id(self.id)
            .relative()
            .min_h(px(28.0))
            .flex()
            .items_center()
            .rounded(radius::BASE)
            .px(spacing(2.0))
            .py(spacing(1.0))
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background).text_color(hover_text))
            .child(label.clone())
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.clone(),
                text: label,
            }))
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                is_copied.update(cx, |state, cx| {
                    *state = true;
                    cx.notify();
                });
                let is_copied = is_copied.downgrade();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(COPY_FEEDBACK_DURATION).await;
                    let _ = is_copied.update(cx, |state, cx| {
                        *state = false;
                        cx.notify();
                    });
                })
                .detach();
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_markdown::Inline;

    fn code_blocks(segments: &[MessageSegment]) -> Vec<(String, String)> {
        segments
            .iter()
            .filter_map(|segment| match segment {
                MessageSegment::Markdown(blocks) => Some(blocks.iter()),
                MessageSegment::Mermaid(_) => None,
            })
            .flatten()
            .filter_map(|block| match block {
                Block::CodeBlock(code) => Some((code.header_label.clone(), code.code.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn code_block_copy_text_is_the_code_without_its_header() {
        let segments = message_segments("설명\n\n```ts\nconst a = 1;\nconst b = 2;\n```\n끝");
        assert_eq!(
            code_blocks(&segments),
            [("ts".to_string(), "const a = 1;\nconst b = 2;\n".to_string())]
        );
    }

    #[test]
    fn unclosed_code_block_is_code_until_the_end() {
        let segments = message_segments("앞\n```\nlet x = 1;\n```not-a-close\nlet y = 2;");
        let blocks = code_blocks(&segments);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, "text");
        assert!(blocks[0].1.contains("```not-a-close\nlet y = 2;"));
    }

    #[test]
    fn changed_content_produces_the_new_blocks() {
        let paragraph = |segments: &[MessageSegment]| match &segments[0] {
            MessageSegment::Markdown(blocks) => match &blocks[0] {
                Block::Paragraph(inlines) => Inline::plain_text(inlines),
                _ => String::new(),
            },
            MessageSegment::Mermaid(_) => String::new(),
        };
        assert_eq!(paragraph(&message_segments("첫 답변")), "첫 답변");
        assert_eq!(paragraph(&message_segments("바뀐 답변")), "바뀐 답변");
    }

    struct MessageTestView {
        content: String,
    }

    impl gpui_kit::Render for MessageTestView {
        fn render(
            &mut self,
            _window: &mut Window,
            cx: &mut gpui_kit::Context<Self>,
        ) -> impl IntoElement {
            let theme = *cx.global::<ThemeTokens>();
            div()
                .size_full()
                .child(gpui_kit::base::TextSelectionLayer)
                .child(div().w(px(600.0)).child(render_message(
                    "row-1",
                    &self.content,
                    AiMessageRole::Assistant,
                    &theme,
                )))
        }
    }

    fn open_message<'a>(
        cx: &'a mut gpui_kit::TestAppContext,
        content: &str,
    ) -> (
        gpui_kit::Entity<MessageTestView>,
        &'a mut gpui_kit::VisualTestContext,
    ) {
        let launch = crate::qa::options::LaunchOptions {
            data_dir: None,
            initial_route: "/ko".into(),
            window_size: crate::qa::options::DEFAULT_WINDOW_SIZE,
            qa: None,
        };
        cx.update(|cx| crate::init_app(&launch, cx).expect("app init"));
        let content = content.to_string();
        let (view, cx) = cx.add_window_view(move |_, _| MessageTestView { content });
        cx.run_until_parked();
        (view, cx)
    }

    #[gpui_kit::test]
    fn selected_text_survives_a_redraw_with_the_same_content(cx: &mut gpui_kit::TestAppContext) {
        let (view, cx) = open_message(cx, "고를 수 있는 답변 본문");
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let modifiers = gpui_kit::Modifiers::default();
        cx.simulate_mouse_down(
            gpui_kit::point(px(1.0), px(10.0)),
            gpui_kit::MouseButton::Left,
            modifiers,
        );
        cx.simulate_mouse_move(
            gpui_kit::point(px(590.0), px(10.0)),
            Some(gpui_kit::MouseButton::Left),
            modifiers,
        );
        cx.simulate_mouse_up(
            gpui_kit::point(px(590.0), px(10.0)),
            gpui_kit::MouseButton::Left,
            modifiers,
        );
        let selected = cx.update(|window, cx| {
            let _ = window.draw(cx);
            gpui_kit::base::TextSelection::selected_text(window, cx)
        });
        assert_eq!(selected, "고를 수 있는 답변 본문");
        view.update(cx, |view, cx| {
            view.content = "고를 수 있는 답변 본문".to_string();
            cx.notify();
        });
        let after = cx.update(|window, cx| {
            let _ = window.draw(cx);
            gpui_kit::base::TextSelection::selected_text(window, cx)
        });
        assert_eq!(after, selected);
    }

    #[test]
    fn a_redraw_with_the_same_diagram_reuses_its_rendered_state() {
        let content = "```mermaid\ngraph TD\nA[보드] --> B[필터]\n```";
        let first = message_segments(content);
        let second = message_segments(content);
        let definition = |segments: &[MessageSegment]| match &segments[0] {
            MessageSegment::Mermaid(definition) => definition.clone(),
            MessageSegment::Markdown(_) => String::new(),
        };
        // 같은 정의는 같은 상태 키를 찾아 이미 그린 그림을 그대로 쓴다(자리표시로 돌아가지 않는다)
        let theme = ThemeTokens::for_mode(crate::theme::ThemeMode::Dark);
        let key = |definition: String| {
            mermaid_view::MermaidDiagram {
                scope: "row-1".into(),
                index: 0,
                definition,
                theme,
            }
            .state_id()
        };
        assert_eq!(key(definition(&first)), key(definition(&second)));
        assert_ne!(key(definition(&first)), key("graph TD\nA --> C".into()));
    }

    #[test]
    fn mermaid_fences_become_their_own_segments() {
        let segments = message_segments("앞\n```mermaid\ngraph TD\nA-->B\n```\n뒤");
        assert!(
            matches!(&segments[1], MessageSegment::Mermaid(definition) if definition == "graph TD\nA-->B")
        );
        assert_eq!(segments.len(), 3);
    }
}
