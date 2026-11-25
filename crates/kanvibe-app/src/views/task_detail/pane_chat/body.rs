//! 대화 영역의 내용(TS `PaneChatView` 대화 영역 안 조건부 블록들). 무엇을 어떤 글로 보일지 먼저 [`BodyBlock`] 목록으로 정하고,
//! 그 목록을 그린다. 화면 검사도 같은 목록을 읽는다.

use gpui_kit::{
    AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};
use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::pane_chat::snapshot::{PaneChatSnapshot, PaneChatTranscriptStatus};
use kanvibe_ai::types::AggregatedAiMessage;

use super::super::ai_provider_icon::ai_provider_icon;
use super::client::RevealOutcome;
use super::session::{CODEX_INPUT_BUSY_REASON, PendingState};
use super::view::PaneChatView;
use super::widgets::view_button;
use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// footer에서 대화 ID를 찾지 못한 Codex. "대화 ID 표시"로 연결할 수 있다
fn is_codex_footer_reason(reason: Option<&str>) -> bool {
    matches!(
        reason,
        Some("codex-thread-unconfirmed" | "codex-thread-input-busy")
    )
}

/// 대화를 연결하지 못한 원인에 맞는 안내 문구 키
pub fn unavailable_message_key(provider: AiSessionProvider, reason: Option<&str>) -> &'static str {
    match (reason, provider) {
        (Some("transcript-read-failed"), _) => "transcriptReadFailed",
        (Some("codex-thread-record-missing"), _) => "codexRecordMissing",
        (_, AiSessionProvider::Claude) => "claudeIdentityMissing",
        (_, AiSessionProvider::Codex) => "codexIdentityMissing",
        _ => "transcriptIdentityMissing",
    }
}

pub fn reveal_message_key(outcome: RevealOutcome) -> &'static str {
    match outcome {
        RevealOutcome::Revealed => "revealRevealed",
        RevealOutcome::InputNotEmpty => "revealInputNotEmpty",
        RevealOutcome::UnsupportedVersion => "revealUnsupportedVersion",
        RevealOutcome::ThreadNotFound => "revealThreadNotFound",
    }
}

/// "대화 ID 표시" 버튼과 그 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevealControl {
    pub label: String,
    pub is_disabled: bool,
    /// 누를 수 없는 이유(Codex가 작업 중이거나 입력란이 차 있음)
    pub busy_note: Option<String>,
    /// 결과 안내와 그것이 성공(`status`)인지 경고(`alert`)인지
    pub outcome: Option<(String, bool)>,
}

