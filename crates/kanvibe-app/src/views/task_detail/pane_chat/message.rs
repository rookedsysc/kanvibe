//! 대화 한 줄(TS `AiChatTranscriptMessage`를 pane 채팅이 쓰는 모양). 도구·추론은 첫 줄만 보이게 접고,
//! 사용자 요청은 오른쪽 말풍선, 답변은 폭 전체 본문으로 그린다. 판정·첨부·복사 버튼은 AI 대화 기록 화면과 같은 함수를 쓴다.

use gpui_kit::{
    AnyElement, ClipboardItem, Context, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};
use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::types::AggregatedAiMessage;

use super::super::chat_message::attachment::{AttachmentContext, ChatAttachment};
use super::super::chat_message::{
    MessageShape, copy_message_button, has_copy_action, message_attachments, message_shape,
    message_text, message_time, meta_name,
};
use super::super::message_content::render_message;
use super::view::PaneChatView;
use crate::components::TailwindText;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

impl PaneChatView {
    /// 첨부 내용을 읽을 대화. TS처럼 대화 id가 있을 때만 만든다
    pub(super) fn attachment_context(&self) -> Option<AttachmentContext> {
        let snapshot = self.session.snapshot()?;
        Some(AttachmentContext {
            task_id: self.session.task_id.clone(),
            provider: snapshot.provider?,
            session_id: snapshot.transcript.session_id.clone()?,
            source_ref: None,
        })
    }

    pub(super) fn render_message_row(
        &self,
        theme: &ThemeTokens,
        message: &AggregatedAiMessage,
        key: &str,
        provider: AiSessionProvider,
        attachment_context: Option<AttachmentContext>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let meta = self.render_meta(theme, message, key, provider, cx);
        let attachments =
            message_attachments(message, key, attachment_context, &self.translator, theme);
        match message_shape(&self.translator, message) {
            MessageShape::Disclosure {
                summary,
                first_line,
            } => {
                let open_body = self.open_disclosures.contains(key).then(|| {
                    div()
                        .children(message_content(message, key, theme))
                        .children(attachments)
                        .into_any_element()
                });
                self.render_disclosure(theme, key, summary, first_line, open_body, cx)
            }
            MessageShape::UserBubble => user_bubble(theme, message, attachments, meta),
            MessageShape::Body => div()
                .min_w_0()
                .w_full()
                .py(spacing(1.0))
                .text_scale(text_scale::SM)
                .line_height(px(28.0))
                .text_color(theme.text.primary)
                .children(message_content(message, key, theme))
                .children(attachments)
                .children(meta)
                .into_any_element(),
        }
    }

    fn render_meta(
        &self,
        theme: &ThemeTokens,
        message: &AggregatedAiMessage,
        key: &str,
        provider: AiSessionProvider,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let parts: Vec<String> = [
            meta_name(&self.translator, message.role, provider),
            message_time(message.timestamp.as_deref()),
        ]
        .into_iter()
        .flatten()
        .collect();
        let copy =
            has_copy_action(message).then(|| self.render_copy_message(theme, message, key, cx));
        (!parts.is_empty() || copy.is_some()).then(|| {
            div()
                .mt(spacing(1.0))
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .children((!parts.is_empty()).then(|| div().child(parts.join(" · "))))
                .children(copy)
                .into_any_element()
        })
    }

    fn render_copy_message(
        &self,
        theme: &ThemeTokens,
        message: &AggregatedAiMessage,
        key: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_copied = self.copied_messages.contains(key);
        let text = message_text(message).to_string();
        let copied_key = key.to_string();
        copy_message_button(
            theme,
            &self.translator,
            &format!("pane-chat-{key}"),
            is_copied,
        )
        .on_click(cx.listener(move |view, _, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
            view.copied_messages.insert(copied_key.clone());
            cx.notify();
        }))
    }

    /// 접힌 도구·추론 줄. `open_body`는 펼쳤을 때만 받는다
    fn render_disclosure(
        &self,
        theme: &ThemeTokens,
        key: &str,
        summary: String,
        first_line: String,
        open_body: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let toggle_key = key.to_string();
        let header = div()
            .id(SharedString::from(format!("pane-chat-disclosure-{key}")))
            .flex()
            .min_w_0()
            .items_center()
            .gap(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .cursor_pointer()
            .child(div().flex_none().css_weight(500).child(summary))
            .child(div().min_w_0().truncate().child(first_line))
            .on_click(cx.listener(move |view, _, _, cx| view.toggle_disclosure(&toggle_key, cx)));
        div()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.surface)
            .px(spacing(4.0))
            .py(spacing(2.0))
            .child(header)
            .children(open_body.map(|body| {
                div()
                    .mt(spacing(2.0))
                    .min_w_0()
                    .text_scale(text_scale::SM)
                    .line_height(px(24.0))
                    .text_color(theme.text.primary)
                    .child(body)
            }))
            .into_any_element()
    }
}

/// 오른쪽 말풍선. 사용자 요청은 해석하지 않고 쓴 그대로 보인다
fn user_bubble(
    theme: &ThemeTokens,
    message: &AggregatedAiMessage,
    attachments: Vec<ChatAttachment>,
    meta: Option<AnyElement>,
) -> AnyElement {
    let text = message_text(message);
    div()
        .flex()
        .min_w_0()
        .justify_end()
        .child(
            div()
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
                .children((!text.is_empty()).then(|| text.to_string()))
                .children(attachments)
                .children(meta),
        )
        .into_any_element()
}

/// 원문이 있을 때만 그리는 본문. 그림만 보낸 메시지에 빈 본문을 두지 않는다
fn message_content(
    message: &AggregatedAiMessage,
    key: &str,
    theme: &ThemeTokens,
) -> Option<AnyElement> {
    let text = message_text(message);
    (!text.is_empty())
        .then(|| render_message(&format!("pane-chat-{key}"), text, message.role, theme))
}
