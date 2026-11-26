//! 원격 세션 이미지 붙여넣기(TS `Terminal.tsx` `sendImageToRemoteTerminal`). 원격 CLI는 로컬 클립보드를 읽지 못하므로
//! 이미지를 scp로 보내고 원격 경로만 글로 붙인다. 브리지가 data URL을 받으므로 여기서 만든다.

use gpui_kit::{
    AnyElement, AppContext, ClipboardEntry, ClipboardItem, Context, Image, IntoElement, Window,
};
use kanvibe_domain::i18n::Translator;

use super::pane::TerminalPane;
use super::transport::ImagePasteOutcome;
use crate::components::toast::{Toast, ToastKind, toast_stack};
use crate::qa::faults::FaultOutcome;
use crate::services::{AppServices, apply_fault, format_service_error_for};

/// TS가 이미지 전송 실패 사유가 비었을 때 쓰던 문구
const IMAGE_TRANSFER_FAILED: &str = "이미지 전송 실패";
/// TS preload `pasteImageToRemoteTerminal`이 부르던 IPC 채널. QA 고장 주입의 대상 이름이다
pub const PASTE_IMAGE_CHANNEL: &str = "kanvibe:terminal-paste-image";

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 전송 진행·실패 알림 하나(TS `showActionStatus`). 실패 알림은 같은 이미지를 다시 보낼 수 있다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePasteToast {
    id: u64,
    kind: ToastKind,
    message: String,
    retry_image_data_url: Option<String>,
}

impl TerminalPane {
    /// 원격 세션이고 클립보드에 이미지가 있으면 보내고 `true`. 아니면 기본 처리를 그대로 둔다
    pub(super) fn try_paste_remote_image(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.target.is_remote {
            return false;
        }
        let Some(image) = cx.read_from_clipboard().as_ref().and_then(clipboard_image) else {
            return false;
        };
        self.send_image_to_remote(image_data_url(&image), cx);
        true
    }

    /// 전송은 바로 시작하되, 경로 쓰기는 붙여넣은 순서대로 한 줄로 세운다
    fn send_image_to_remote(&mut self, image_data_url: String, cx: &mut Context<Self>) {
        let Some(transport) = self.transport.clone() else {
            return;
        };
        let progress_id = self.push_image_toast(ToastKind::Progress, "imagePasteSending", None, cx);
        let task_id = self.target.task_id.clone();
        let sent_image = image_data_url.clone();
        let arguments = serde_json::json!([task_id, sent_image]).to_string();
        let fault = cx
            .global::<AppServices>()
            .check_fault(PASTE_IMAGE_CHANNEL, &arguments);
        let executor = cx.background_executor().clone();
        let transfer = cx.background_spawn(async move {
            if let FaultOutcome::Result(value) = &fault {
                return paste_outcome_from_fault_value(value);
            }
            if let Err(failure) = apply_fault(fault, &executor).await {
                // TS는 IPC 거절 메시지(`Error invoking remote method '<채널>': …`)를 그대로 보였다
                return ImagePasteOutcome::RequestFailed(format_service_error_for(
                    PASTE_IMAGE_CHANNEL,
                    &failure.reason,
                ));
            }
            transport.paste_image(&task_id, &sent_image)
        });
        let previous_write = self.image_write_queue.take();
        self.image_write_queue = Some(cx.spawn(async move |this, cx| {
            if let Some(previous_write) = previous_write {
                previous_write.await;
            }
            let outcome = transfer.await;
            let _ = this.update(cx, |pane, cx| {
                pane.finish_image_paste(outcome, image_data_url, cx);
                pane.image_toasts.retain(|toast| toast.id != progress_id);
                cx.notify();
            });
        }));
    }