/// 대화를 연결하지 못한 동안의 안내 카드
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableCard {
    pub heading: String,
    /// 버튼 앞 안내 줄
    pub lines: Vec<String>,
    pub reveal: Option<RevealControl>,
    /// 버튼 뒤 안내 줄(데몬 없이 다시 실행하는 방법)
    pub trailing_line: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BodyBlock {
    Loading(String),
    NoAgent(String),
    LoadOlder {
        label: String,
        is_loading: bool,
    },
    OlderError(String),
    Welcome {
        title: String,
        hint: String,
        provider: AiSessionProvider,
    },
    Message {
        key: String,
        message: Box<AggregatedAiMessage>,
    },
    Unavailable(UnavailableCard),
    Screen {
        label: String,
        content: Option<String>,
    },
    Pending {
        id: String,
        message: Box<AggregatedAiMessage>,
        status: String,
    },
    Working {
        label: String,
        elapsed: Option<String>,
    },
}

impl BodyBlock {
    /// 이 블록이 보이는 글(버튼 이름 빼고)
    pub fn texts(&self) -> Vec<String> {
        match self {
            Self::Loading(text) | Self::NoAgent(text) | Self::OlderError(text) => {
                vec![text.clone()]
            }
            Self::LoadOlder { label, .. } => vec![label.clone()],
            Self::Welcome { title, hint, .. } => vec![title.clone(), hint.clone()],
            Self::Message { message, .. } => vec![message_text(message)],
            Self::Unavailable(card) => {
                let mut texts = vec![card.heading.clone()];
                texts.extend(card.lines.iter().cloned());
                if let Some(reveal) = &card.reveal {
                    texts.extend(reveal.busy_note.iter().cloned());
                    texts.extend(reveal.outcome.iter().map(|(text, _)| text.clone()));
                }
                texts.extend(card.trailing_line.iter().cloned());
                texts
            }
            Self::Screen { label, content } => std::iter::once(label.clone())
                .chain(content.iter().cloned())
                .collect(),
            Self::Pending {
                message, status, ..
            } => vec![message_text(message), status.clone()],
            Self::Working { label, elapsed } => std::iter::once(label.clone())
                .chain(elapsed.iter().cloned())
                .collect(),
        }
    }
}

fn message_text(message: &AggregatedAiMessage) -> String {
    super::super::chat_message::message_text(message).to_string()
}

impl PaneChatView {
    /// 대화 영역에 보일 블록들(TS 대화 영역의 조건부 블록 순서)
    pub fn body_blocks(&self, cx: &App) -> Vec<BodyBlock> {
        let Some(snapshot) = self.session.snapshot().cloned() else {
            return match self.session.has_read_error {
                true => Vec::new(),
                false => vec![BodyBlock::Loading(self.t("loading"))],
            };
        };
        let mut blocks = Vec::new();
        if !snapshot.can_send && snapshot.provider.is_none() {
            blocks.push(BodyBlock::NoAgent(self.t("noAgent")));
        }
        if snapshot.can_send && snapshot.transcript.status == PaneChatTranscriptStatus::Available {
            blocks.extend(self.transcript_blocks(&snapshot, cx));
        }
        if let (Some(provider), PaneChatTranscriptStatus::Unavailable) =
            (snapshot.provider, snapshot.transcript.status)
        {
            blocks.push(BodyBlock::Unavailable(
                self.unavailable_card(provider, &snapshot),
            ));
        }
        if !snapshot.screen.is_empty()
            && snapshot.transcript.status != PaneChatTranscriptStatus::Available
        {
            let content = self.is_screen_open.then(|| snapshot.screen.clone());
            blocks.push(BodyBlock::Screen {
                label: self.t("screen"),
                content,
            });
        }
        blocks.extend(self.pending_blocks());
        if self.session.is_agent_working() && snapshot.can_send {
            let elapsed = self
                .working_seconds
                .filter(|seconds| *seconds > 0)
                .map(|seconds| {
                    self.translator.translate_with(
                        "paneChat.agentWorkingElapsed",
                        &[("seconds", &seconds.to_string())],
                    )
                });
            blocks.push(BodyBlock::Working {
                label: self.t("agentWorking"),
                elapsed,
            });
        }
        blocks
    }

    /// 대화 영역에 보이는 글 전부(버튼 이름 빼고)
    pub fn body_texts(&self, cx: &App) -> Vec<String> {
        self.body_blocks(cx)
            .iter()
            .flat_map(BodyBlock::texts)
            .collect()
    }

    fn transcript_blocks(&self, snapshot: &PaneChatSnapshot, cx: &App) -> Vec<BodyBlock> {
        let mut blocks = Vec::new();
        if self.transcript.next_cursor(Some(snapshot)).is_some() {
            let is_loading = self.transcript.is_loading(Some(snapshot));
            let label = self.t(if is_loading {
                "loadingOlder"
            } else {
                "loadOlder"
            });
            blocks.push(BodyBlock::LoadOlder { label, is_loading });
        }
        if self.transcript.has_error(Some(snapshot)) {
            blocks.push(BodyBlock::OlderError(self.t("olderError")));
        }
        let provider = snapshot.provider.unwrap_or(AiSessionProvider::Codex);
        if self.is_new_conversation(cx) {
            let title = self
                .translator
                .translate_with("paneChat.welcomeTitle", &[("provider", provider.label())]);
            blocks.push(BodyBlock::Welcome {
                title,
                hint: self.t("welcomeHint"),
                provider,
            });
        }
        let messages = self.visible_messages(cx);
        let session_key = snapshot.transcript.session_id.clone();
        for row in super::super::chat_history::transcript_rows(&messages, session_key.as_deref()) {
            blocks.push(BodyBlock::Message {
                key: row.key,
                message: Box::new(row.message.clone()),
            });
        }
        blocks
    }

    fn unavailable_card(
        &self,
        provider: AiSessionProvider,
        snapshot: &PaneChatSnapshot,
    ) -> UnavailableCard {
        let reason = snapshot.transcript.reason.as_deref();
        let is_waiting_for_input = reason == Some(CODEX_INPUT_BUSY_REASON);
        let heading = self.t(if is_waiting_for_input {
            "transcriptWaiting"
        } else {
            "transcriptUnavailable"
        });
        if !is_codex_footer_reason(reason) {
            return UnavailableCard {
                heading,
                lines: vec![self.t(unavailable_message_key(provider, reason))],
                reveal: None,
                trailing_line: None,
            };
        }
        let mut lines = Vec::new();
        if is_waiting_for_input {
            lines.push(self.t("transcriptInputBusy"));
        }
        lines.push(self.t("codexDaemonLimit"));
        lines.push(self.t("codexRevealHint"));
        let is_busy = self.session.is_busy;
        let reveal = RevealControl {
            label: self.t(if is_busy {
                "revealingThread"
            } else {
                "revealThread"
            }),
            is_disabled: is_waiting_for_input || is_busy,
            busy_note: is_waiting_for_input.then(|| self.t("codexRevealBusy")),
            outcome: self.reveal_outcome.map(|outcome| {
                (
                    self.t(reveal_message_key(outcome)),
                    outcome == RevealOutcome::Revealed,
                )
            }),
        };
        UnavailableCard {
            heading,
            lines,
            reveal: Some(reveal),
            trailing_line: Some(self.t("codexNoDaemonHint")),
        }
    }

    fn pending_blocks(&self) -> Vec<BodyBlock> {
        self.session
            .pending_messages()
            .into_iter()
            .map(|pending| BodyBlock::Pending {
                id: pending.id.clone(),
                message: Box::new(pending.message.clone()),
                status: self.t(match pending.state {
                    PendingState::Sending => "sending",
                    PendingState::Sent => "messageSent",
                    PendingState::Failed => "sendError",
                }),
            })
            .collect()
    }
}

/// 블록 그리기
impl PaneChatView {
    pub(super) fn render_body_blocks(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.body_blocks(cx)
            .into_iter()
            .map(|block| self.render_body_block(theme, block, cx))
            .collect()
    }

    fn render_body_block(
        &self,
        theme: &ThemeTokens,
        block: BodyBlock,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let provider = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.provider);
        let message_provider = provider.unwrap_or(AiSessionProvider::Codex);
        match block {
            BodyBlock::Loading(text) => secondary_text(theme, text),
            BodyBlock::NoAgent(text) => self.render_no_agent(theme, text, cx),
            BodyBlock::LoadOlder { label, is_loading } => div()
                .flex()
                .child(
                    view_button(theme, "pane-chat-load-older", label, is_loading).on_click(
                        cx.listener(|view, _, window, cx| view.load_older_messages(window, cx)),
                    ),
                )
                .into_any_element(),
            BodyBlock::OlderError(text) => div()
                .text_scale(text_scale::XS)
                .text_color(theme.status.error)
                .child(text)
                .into_any_element(),
            BodyBlock::Welcome {
                title,
                hint,
                provider,
            } => render_welcome(theme, title, hint, provider),
            BodyBlock::Message { key, message } => {
                self.render_message_row(theme, &message, &key, message_provider, cx)
            }
            BodyBlock::Unavailable(card) => self.render_unavailable(theme, card, cx),
            BodyBlock::Screen { label, content } => self.render_screen(theme, label, content, cx),
            BodyBlock::Pending {
                id,
                message,
                status,
            } => {
                let row = self.render_message_row(theme, &message, &id, message_provider, cx);
                pending_bubble(theme, row, status)
            }
            BodyBlock::Working { label, elapsed } => {
                self.render_working(theme, provider, label, elapsed)
            }
        }
    }

    fn render_no_agent(
        &self,
        theme: &ThemeTokens,
        text: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let terminal = view_button(
            theme,
            "pane-chat-no-agent-terminal",
            self.t("terminal"),
            false,
        )
        .on_click(cx.listener(|view, _, _, cx| view.open_terminal(cx)));
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(spacing(4.0))
            .child(secondary_text(theme, text))
            .child(terminal)
            .into_any_element()
    }

    fn render_unavailable(
        &self,
        theme: &ThemeTokens,
        card: UnavailableCard,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let reveal = card
            .reveal
            .map(|reveal| self.render_reveal_controls(theme, reveal, cx));
        let retry = view_button(theme, "pane-chat-unavailable-retry", self.t("retry"), false)
            .on_click(cx.listener(|view, _, window, cx| view.refresh(window, cx)));
        let terminal = view_button(
            theme,
            "pane-chat-unavailable-terminal",
            self.t("terminal"),
            false,
        )
        .on_click(cx.listener(|view, _, _, cx| view.open_terminal(cx)));
        div()
            .flex()
            .flex_col()
            .gap(spacing(3.0))
            .rounded(radius::LG)
            .bg(theme.background.surface)
            .p(spacing(4.0))
            .text_scale(text_scale::SM)
            .text_color(theme.text.secondary)
            .child(card.heading)
            .children(card.lines.into_iter().map(|line| div().child(line)))
            .children(reveal.into_iter().flatten())
            .children(card.trailing_line.map(|line| div().child(line)))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(spacing(2.0))
                    .child(retry)
                    .child(terminal),
            )
            .into_any_element()
    }

    /// "대화 ID 표시" 버튼과 누를 수 없는 이유, 그리고 결과 안내(성공은 기본 글자색, 아니면 경고색)
    fn render_reveal_controls(
        &self,
        theme: &ThemeTokens,
        reveal: RevealControl,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let button_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(spacing(2.0))
            .child(self.render_reveal_button(theme, reveal.label, reveal.is_disabled, cx))
            .children(
                reveal
                    .busy_note
                    .map(|note| div().text_scale(text_scale::XS).child(note)),
            )
            .into_any_element();
        let outcome = reveal.outcome.map(|(text, is_success)| {
            let color = if is_success {
                theme.text.primary
            } else {
                theme.status.warning
            };
            div().text_color(color).child(text).into_any_element()
        });
        std::iter::once(button_row).chain(outcome).collect()
    }

    fn render_reveal_button(
        &self,
        theme: &ThemeTokens,
        label: String,
        is_disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let button = div()
            .id("pane-chat-reveal-thread")
            .relative()
            .flex()
            .min_h(px(40.0))
            .items_center()
            .rounded(radius::MD)
            .bg(theme.brand.primary)
            .px(spacing(3.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.inverse)
            .child(label.clone())
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.clone().into(),
                text: label.into(),
            }));
        match is_disabled {
            true => button.opacity(0.4).into_any_element(),
            false => button
                .cursor_pointer()
                .hover(|style| style.bg(theme.brand.hover))
                .on_click(cx.listener(|view, _, window, cx| view.reveal_codex_thread(window, cx)))
                .into_any_element(),
        }
    }

    /// 대화를 연결하지 못한 동안의 "현재 CLI 화면" 펼침
    fn render_screen(
        &self,
        theme: &ThemeTokens,
        label: String,
        content: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let summary = screen_summary(theme, label, content.is_some())
            .on_click(cx.listener(|view, _, _, cx| view.toggle_screen(cx)));
        div()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.surface)
            .child(summary)
            .children(content.map(|screen| screen_lines(theme, &screen)))
            .into_any_element()
    }

    /// 보낸 메시지를 처리하는 동안 대화 끝에 붙는 표시. 제공자 아이콘이 깜빡이고 경과 초가 올라간다
    fn render_working(
        &self,
        theme: &ThemeTokens,
        provider: Option<AiSessionProvider>,
        label: String,
        elapsed: Option<String>,
    ) -> AnyElement {
        let seconds = self.working_seconds.unwrap_or(0);
        let opacity = if seconds.is_multiple_of(2) { 1.0 } else { 0.5 };
        div()
            .id("pane-chat-working")
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .py(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .children(provider.map(|provider| {
                div()
                    .opacity(opacity)
                    .child(ai_provider_icon(provider, 16.0))
            }))
            .child(label)
            .children(elapsed)
            .into_any_element()
    }
}

