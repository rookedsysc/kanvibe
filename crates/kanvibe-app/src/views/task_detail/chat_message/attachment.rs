//! 대화 기록의 첨부(TS `AiChatAttachment`).
//!
//! 그림 첨부는 화면에 보일 때 한 번 읽어 작게 보이고, 누르면 크게 펼친다. 파일 첨부는 내려받기를 누를 때만 읽는다.
//! 경계(`getTaskAiSessionAttachment`)가 돌려준 값은 base64 data URL만 받고, 그림은 png·jpeg·gif·webp만 그린다
//! (`file://` 주소와 스크립트를 담을 수 있는 SVG는 거절한다). 읽기를 다시 시작하면 그 전 응답은 버린다.

use std::sync::Arc;

use gpui_kit::{
    AnyElement, App, AppContext, ElementId, Entity, Image, ImageFormat, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement,
    Styled, StyledImage, Task, Window, canvas, div, img, px,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{AiAttachmentKind, AiMessageAttachment, AiSessionAttachmentContent};
use kanvibe_domain::i18n::Translator;
use kanvibe_services::project::ports::AiSessionAttachmentRequest;

use super::super::session_queries::load_session_attachment;
use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 그림 첨부의 접힌 최대 높이(`max-h-56`)
const COLLAPSED_IMAGE_MAX_HEIGHT: f32 = 224.0;
/// 펼친 그림의 최대 높이 비율(`max-h-[70vh]`)
const EXPANDED_IMAGE_VIEWPORT_RATIO: f32 = 0.7;

/// 첨부를 읽을 세션(TS `AiChatAttachmentContext`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentContext {
    pub task_id: String,
    pub provider: AiSessionProvider,
    pub session_id: String,
    pub source_ref: Option<String>,
}

/// `^data:[\w.+-]+/[\w.+-]+;base64,[A-Za-z0-9+/]+=*$`이고, 그림이면 png·jpeg·gif·webp일 때만 받는다
pub fn is_acceptable_media(kind: AiAttachmentKind, data_url: &str) -> bool {
    let Some((header, payload)) = data_url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(";base64,"))
    else {
        return false;
    };
    let is_token = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "_.+-".contains(character))
    };
    let Some((media_type, subtype)) = header.split_once('/') else {
        return false;
    };
    let body = payload.trim_end_matches('=');
    let is_base64 = !body.is_empty()
        && body.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '+' || character == '/'
        });
    let is_valid = is_token(media_type) && is_token(subtype) && is_base64;
    match kind {
        AiAttachmentKind::File => is_valid,
        AiAttachmentKind::Image => {
            is_valid && media_type == "image" && image_format(subtype).is_some()
        }
    }
}

/// 그림만 보일 때 읽고, 파일은 내려받기를 누를 때까지 읽지 않는다
pub fn should_load_when_visible(kind: AiAttachmentKind) -> bool {
    kind == AiAttachmentKind::Image
}

fn image_format(subtype: &str) -> Option<ImageFormat> {
    match subtype {
        "png" => Some(ImageFormat::Png),
        "jpeg" => Some(ImageFormat::Jpeg),
        "gif" => Some(ImageFormat::Gif),
        "webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

/// 표준 base64(패딩 허용)를 바이트로 바꾼다
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let value = |byte: u8| -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let body = text.trim_end_matches('=').as_bytes();
    let mut bytes = Vec::with_capacity(body.len() * 3 / 4);
    for chunk in body.chunks(4) {
        let mut buffer = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            buffer |= value(*byte)? << (18 - 6 * index);
        }
        let produced = (chunk.len() * 6) / 8;
        bytes.extend_from_slice(&buffer.to_be_bytes()[1..1 + produced]);
    }
    Some(bytes)
}

/// data URL의 바이트
fn data_url_bytes(data_url: &str) -> Option<Vec<u8>> {
    decode_base64(data_url.split_once(";base64,")?.1)
}