    fn finish_image_paste(
        &mut self,
        outcome: ImagePasteOutcome,
        image_data_url: String,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            ImagePasteOutcome::Pasted(remote_path) => self.write_input(&remote_path),
            // 거절 사유는 세션을 끝내지 않으므로 연결 종료 띠가 아니라 닫을 수 있는 실패 알림으로 보인다.
            // 같은 이미지를 다시 보내도 같은 이유로 거절되므로 다시 보내기는 두지 않는다
            ImagePasteOutcome::Rejected(reason) => {
                let reason = match reason.is_empty() {
                    true => IMAGE_TRANSFER_FAILED.to_string(),
                    false => reason,
                };
                self.push_image_toast(
                    ToastKind::Failure,
                    "imagePasteFailed",
                    Some((reason, None)),
                    cx,
                );
            }
            ImagePasteOutcome::RequestFailed(reason) => {
                self.push_image_toast(
                    ToastKind::Failure,
                    "imagePasteFailed",
                    Some((reason, Some(image_data_url))),
                    cx,
                );
            }
        }
    }

    fn push_image_toast(
        &mut self,
        kind: ToastKind,
        message_key: &str,
        failure: Option<(String, Option<String>)>,
        cx: &mut Context<Self>,
    ) -> u64 {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        let (message, retry_image_data_url) = match failure {
            Some((reason, retry_image)) => (
                self.translator
                    .translate_with(message_key, &[("reason", &reason)]),
                retry_image,
            ),
            None => (self.translator.translate(message_key), None),
        };
        self.image_toasts.push(ImagePasteToast {
            id,
            kind,
            message,
            retry_image_data_url,
        });
        cx.notify();
        id
    }

    fn retry_image_paste(&mut self, toast_id: u64, cx: &mut Context<Self>) {
        let Some(index) = self
            .image_toasts
            .iter()
            .position(|toast| toast.id == toast_id)
        else {
            return;
        };
        let toast = self.image_toasts.remove(index);
        if let Some(image_data_url) = toast.retry_image_data_url {
            self.send_image_to_remote(image_data_url, cx);
        }
        cx.notify();
    }

    fn dismiss_image_toast(&mut self, toast_id: u64, cx: &mut Context<Self>) {
        self.image_toasts.retain(|toast| toast.id != toast_id);
        cx.notify();
    }

    pub(super) fn render_image_toasts(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.image_toasts.is_empty() {
            return None;
        }
        let common = Translator::new(self.translator.locale(), "common");
        let retry_label = self.translator.translate("imagePasteRetry");
        let toasts: Vec<Toast> = self
            .image_toasts
            .iter()
            .map(|toast| {
                let toast_id = toast.id;
                let mut element = Toast::new(
                    ("terminal-image-toast", toast_id),
                    toast.kind,
                    toast.message.clone(),
                    common.translate("close"),
                )
                .on_dismiss(
                    cx.listener(move |pane, _, _, cx| pane.dismiss_image_toast(toast_id, cx)),
                );
                if toast.retry_image_data_url.is_some() {
                    element = element.action(
                        retry_label.clone(),
                        cx.listener(move |pane, _, _, cx| pane.retry_image_paste(toast_id, cx)),
                    );
                }
                element
            })
            .collect();
        Some(toast_stack(window.viewport_size(), toasts).into_any_element())
    }
}

/// QA 고장 주입 값(TS 응답 `{ ok, remotePath, error }`)
fn paste_outcome_from_fault_value(value: &serde_json::Value) -> ImagePasteOutcome {
    let text = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    match (
        value.get("ok").and_then(serde_json::Value::as_bool),
        text("remotePath"),
    ) {
        (Some(true), Some(remote_path)) => ImagePasteOutcome::Pasted(remote_path),
        _ => ImagePasteOutcome::Rejected(text("error").unwrap_or_default()),
    }
}

/// 클립보드의 첫 이미지
pub fn clipboard_image(item: &ClipboardItem) -> Option<Image> {
    item.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image.clone()),
        _ => None,
    })
}

/// TS `readClipboardImage`가 돌려주던 `data:image/png;base64,...`
pub fn image_data_url(image: &Image) -> String {
    format!("data:image/png;base64,{}", encode_base64(&image.bytes))
}

fn encode_base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk.iter().enumerate().fold(0u32, |value, (index, byte)| {
            value | u32::from(*byte) << (16 - 8 * index)
        });
        let symbol_count = chunk.len() + 1;
        for index in 0..4 {
            match index < symbol_count {
                true => {
                    let sextet = (value >> (18 - 6 * index)) & 0x3f;
                    encoded.push(char::from(BASE64_ALPHABET[sextet as usize]));
                }
                false => encoded.push('='),
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_with_padding() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(&[0xff, 0xfe, 0xfd, 0x00]), "//79AA==");
    }
}