/// 빈 대화의 첫 화면. 바로 아래 입력창과 한 묶음으로 화면 가운데에 놓인다
fn render_welcome(
    theme: &ThemeTokens,
    title: String,
    hint: String,
    provider: AiSessionProvider,
) -> AnyElement {
    div()
        .id("pane-chat-welcome")
        .flex()
        .flex_col()
        .items_center()
        .gap(spacing(2.0))
        .pb(spacing(2.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(spacing(3.0))
                .child(ai_provider_icon(provider, 28.0))
                .child(
                    div()
                        .text_scale(text_scale::XL)
                        .css_weight(500)
                        .text_color(theme.text.primary)
                        .child(aligned_text(title, text_scale::XL, TextFamily::Sans)),
                ),
        )
        .child(secondary_text(theme, hint))
        .into_any_element()
}

/// 보낸 말풍선과 그 아래 오른쪽 상태 글(전송 중·전송됨·보내지 못함)
fn pending_bubble(theme: &ThemeTokens, row: AnyElement, status: String) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .child(row)
        .child(
            div()
                .flex()
                .justify_end()
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(status),
        )
        .into_any_element()
}

/// "현재 CLI 화면" 펼침 머리줄(HTML `<details>`의 삼각형 표시)
fn screen_summary(
    theme: &ThemeTokens,
    label: String,
    is_open: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let marker = if is_open { "▼" } else { "▶" };
    div()
        .id("pane-chat-screen-toggle")
        .relative()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(spacing(4.0))
        .py(spacing(3.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.secondary)
        .cursor_pointer()
        .child(div().text_size(px(8.0)).child(marker))
        .child(aligned_text(
            label.clone(),
            text_scale::XS,
            TextFamily::Sans,
        ))
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }))
}

/// 펼친 CLI 화면. 줄을 접지 않고 고정폭 글꼴로 보인다
fn screen_lines(theme: &ThemeTokens, screen: &str) -> AnyElement {
    div()
        .id("pane-chat-screen")
        .max_h(px(320.0))
        .overflow_y_scroll()
        .px(spacing(4.0))
        .pb(spacing(4.0))
        .mono(400)
        .text_scale(text_scale::XS)
        .line_height(px(20.0))
        .text_color(theme.text.primary)
        .children(
            screen
                .split('\n')
                .map(|line| div().whitespace_nowrap().child(line.to_string()))
                .collect::<Vec<_>>(),
        )
        .into_any_element()
}

fn secondary_text(theme: &ThemeTokens, text: String) -> AnyElement {
    div()
        .text_scale(text_scale::SM)
        .text_color(theme.text.secondary)
        .child(text)
        .into_any_element()
}