/// 첨부 하나의 상태
#[derive(Default)]
pub struct ChatAttachmentState {
    pub media: Option<AiSessionAttachmentContent>,
    pub image: Option<Arc<Image>>,
    pub is_loading: bool,
    pub is_failed: bool,
    pub is_expanded: bool,
    /// 화면에 한 번 보여 그림 읽기를 시작했는지
    pub has_started: bool,
    generation: u64,
    _load: Option<Task<()>>,
}

impl ChatAttachmentState {
    /// 새 읽기를 시작한다. 그 전 읽기의 응답은 이후 버린다
    pub fn begin_load(&mut self) -> u64 {
        self.generation += 1;
        self.is_loading = true;
        self.is_failed = false;
        self.generation
    }

    /// 읽기 결과를 받는다. 다시 시작한 뒤에 도착한 지난 응답이면 무시한다
    pub fn finish_load(
        &mut self,
        generation: u64,
        kind: AiAttachmentKind,
        result: Option<AiSessionAttachmentContent>,
    ) {
        if generation != self.generation {
            return;
        }
        self.is_loading = false;
        let accepted = result.filter(|content| is_acceptable_media(kind, &content.data_url));
        let image = accepted.as_ref().and_then(|content| match kind {
            AiAttachmentKind::Image => decoded_image(&content.data_url),
            AiAttachmentKind::File => None,
        });
        self.is_failed = accepted.is_none() || (kind == AiAttachmentKind::Image && image.is_none());
        if !self.is_failed {
            self.image = image;
            self.media = accepted;
        }
    }

    /// 지금 그림을 보이는지(TS `attachment.kind === "image" && media && !failed`)
    pub fn shows_image(&self) -> bool {
        self.image.is_some() && !self.is_failed
    }
}

fn decoded_image(data_url: &str) -> Option<Arc<Image>> {
    let subtype = data_url.strip_prefix("data:image/")?.split(';').next()?;
    let format = image_format(subtype)?;
    Some(Arc::new(Image::from_bytes(
        format,
        data_url_bytes(data_url)?,
    )))
}

/// 읽기를 시작해 결과를 상태에 넣는다. `then`은 성공한 내용으로 할 일(내려받기)이다
fn start_load(
    state: &Entity<ChatAttachmentState>,
    context: &AttachmentContext,
    attachment: &AiMessageAttachment,
    cx: &mut App,
    then: impl FnOnce(AiSessionAttachmentContent, &mut App) + 'static,
) {
    let request = AiSessionAttachmentRequest {
        provider: context.provider,
        session_id: context.session_id.clone(),
        attachment_id: attachment.id.clone(),
        source_ref: context.source_ref.clone(),
    };
    let read = load_session_attachment(&context.task_id, request, cx);
    let kind = attachment.kind;
    state.update(cx, |state, cx| {
        let generation = state.begin_load();
        state._load = Some(cx.spawn(async move |state, cx| {
            let result = read.await.ok().flatten();
            let _ = state.update(cx, |state, cx| {
                state.finish_load(generation, kind, result);
                let media = state.media.clone().filter(|_| !state.is_failed);
                cx.notify();
                if let Some(media) = media {
                    cx.defer(move |cx| then(media, cx));
                }
            });
        }));
        cx.notify();
    });
}

/// 받은 바이트를 사용자가 고른 경로에 쓴다(브라우저 내려받기)
fn save_attachment(media: AiSessionAttachmentContent, cx: &mut App) {
    let Some(bytes) = data_url_bytes(&media.data_url) else {
        return;
    };
    let directory = std::env::home_dir().unwrap_or_default().join("Downloads");
    let chosen = cx.prompt_for_new_path(&directory, Some(&media.name));
    cx.background_spawn(async move {
        if let Ok(Ok(Some(path))) = chosen.await {
            let _ = std::fs::write(path, bytes);
        }
    })
    .detach();
}

/// 첨부 문구(화면 언어)
#[derive(Clone)]
pub struct AttachmentLabels {
    pub open_image: SharedString,
    pub close_image: SharedString,
    pub loading: SharedString,
    pub unavailable: SharedString,
    pub retry: SharedString,
    pub download: SharedString,
}

