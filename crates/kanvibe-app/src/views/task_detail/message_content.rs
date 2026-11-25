//! AI 대화 본문(TS `AiSessionMessageContent`)의 임시 그리기.
//!
//! Markdown 해석은 `kanvibe-markdown`이 맡는다. 앱이 그 crate에 의존하기 전까지는 본문을 줄글로 두고 울타리 코드 블록만
//! 언어 머리줄·복사 버튼이 달린 고정폭 블록으로 그린다. 바꿀 때는 [`render_message`]의 몸통만
//! `kanvibe_markdown::parse_message`의 `Segment` 목록을 그리도록 고치면 된다.

use std::time::Duration;

use gpui_kit::{
    AnyElement, App, ClipboardItem, ElementId, Global, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_ai::types::AiMessageRole;
use kanvibe_domain::i18n::Translator;

use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 복사 버튼이 결과를 보여 주는 시간(TS `COPY_FEEDBACK_DURATION_MS`)
const COPY_FEEDBACK_DURATION: Duration = Duration::from_millis(1_500);
/// 언어 없는 코드 블록 머리줄 이름(`kanvibe_markdown::PLAIN_CODE_LABEL`)
const PLAIN_CODE_LABEL: &str = "text";

/// 본문 한 덩어리
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageBlock {
    Text(String),
    /// `code`가 복사 버튼이 옮기는 글자다(머리줄 제외)
    Code {
        header_label: String,
        code: String,
    },
}

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

fn fence_info(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    (indent <= 3)
        .then(|| line.trim_start_matches(' ').strip_prefix("```"))
        .flatten()
}

fn push_text(blocks: &mut Vec<MessageBlock>, lines: &mut Vec<&str>) {
    let text = lines.join("\n");
    let trimmed = text.trim_matches('\n');
    if !trimmed.trim().is_empty() {
        blocks.push(MessageBlock::Text(trimmed.to_string()));
    }
    lines.clear();
}

/// 울타리 코드 블록과 나머지 글로 나눈다. 닫히지 않은 코드 블록은 끝까지 코드다
pub fn split_message_blocks(content: &str) -> Vec<MessageBlock> {
    let mut blocks = Vec::new();
    let mut text_lines: Vec<&str> = Vec::new();
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        let Some(info) = fence_info(line) else {
            text_lines.push(line);
            continue;
        };
        push_text(&mut blocks, &mut text_lines);
        let language = info.split_whitespace().next().unwrap_or_default();
        let code_lines: Vec<&str> = lines
            .by_ref()
            .take_while(|line| fence_info(line).is_none_or(|rest| !rest.trim().is_empty()))
            .collect();
        blocks.push(MessageBlock::Code {
            header_label: if language.is_empty() {
                PLAIN_CODE_LABEL.to_string()
            } else {
                language.to_string()
            },
            code: code_lines.join("\n"),
        });
    }
    push_text(&mut blocks, &mut text_lines);
    blocks
}

/// 대화 본문 하나. 사용자 요청은 해석하지 않고 쓴 그대로 둔다
pub fn render_message(
    content: &str,
    role: AiMessageRole,
    theme: &ThemeTokens,
    cx: &mut App,
) -> AnyElement {
    if role == AiMessageRole::User {
        return div().child(content.to_string()).into_any_element();
    }
    let labels = cx
        .try_global::<MessageCopyLabels>()
        .cloned()
        .unwrap_or(MessageCopyLabels {
            copy: "Copy".into(),
            copied: "Copied".into(),
        });
    let content_hash = stable_hash(content);
    div()
        .flex()
        .flex_col()
        .gap(spacing(3.0))
        .text_scale(text_scale::SM)
        .line_height(px(24.0))
        .children(
            split_message_blocks(content).into_iter().enumerate().map(
                |(index, block)| match block {
                    MessageBlock::Text(text) => div().child(text).into_any_element(),
                    MessageBlock::Code { header_label, code } => {
                        let copy = CodeCopyButton {
                            id: ElementId::from((
                                SharedString::from(format!("code-copy-{content_hash:x}")),
                                index,
                            )),
                            code: code.clone(),
                            labels: labels.clone(),
                            theme: *theme,
                        };
                        render_code_block(theme, header_label, code, copy)
                    }
                },
            ),
        )
        .into_any_element()
}

/// FNV-1a. 같은 내용의 같은 블록이 같은 복사 상태를 다시 찾게 한다
fn stable_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `kv-chat-code`: 언어 이름과 복사 버튼이 놓인 머리줄 + 옆으로 넘기는 코드
fn render_code_block(
    theme: &ThemeTokens,
    header_label: String,
    code: String,
    copy: CodeCopyButton,
) -> AnyElement {
    div()
        .overflow_hidden()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.subtle)
        .bg(theme.background.page)
        .child(
            div()
                .flex()
                .min_h(px(32.0))
                .items_center()
                .justify_between()
                .pl(spacing(3.0))
                .pr(spacing(1.0))
                .border_b_1()
                .border_color(theme.border.subtle)
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(header_label)
                .child(copy),
        )
        .child(
            div()
                .p(spacing(3.0))
                .text_size(px(12.04))
                .line_height(px(24.0))
                .font(crate::fonts::mono_font(400))
                .text_color(theme.text.primary)
                .whitespace_nowrap()
                .child(code),
        )
        .into_any_element()
}

/// 코드 블록 복사 버튼. 누르면 그 블록의 코드만 복사하고 잠시 결과를 보인다
#[derive(IntoElement)]
struct CodeCopyButton {
    id: ElementId,
    code: String,
    labels: MessageCopyLabels,
    theme: ThemeTokens,
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
            .rounded(radius::SM)
            .px(spacing(2.0))
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

    fn code(label: &str, code: &str) -> MessageBlock {
        MessageBlock::Code {
            header_label: label.into(),
            code: code.into(),
        }
    }

    // parity: TS-renderer-AiSessionMessageContent-0ce8d8
    #[test]
    fn code_block_copy_text_is_the_code_without_its_header() {
        let blocks = split_message_blocks("설명\n\n```ts\nconst a = 1;\nconst b = 2;\n```\n끝");
        assert_eq!(
            blocks,
            [
                MessageBlock::Text("설명".into()),
                code("ts", "const a = 1;\nconst b = 2;"),
                MessageBlock::Text("끝".into()),
            ]
        );
    }

    // parity: TS-renderer-AiSessionMessageContent-86db00
    #[test]
    fn unclosed_code_block_is_code_until_the_end() {
        assert_eq!(
            split_message_blocks("앞\n```\nlet x = 1;\n```not-a-close\nlet y = 2;"),
            [
                MessageBlock::Text("앞".into()),
                code("text", "let x = 1;\n```not-a-close\nlet y = 2;")
            ]
        );
    }

    // parity: TS-renderer-AiSessionMessageContent-ac9103
    #[test]
    fn changed_content_produces_the_new_blocks() {
        assert_eq!(
            split_message_blocks("첫 답변"),
            [MessageBlock::Text("첫 답변".into())]
        );
        assert_eq!(
            split_message_blocks("바뀐 답변"),
            [MessageBlock::Text("바뀐 답변".into())]
        );
        assert_ne!(stable_hash("첫 답변"), stable_hash("바뀐 답변"));
    }
}
