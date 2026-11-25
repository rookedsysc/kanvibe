//! 대화 기록 메시지 한 줄(TS `AiChatTranscriptMessage`).
//!
//! 도구·추론 메시지는 접힌 상태로 두고 머리줄을 눌러 펼친다. 사용자 요청은 쓴 그대로, 나머지는 본문 그리기로 보인다.
//! 메시지마다 원문 전체를 복사하는 버튼과 시각을 붙이고, AI가 만들거나 고친 파일은 미리보기를 여는 카드로 보인다.

mod attachment;

use gpui_kit::{
    AnyElement, ClipboardItem, Context, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{AggregatedAiMessage, AiFileChange, AiMessageRole};
use kanvibe_domain::i18n::Translator;

use super::ai_provider_icon::ai_provider_icon;
use super::chat_history::TranscriptRow;
use super::chat_view::InlineAiChatView;
use super::message_content::render_message;
use crate::components::TailwindText;
use crate::locale_format::format_time_two_digit_hm;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::local_time::LocalDateTime;
use attachment::{AttachmentContext, AttachmentLabels, ChatAttachment};

/// 메시지 모양
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageShape {
    /// 접어 두는 도구·추론 줄. `summary`는 도구 이름(없으면 역할 이름), `first_line`은 본문 첫 줄
    Disclosure { summary: String, first_line: String },
    /// 오른쪽 말풍선(사용자 요청)
    UserBubble,
    /// 폭 전체를 쓰는 본문
    Body,
}

/// AI가 바꾼 파일 카드
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCard {
    pub path: String,
    pub name: String,
    pub change_label: String,
}

pub fn role_label(translator: &Translator, role: AiMessageRole) -> String {
    let key = serde_json::to_value(role)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    translator.translate(&format!("aiSessions.roles.{key}"))
}

/// 복사·펼침에 쓰는 원문(TS `fullText || text`)
pub fn message_text(message: &AggregatedAiMessage) -> &str {
    if message.full_text.is_empty() {
        &message.text
    } else {
        &message.full_text
    }
}

pub fn message_shape(translator: &Translator, message: &AggregatedAiMessage) -> MessageShape {
    match message.role {
        AiMessageRole::Tool | AiMessageRole::Reasoning => MessageShape::Disclosure {
            summary: message
                .tool_name
                .clone()
                .unwrap_or_else(|| role_label(translator, message.role)),
            first_line: message_text(message)
                .split('\n')
                .next()
                .unwrap_or_default()
                .to_string(),
        },
        AiMessageRole::User => MessageShape::UserBubble,
        _ => MessageShape::Body,
    }
}

/// 메타 줄의 이름. 사용자 요청은 이름이 없고, 답변은 provider 이름, 나머지는 역할 이름이다
pub fn meta_name(
    translator: &Translator,
    role: AiMessageRole,
    provider: AiSessionProvider,
) -> Option<String> {
    match role {
        AiMessageRole::User => None,
        AiMessageRole::Assistant => Some(provider.label().to_string()),
        other => Some(role_label(translator, other)),
    }
}

/// `toLocaleTimeString([], {hour: "2-digit", minute: "2-digit"})`: en-US는 `02:45 AM`
pub fn message_time(timestamp: Option<&str>) -> Option<String> {
    let epoch_ms = kanvibe_ai::javascript::parse_date_ms(timestamp?)?;
    format_time_two_digit_hm(LocalDateTime::from_epoch_ms(epoch_ms)?)
}

/// 원문이 있을 때만 메시지 복사 버튼을 둔다(그림만 있는 메시지에는 빈 복사가 없다)
pub fn has_copy_action(message: &AggregatedAiMessage) -> bool {
    !message_text(message).is_empty()
}

/// 미리보기를 열 수 있는 화면에서만 카드를 그린다
pub fn file_cards(
    translator: &Translator,
    message: &AggregatedAiMessage,
    can_open_file: bool,
) -> Vec<FileCard> {
    if !can_open_file {
        return Vec::new();
    }
    message
        .files
        .iter()
        .flatten()
        .map(|file| FileCard {
            path: file.path.clone(),
            name: file
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&file.path)
                .to_string(),
            change_label: translator.translate(match file.change {
                AiFileChange::Created => "aiSessions.fileChange.created",
                AiFileChange::Modified => "aiSessions.fileChange.modified",
            }),
        })
        .collect()
}