impl AttachmentLabels {
    pub fn from_translator(translator: &Translator) -> Self {
        let text = |key: &str| SharedString::from(translator.translate(key));
        Self {
            open_image: text("aiSessions.openImage"),
            close_image: text("aiSessions.closeImage"),
            loading: text("aiSessions.attachmentLoading"),
            unavailable: text("aiSessions.attachmentUnavailable"),
            retry: text("aiSessions.retry"),
            download: text("aiSessions.downloadAttachment"),
        }
    }
}

/// 첨부 하나(`figure my-2 min-w-0 max-w-full rounded-lg border border-border-subtle bg-bg-page p-2`)
#[derive(IntoElement)]
pub struct ChatAttachment {
    pub key: String,
    pub attachment: AiMessageAttachment,
    pub context: Option<AttachmentContext>,
    pub labels: AttachmentLabels,
    pub theme: ThemeTokens,
}

impl ChatAttachment {
    /// 세션·첨부가 바뀌면 상태를 새로 만든다(TS 효과의 의존 값)
    fn state_id(&self) -> ElementId {
        let context = self
            .context
            .as_ref()
            .map(|context| {
                format!(
                    "{}/{}/{}",
                    context.task_id,
                    context.provider.id(),
                    context.session_id
                )
            })
            .unwrap_or_default();
        ElementId::Name(format!("attachment/{}/{context}/{}", self.key, self.attachment.id).into())
    }
}

impl RenderOnce for ChatAttachment {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state =
            window.use_keyed_state(self.state_id(), cx, |_, _| ChatAttachmentState::default());
        let viewport_height = f32::from(window.viewport_size().height);
        let theme = self.theme;
        let mut figure = div()
            .relative()
            .my(spacing(2.0))
            .min_w_0()
            .max_w_full()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.page)
            .p(spacing(2.0))
            .child(self.visibility_probe(&state));
        let snapshot = state.read(cx);
        if snapshot.shows_image() && self.attachment.kind == AiAttachmentKind::Image {
            figure = figure.child(self.render_image(&state, snapshot, viewport_height));
        }
        if snapshot.is_loading {
            figure = figure.child(
                div()
                    .px(spacing(2.0))
                    .py(spacing(4.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.secondary)
                    .child(self.labels.loading.clone()),
            );
        }
        if snapshot.is_failed {
            figure = figure.child(self.render_failure(&state));
        }
        figure.child(self.render_caption(&state, snapshot.is_loading))
    }
}

