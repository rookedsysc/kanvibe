//! 대화 한 줄(TS `AiChatTranscriptMessage`를 pane 채팅이 쓰는 모양). 도구·추론은 첫 줄만 보이게 접고,
//! 사용자 요청은 오른쪽 말풍선, 답변은 폭 전체 본문으로 그린다. 판정은 AI 대화 기록 화면과 같은 함수를 쓴다.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};
use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::types::AggregatedAiMessage;

use super::super::chat_message::{
    MessageShape, message_shape, message_text, message_time, meta_name,
};
use super::super::message_content::render_message;
use super::view::PaneChatView;
use crate::components::TailwindText;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

impl PaneChatView {
    pub(super) fn render_message_row(
        &self,
        theme: &ThemeTokens,
        message: &AggregatedAiMessage,
        key: &str,
        provider: AiSessionProvider,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let meta = self.render_meta(theme, message, provider);
        match message_shape(&self.translator, message) {
            MessageShape::Disclosure {
                summary,
                first_line,
            } => self.render_disclosure(theme, message, key, summary, first_line, cx),
            MessageShape::UserBubble => user_bubble(theme, message, meta),
            MessageShape::Body => div()
                .min_w_0()
                .w_full()
                .py(spacing(1.0))
                .text_scale(text_scale::SM)
                .line_height(px(28.0))
                .text_color(theme.text.primary)
                .child(render_message(
                    &format!("pane-chat-{key}"),
                    message_text(message),
                    message.role,
                    theme,
                ))
                .children(meta)
                .into_any_element(),
        }
    }

    fn render_meta(
        &self,
        theme: &ThemeTokens,
        message: &AggregatedAiMessage,
        provider: AiSessionProvider,
    ) -> Option<AnyElement> {
        let parts: Vec<String> = [
            meta_name(&self.translator, message.role, provider),
            message_time(message.timestamp.as_deref()),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| {
            div()
                .mt(spacing(1.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(parts.join(" · "))
                .into_any_element()
        })
    }

    fn render_disclosure(
        &self,
        theme: &ThemeTokens,
        message: &AggregatedAiMessage,
        key: &str,
        summary: String,
        first_line: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_open = self.open_disclosures.contains(key);
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
            .children(is_open.then(|| {
                div()
                    .mt(spacing(2.0))
                    .min_w_0()
                    .text_scale(text_scale::SM)
                    .line_height(px(24.0))
                    .text_color(theme.text.primary)
                    .child(render_message(
                        &format!("pane-chat-{key}"),
                        message_text(message),
                        message.role,
                        theme,
                    ))
            }))
            .into_any_element()
    }
}

/// 오른쪽 말풍선. 사용자 요청은 해석하지 않고 쓴 그대로 보인다
fn user_bubble(
    theme: &ThemeTokens,
    message: &AggregatedAiMessage,
    meta: Option<AnyElement>,
) -> AnyElement {
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
                .child(message_text(message).to_string())
                .children(meta),
        )
        .into_any_element()
}