impl InlineAiChatView {
    pub(super) fn render_transcript_row(
        &self,
        theme: &ThemeTokens,
        row: &TranscriptRow<'_>,
        provider: AiSessionProvider,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let message = row.message;
        let cards = file_cards(&self.translator, message, true);
        let cards = (!cards.is_empty()).then(|| self.render_file_cards(theme, &row.key, cards, cx));
        let meta = self.render_meta(theme, row, provider, cx);
        match message_shape(&self.translator, message) {
            MessageShape::Disclosure {
                summary,
                first_line,
            } => {
                let disclosure = self.render_disclosure(theme, row, summary, first_line, meta, cx);
                div().child(disclosure).children(cards).into_any_element()
            }
            MessageShape::UserBubble => {
                render_user_bubble(theme, self.render_message_body(theme, row), cards, meta)
            }
            MessageShape::Body => {
                let body = self.render_message_body(theme, row);
                div()
                    .min_w_0()
                    .w_full()
                    .py(spacing(1.0))
                    .text_scale(text_scale::SM)
                    .line_height(px(28.0))
                    .text_color(theme.text.primary)
                    .child(body)
                    .children(cards)
                    .child(meta)
                    .into_any_element()
            }
        }
    }

    /// 본문(원문이 있을 때만)과 첨부. TS는 원문 없는 그림 메시지에 본문을 그리지 않는다
    fn render_message_body(&self, theme: &ThemeTokens, row: &TranscriptRow<'_>) -> AnyElement {
        let message = row.message;
        let text = message_text(message);
        let content =
            (!text.is_empty()).then(|| render_message(&row.key, text, message.role, theme));
        let context = self
            .state
            .selected_session()
            .map(|session| AttachmentContext {
                task_id: self.task_id.clone(),
                provider: session.provider,
                session_id: session.id.clone(),
                source_ref: session.source_ref.clone(),
            });
        let labels = AttachmentLabels::from_translator(&self.translator);
        let attachments =
            message
                .attachment_list()
                .iter()
                .enumerate()
                .map(|(index, attachment)| ChatAttachment {
                    key: format!("{}:{index}", row.key),
                    attachment: attachment.clone(),
                    context: context.clone(),
                    labels: labels.clone(),
                    theme: *theme,
                });
        div()
            .min_w_0()
            .children(content)
            .children(attachments)
            .into_any_element()
    }

    /// 접힌 도구·추론 줄. 펼치면 본문과 메타 줄이 머리줄 아래에 붙는다
    fn render_disclosure(
        &self,
        theme: &ThemeTokens,
        row: &TranscriptRow<'_>,
        summary: String,
        first_line: String,
        meta: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_open = self.open_disclosures.contains(&row.key);
        let header = self.render_disclosure_header(theme, &row.key, summary, first_line, cx);
        let mut frame = div()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.surface)
            .px(spacing(4.0))
            .py(spacing(2.0))
            .child(header);
        if is_open {
            let body = self.render_message_body(theme, row);
            frame = frame.child(
                div()
                    .mt(spacing(2.0))
                    .min_w_0()
                    .pb(spacing(2.0))
                    .text_scale(text_scale::SM)
                    .line_height(px(24.0))
                    .text_color(theme.text.primary)
                    .child(body)
                    .child(meta),
            );
        }
        frame.into_any_element()
    }

    /// `<summary>`: 펼침 표시, 도구 이름, 본문 첫 줄, 펼치기/접기.
    /// `inline-block align-bottom` 첫 줄 조각 때문에 TS 줄 상자가 1px 커서 아래 여백을 1px 더 둔다
    fn render_disclosure_header(
        &self,
        theme: &ThemeTokens,
        row_key: &str,
        summary: String,
        first_line: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_open = self.open_disclosures.contains(row_key);
        let toggle_label = self.translator.translate(if is_open {
            "aiSessions.collapseTool"
        } else {
            "aiSessions.expandTool"
        });
        let key = row_key.to_string();
        div()
            .id(SharedString::from(format!("disclosure-{row_key}")))
            .relative()
            .flex()
            .items_center()
            .pt(spacing(3.0))
            .pb(px(13.0))
            .text_scale(text_scale::XS)
            .css_weight(500)
            .text_color(theme.text.secondary)
            .cursor_pointer()
            .child(disclosure_marker(is_open))
            .child(div().flex_none().child(summary.clone()))
            .child(
                div()
                    .ml(spacing(2.0))
                    .max_w(gpui_kit::relative(0.55))
                    .truncate()
                    .css_weight(400)
                    .child(first_line),
            )
            .child(div().flex_1())
            .child(div().flex_none().ml(spacing(2.0)).child(toggle_label))
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: summary.clone().into(),
                text: summary.into(),
            }))
            .on_click(cx.listener(move |view, _, _, cx| view.toggle_disclosure(&key, cx)))
    }

    fn render_meta(
        &self,
        theme: &ThemeTokens,
        row: &TranscriptRow<'_>,
        provider: AiSessionProvider,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let message = row.message;
        let name = meta_name(&self.translator, message.role, provider);
        let text = message_text(message).to_string();
        let copy =
            has_copy_action(message).then(|| self.render_copy_message(theme, &row.key, text, cx));
        div()
            .mt(spacing(1.0))
            .flex()
            .flex_wrap()
            .min_h(px(28.0))
            .items_center()
            .gap_x(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .children(name.is_some().then(|| ai_provider_icon(provider, 16.0)))
            .children(name.map(|name| div().css_weight(500).child(name)))
            .children(message_time(message.timestamp.as_deref()).map(|time| div().child(time)))
            .children(copy)
            .into_any_element()
    }

    /// 메시지 원문 복사. 한 번 복사하면 `복사됨`으로 남는다
    fn render_copy_message(
        &self,
        theme: &ThemeTokens,
        row_key: &str,
        text: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_copied = self.copied_messages.contains(row_key);
        let label = self.translator.translate(if is_copied {
            "aiSessions.copiedMessage"
        } else {
            "aiSessions.copyMessage"
        });
        let key = row_key.to_string();
        let hover_background = theme.background.surface;
        let hover_text = theme.text.primary;
        div()
            .id(SharedString::from(format!("copy-{row_key}")))
            .relative()
            .min_h(px(28.0))
            .flex()
            .items_center()
            .rounded(radius::SM)
            .px(spacing(2.0))
            .py(spacing(1.0))
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background).text_color(hover_text))
            .child(label.clone())
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.clone().into(),
                text: label.into(),
            }))
            .on_click(cx.listener(move |view, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                view.copied_messages.insert(key.clone());
                cx.notify();
            }))
    }

    fn render_file_cards(
        &self,
        theme: &ThemeTokens,
        row_key: &str,
        cards: Vec<FileCard>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .mt(spacing(2.0))
            .flex()
            .flex_wrap()
            .gap(spacing(2.0))
            .children(cards.into_iter().enumerate().map(|(index, card)| {
                let path = card.path.clone();
                render_file_card(theme, format!("file-card-{row_key}-{index}"), card).on_click(
                    cx.listener(move |view, _, _, cx| view.open_file_preview(path.clone(), cx)),
                )
            }))
            .into_any_element()
    }
}