impl ChatAttachment {
    /// 그림은 화면에 처음 보일 때 읽는다(TS `IntersectionObserver`)
    fn visibility_probe(&self, state: &Entity<ChatAttachmentState>) -> impl IntoElement {
        let state = state.clone();
        let context = self.context.clone();
        let attachment = self.attachment.clone();
        canvas(
            move |bounds, window, cx| {
                let is_visible = window.content_mask().bounds.intersects(&bounds);
                if !is_visible
                    || state.read(cx).has_started
                    || !should_load_when_visible(attachment.kind)
                {
                    return;
                }
                state.update(cx, |state, _| state.has_started = true);
                match context.as_ref() {
                    Some(context) => start_load(&state, context, &attachment, cx, |_, _| {}),
                    None => state.update(cx, |state, cx| {
                        state.is_failed = true;
                        cx.notify();
                    }),
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    fn render_image(
        &self,
        state: &Entity<ChatAttachmentState>,
        snapshot: &ChatAttachmentState,
        viewport_height: f32,
    ) -> AnyElement {
        let Some(image) = snapshot.image.clone() else {
            return div().into_any_element();
        };
        let is_expanded = snapshot.is_expanded;
        let max_height = match is_expanded {
            true => viewport_height * EXPANDED_IMAGE_VIEWPORT_RATIO,
            false => COLLAPSED_IMAGE_MAX_HEIGHT,
        };
        let label = match is_expanded {
            true => self.labels.close_image.clone(),
            false => self.labels.open_image.clone(),
        };
        let unavailable = self.labels.unavailable.clone();
        let state = state.clone();
        div()
            .id(ElementId::Name(format!("{}-image", self.state_id()).into()))
            .relative()
            .rounded(radius::MD)
            .cursor_pointer()
            .child(
                img(image)
                    .max_w_full()
                    .max_h(px(max_height))
                    .rounded(radius::BASE)
                    .object_fit(ObjectFit::Contain)
                    .with_fallback(move || div().child(unavailable.clone()).into_any_element()),
            )
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.clone(),
                text: label,
            }))
            .on_click(move |_, _, cx| {
                state.update(cx, |state, cx| {
                    state.is_expanded = !state.is_expanded;
                    cx.notify();
                })
            })
            .into_any_element()
    }

    /// 읽지 못했다는 안내와 다시 시도
    fn render_failure(&self, state: &Entity<ChatAttachmentState>) -> impl IntoElement {
        let theme = &self.theme;
        let state = state.clone();
        let context = self.context.clone();
        let attachment = self.attachment.clone();
        let retry = link_button(
            ElementId::Name(format!("{}-retry", self.state_id()).into()),
            self.labels.retry.clone(),
            self.labels.retry.clone(),
            theme,
        )
        .on_click(move |_, _, cx| {
            if let Some(context) = context.as_ref() {
                start_load(&state, context, &attachment, cx, |_, _| {});
            }
        });
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(spacing(2.0))
            .px(spacing(2.0))
            .py(spacing(2.0))
            .text_scale(text_scale::XS)
            .child(
                div()
                    .text_color(theme.text.primary)
                    .child(self.labels.unavailable.clone()),
            )
            .child(retry)
    }

    /// 이름과 내려받기. 읽는 중이거나 세션을 모르면 누를 수 없다
    fn render_caption(
        &self,
        state: &Entity<ChatAttachmentState>,
        is_loading: bool,
    ) -> impl IntoElement {
        let theme = &self.theme;
        let name = self.attachment.name.clone();
        let accessible = format!("{}: {name}", self.labels.download);
        let download = link_button(
            ElementId::Name(format!("{}-download", self.state_id()).into()),
            self.labels.download.clone(),
            accessible.into(),
            theme,
        );
        let download = match (is_loading, self.context.clone()) {
            (false, Some(context)) => {
                let state = state.clone();
                let attachment = self.attachment.clone();
                download.on_click(move |_, _, cx| {
                    let cached = state.read(cx).media.clone();
                    match cached {
                        Some(media) => save_attachment(media, cx),
                        None => start_load(&state, &context, &attachment, cx, save_attachment),
                    }
                })
            }
            _ => download.opacity(0.4),
        };
        div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(spacing(2.0))
            .px(spacing(1.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .child(div().min_w_0().flex_1().truncate().child(name))
            .child(download)
    }
}

/// `min-h-9 rounded px-2 text-brand-primary`
fn link_button(
    id: ElementId,
    label: SharedString,
    accessible_name: SharedString,
    theme: &ThemeTokens,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .min_h(px(36.0))
        .items_center()
        .rounded(radius::BASE)
        .px(spacing(2.0))
        .text_color(theme.brand.primary)
        .cursor_pointer()
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: accessible_name,
            text: label,
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aRZkAAAAASUVORK5CYII=";

    fn content(data_url: &str) -> AiSessionAttachmentContent {
        AiSessionAttachmentContent {
            id: "image-a".into(),
            name: "photo.png".into(),
            mime_type: "image/png".into(),
            data_url: data_url.into(),
        }
    }

    #[test]
    fn base64_decodes_like_atob() {
        assert_eq!(decode_base64("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(decode_base64("aGk"), Some(b"hi".to_vec()));
        assert_eq!(decode_base64("a$=="), None);
    }

    // parity: TS-renderer-AiChatAttachment-33cf88
    #[test]
    fn a_late_response_from_an_earlier_request_does_not_replace_the_current_image() {
        let mut state = ChatAttachmentState::default();
        let earlier = state.begin_load();
        let current = state.begin_load();
        state.finish_load(current, AiAttachmentKind::Image, Some(content(PNG)));
        state.finish_load(
            earlier,
            AiAttachmentKind::Image,
            Some(content("data:image/png;base64,T0xEX1JFU1BPTlNF")),
        );
        assert!(state.shows_image());
        assert_eq!(
            state.media.as_ref().map(|media| media.data_url.as_str()),
            Some(PNG)
        );
        // 다른 작업의 같은 첨부는 다른 상태를 쓴다
        let attachment = AiMessageAttachment {
            id: "image-a".into(),
            kind: AiAttachmentKind::Image,
            name: "photo.png".into(),
            mime_type: Some("image/png".into()),
            size_bytes: Some(68),
        };
        let element = |task: &str| ChatAttachment {
            key: "row".into(),
            attachment: attachment.clone(),
            context: Some(AttachmentContext {
                task_id: task.into(),
                provider: AiSessionProvider::Codex,
                session_id: "session-a".into(),
                source_ref: None,
            }),
            labels: AttachmentLabels::from_translator(&Translator::new(
                kanvibe_domain::i18n::Locale::En,
                "taskDetail",
            )),
            theme: ThemeTokens::for_mode(crate::theme::ThemeMode::Dark),
        };
        assert_ne!(element("task-a").state_id(), element("task-b").state_id());
    }

    // parity: TS-renderer-AiChatAttachment-e644e9
    #[test]
    fn an_unavailable_attachment_recovers_when_retried() {
        let mut state = ChatAttachmentState::default();
        let first = state.begin_load();
        state.finish_load(first, AiAttachmentKind::Image, None);
        assert!(state.is_failed);
        assert!(!state.shows_image());
        let retry = state.begin_load();
        assert!(!state.is_failed);
        state.finish_load(retry, AiAttachmentKind::Image, Some(content(PNG)));
        assert!(state.shows_image());
        assert!(!state.is_failed);
    }

    // parity: TS-renderer-AiChatAttachment-5fded2
    #[test]
    fn an_image_only_message_shows_its_image_and_expands_without_a_copy_action() {
        let message = kanvibe_ai::types::AggregatedAiMessage::plain(
            kanvibe_ai::types::AiMessageRole::User,
            None,
            "",
        );
        assert!(!super::super::has_copy_action(&message));
        let mut state = ChatAttachmentState::default();
        let load = state.begin_load();
        state.finish_load(load, AiAttachmentKind::Image, Some(content(PNG)));
        assert!(state.shows_image());
        assert!(!state.is_expanded);
        state.is_expanded = !state.is_expanded;
        let labels = AttachmentLabels::from_translator(&Translator::new(
            kanvibe_domain::i18n::Locale::En,
            "taskDetail",
        ));
        assert_eq!(labels.close_image.as_ref(), "Collapse image");
        assert!(state.is_expanded);
    }

    // parity: TS-renderer-AiChatAttachment-189cef
    #[test]
    fn file_bytes_stay_unread_until_an_explicit_download() {
        assert!(!should_load_when_visible(AiAttachmentKind::File));
        assert!(should_load_when_visible(AiAttachmentKind::Image));
        let labels = AttachmentLabels::from_translator(&Translator::new(
            kanvibe_domain::i18n::Locale::En,
            "taskDetail",
        ));
        assert_eq!(
            format!("{}: notes.txt", labels.download),
            "Download: notes.txt"
        );
    }

    // parity: TS-renderer-AiChatAttachment-3b5b3e
    #[test]
    fn file_urls_and_active_image_markup_are_refused() {
        let mut state = ChatAttachmentState::default();
        let first = state.begin_load();
        state.finish_load(
            first,
            AiAttachmentKind::Image,
            Some(content("file:///owned/private.png")),
        );
        assert!(state.is_failed && !state.shows_image());
        let second = state.begin_load();
        state.finish_load(
            second,
            AiAttachmentKind::Image,
            Some(content("data:image/svg+xml;base64,PHN2Zy8+")),
        );
        assert!(state.is_failed && !state.shows_image());
        assert!(is_acceptable_media(
            AiAttachmentKind::File,
            "data:text/plain;base64,aGk="
        ));
    }
}