/// 접힘 삼각형(`<details>` 표시)
fn disclosure_marker(is_open: bool) -> impl IntoElement {
    div()
        .flex_none()
        .w(px(12.0))
        .text_size(px(8.0))
        .child(if is_open { "▼" } else { "▶" })
}

/// AI가 바꾼 파일 카드: 파일 이름과 바뀐 방식
fn render_file_card(
    theme: &ThemeTokens,
    id: String,
    card: FileCard,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let hover = theme.background.page;
    div()
        .id(SharedString::from(id))
        .relative()
        .flex()
        .min_h(px(44.0))
        .items_center()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .px(spacing(3.0))
        .py(spacing(2.0))
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .child(
            div()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_scale(text_scale::SM)
                        .css_weight(500)
                        .text_color(theme.text.primary)
                        .child(card.name.clone()),
                )
                .child(
                    div()
                        .text_scale(text_scale::XS)
                        .text_color(theme.text.secondary)
                        .child(card.change_label),
                ),
        )
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: card.path.into(),
            text: card.name.into(),
        }))
}

/// 오른쪽 말풍선. 사용자 요청은 해석하지 않고 줄바꿈까지 쓴 그대로 보인다
fn render_user_bubble(
    theme: &ThemeTokens,
    body: AnyElement,
    cards: Option<AnyElement>,
    meta: AnyElement,
) -> AnyElement {
    let bubble = div()
        .min_w_0()
        .max_w(gpui_kit::relative(0.8))
        .rounded_tl(px(16.0))
        .rounded_tr(px(16.0))
        .rounded_bl(px(16.0))
        .rounded_br(radius::MD)
        .bg(theme.background.surface)
        .px(spacing(4.0))
        .py(spacing(2.0))
        .text_scale(text_scale::SM)
        .line_height(px(24.0))
        .text_color(theme.text.primary)
        .child(body)
        .children(cards)
        .child(meta);
    div()
        .flex()
        .min_w_0()
        .justify_end()
        .child(bubble)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_ai::types::AiMessageFile;
    use kanvibe_domain::i18n::Locale;

    fn translator() -> Translator {
        Translator::new(Locale::Ko, "taskDetail")
    }

    fn tool(name: Option<&str>, text: &str) -> AggregatedAiMessage {
        let mut message = AggregatedAiMessage::plain(AiMessageRole::Tool, None, text);
        message.tool_name = name.map(str::to_string);
        message
    }

    // parity: TS-renderer-AiChatTranscriptMessage-bae52b
    #[test]
    fn tool_output_is_folded_behind_its_first_line() {
        let mut message = tool(Some("Bash"), "preview");
        message.full_text = "Full tool output\nsecond line".into();
        assert_eq!(
            message_shape(&translator(), &message),
            MessageShape::Disclosure {
                summary: "Bash".into(),
                first_line: "Full tool output".into()
            }
        );
        assert_eq!(message_text(&message), "Full tool output\nsecond line");
    }

    // parity: TS-renderer-AiChatTranscriptMessage-3b12f1
    #[test]
    fn copy_takes_the_full_original_message_and_time_is_shown() {
        let mut message = AggregatedAiMessage::plain(
            AiMessageRole::Assistant,
            Some("2026-11-12T03:00:00Z"),
            "preview",
        );
        message.full_text = "Full original answer".into();
        assert_eq!(message_text(&message), "Full original answer");
        // 표기는 시스템 locale·시간대를 따른다(`locale_format`이 en-US 모양을 따로 확인한다)
        assert!(message_time(message.timestamp.as_deref()).is_some());
        assert_eq!(message_time(None), None);
    }

    // parity: TS-renderer-TaskDetailRoute-b96b6e
    #[test]
    fn each_message_copies_its_original_text() {
        let mut truncated = AggregatedAiMessage::plain(AiMessageRole::Assistant, None, "미리보기…");
        truncated.full_text = "미리보기 전체 원문".into();
        truncated.is_truncated = true;
        assert_eq!(message_text(&truncated), "미리보기 전체 원문");
        let mut preview_only =
            AggregatedAiMessage::plain(AiMessageRole::Assistant, None, "짧은 답");
        preview_only.full_text.clear();
        assert_eq!(message_text(&preview_only), "짧은 답");
    }

    // parity: TS-renderer-AiChatTranscriptMessage-adda56
    #[test]
    fn user_requests_stay_as_typed() {
        let message = AggregatedAiMessage::plain(AiMessageRole::User, None, "# 제목 아님\n둘째 줄");
        assert_eq!(
            message_shape(&translator(), &message),
            MessageShape::UserBubble
        );
        assert_eq!(message_text(&message), "# 제목 아님\n둘째 줄");
        assert_eq!(
            meta_name(
                &translator(),
                AiMessageRole::User,
                AiSessionProvider::Claude
            ),
            None
        );
    }

    // parity: TS-renderer-AiChatTranscriptMessage-089073
    #[test]
    fn tool_rows_show_the_tool_name_or_the_role() {
        let named = message_shape(&translator(), &tool(Some("Bash"), "ls"));
        let unnamed = message_shape(&translator(), &tool(None, "ls"));
        assert!(matches!(named, MessageShape::Disclosure { summary, .. } if summary == "Bash"));
        assert!(matches!(unnamed, MessageShape::Disclosure { summary, .. } if summary == "도구"));
    }

    // parity: TS-renderer-AiChatTranscriptMessage-da1118
    #[test]
    fn changed_files_become_preview_cards() {
        let mut message = tool(Some("Write"), "done");
        message.files = Some(vec![
            AiMessageFile {
                path: "/repo/docs/plan.md".into(),
                change: AiFileChange::Created,
            },
            AiMessageFile {
                path: "/repo/src/app.ts".into(),
                change: AiFileChange::Modified,
            },
        ]);
        let cards = file_cards(&translator(), &message, true);
        let shown: Vec<(&str, &str)> = cards
            .iter()
            .map(|card| (card.name.as_str(), card.change_label.as_str()))
            .collect();
        assert_eq!(shown, [("plan.md", "새로 만듦"), ("app.ts", "수정함")]);
        assert_eq!(cards[1].path, "/repo/src/app.ts");
    }

    // parity: TS-renderer-AiChatTranscriptMessage-861e2a
    #[test]
    fn no_file_cards_where_previews_cannot_open() {
        let mut message = tool(Some("Write"), "done");
        message.files = Some(vec![AiMessageFile {
            path: "a.md".into(),
            change: AiFileChange::Created,
        }]);
        assert!(file_cards(&translator(), &message, false).is_empty());
    }

    // parity: TS-renderer-AiChatTranscriptMessage-981202
    #[test]
    fn system_and_developer_messages_are_shown_directly() {
        for role in [AiMessageRole::System, AiMessageRole::Developer] {
            let message = AggregatedAiMessage::plain(role, None, "System instructions");
            assert_eq!(message_shape(&translator(), &message), MessageShape::Body);
        }
        assert_eq!(
            meta_name(
                &translator(),
                AiMessageRole::System,
                AiSessionProvider::Claude
            )
            .as_deref(),
            Some("시스템 입력")
        );
        assert_eq!(
            meta_name(
                &translator(),
                AiMessageRole::Assistant,
                AiSessionProvider::Claude
            )
            .as_deref(),
            Some("Claude")
        );
    }
}
