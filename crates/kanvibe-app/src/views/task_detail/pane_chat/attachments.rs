//! 입력창 첨부(TS `usePaneChatComposer`의 `addFiles`·`upload`·`remove`·`retry`). 파일 선택, 붙여넣은 이미지, 끌어 놓은 파일을
//! 같은 길로 받아 지금 CLI 대상에 올린다. 올리는 동안 대상이 바뀌었거나 화면이 닫혔으면 올린 파일을 곧바로 지운다.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{
    AppContext, ClipboardEntry, Context, ExternalPaths, Image, ImageFormat, PathPromptOptions,
    Window,
};

use super::client::AttachmentUpload;
use super::drafts::{AttachmentError, AttachmentStatus, DraftAttachment, DraftError, DraftOwner};
use super::view::PaneChatView;

/// 첨부로 받을 파일 하나
#[derive(Debug, Clone)]
pub struct PickedFile {
    pub name: String,
    pub mime_type: String,
    pub bytes: Arc<Vec<u8>>,
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk.iter().enumerate().fold(0u32, |value, (index, byte)| {
            value | u32::from(*byte) << (16 - 8 * index)
        });
        for index in 0..4 {
            match index <= chunk.len() {
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

/// 브라우저 `FileReader.readAsDataURL`과 같은 `data:형식;base64,…`
pub fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    format!("data:{mime_type};base64,{}", encode_base64(bytes))
}

/// 파일 이름의 확장자로 정한 형식. 모르는 형식은 TS처럼 `application/octet-stream`이다
pub fn mime_type_for(name: &str) -> String {
    let extension = name.rsplit('.').next().unwrap_or_default().to_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "csv" => "text/csv",
        "html" => "text/html",
        _ => "application/octet-stream",
    };
    mime.to_string()
}

fn image_format(mime_type: &str) -> Option<ImageFormat> {
    match mime_type {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::Webp),
        "image/bmp" => Some(ImageFormat::Bmp),
        "image/svg+xml" => Some(ImageFormat::Svg),
        _ => None,
    }
}

/// 붙여넣은 클립보드 이미지를 실제 형식 그대로 첨부한다(TS `clipboardData.files`의 `File.type`)
fn clipboard_image_file(image: &Image) -> PickedFile {
    PickedFile {
        name: format!("image.{}", image.format.extension()),
        mime_type: image.format.mime_type().to_string(),
        bytes: Arc::new(image.bytes.clone()),
    }
}

fn read_picked_files(paths: Vec<PathBuf>) -> Vec<PickedFile> {
    paths
        .into_iter()
        .filter_map(|path| {
            let bytes = std::fs::read(&path).ok()?;
            let name = path.file_name()?.to_string_lossy().to_string();
            Some(PickedFile {
                mime_type: mime_type_for(&name),
                name,
                bytes: Arc::new(bytes),
            })
        })
        .collect()
}

impl PaneChatView {
    /// 파일을 첨부로 더한다. 개수 상한을 넘으면 하나도 더하지 않고, 크기 상한을 넘는 파일은 올리지 않고 안내한다
    pub fn add_files(
        &mut self,
        files: Vec<PickedFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = self.session.snapshot() else {
            return;
        };
        let (Some(token), Some(capabilities)) =
            (snapshot.target_token.clone(), snapshot.composer.clone())
        else {
            return;
        };
        if !capabilities.can_attach || files.is_empty() {
            return;
        }
        let key = self.composer.draft_key.clone();
        let remaining = capabilities
            .max_attachments
            .saturating_sub(self.composer.draft().attachments.len() as u64);
        if files.len() as u64 > remaining {
            self.composer.drafts.entry(&key).error = Some(DraftError::AttachmentLimit);
            cx.notify();
            return;
        }
        let items: Vec<DraftAttachment> = files
            .into_iter()
            .map(|file| self.new_attachment(file, capabilities.max_attachment_bytes))
            .collect();
        let uploads: Vec<u64> = items
            .iter()
            .filter(|item| item.status == AttachmentStatus::Uploading)
            .map(|item| item.local_id)
            .collect();
        let draft = self.composer.drafts.entry(&key);
        draft.owner = Some(DraftOwner {
            task_id: self.session.task_id.clone(),
            target_token: token.clone(),
        });
        draft.error = None;
        draft.attachments.extend(items);
        for local_id in uploads {
            self.upload_attachment(&key, &token, local_id, window, cx);
        }
        cx.notify();
    }

    fn new_attachment(&mut self, file: PickedFile, max_bytes: u64) -> DraftAttachment {
        self.composer.next_local_id += 1;
        let size_bytes = file.bytes.len() as u64;
        let is_too_large = size_bytes > max_bytes;
        let preview = image_format(&file.mime_type)
            .map(|format| Arc::new(Image::from_bytes(format, file.bytes.as_ref().clone())));
        DraftAttachment {
            local_id: self.composer.next_local_id,
            name: file.name,
            mime_type: file.mime_type,
            size_bytes,
            bytes: file.bytes,
            preview,
            staged_id: None,
            status: match is_too_large {
                true => AttachmentStatus::Failed,
                false => AttachmentStatus::Uploading,
            },
            error: is_too_large.then_some(AttachmentError::TooLarge),
        }
    }

    /// 첨부 하나를 올린다. 화면이 닫혔거나 대상이 바뀐 뒤 올라갔으면 곧바로 지운다
    fn upload_attachment(
        &mut self,
        key: &str,
        token: &str,
        local_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self.composer.drafts.entry(key).attachment_mut(local_id) else {
            return;
        };
        item.status = AttachmentStatus::Uploading;
        item.error = None;
        let upload = AttachmentUpload {
            name: item.name.clone(),
            mime_type: item.mime_type.clone(),
            data_url: data_url(&item.mime_type, &item.bytes),
        };
        let request = self
            .client
            .stage_attachment(&self.session.task_id, token, &upload, cx);
        let (key, token) = (key.to_string(), token.to_string());
        let client = self.client.clone();
        let task_id = self.session.task_id.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let staged_id = result.as_ref().ok().cloned();
            let kept = this
                .update(cx, |view, cx| {
                    view.finish_upload(&key, &token, local_id, result, cx)
                })
                .unwrap_or(false);
            if let (Some(staged_id), false) = (staged_id, kept) {
                // 창이 닫혔어도 앱 문맥으로 지운다
                let app: &gpui_kit::AsyncApp = cx;
                let removal =
                    app.update(|cx| client.remove_attachment(&task_id, &token, &staged_id, cx));
                let _ = removal.await;
            }
        })
        .detach();
    }

    /// 올린 결과를 붙인다. 올린 파일을 계속 쓸 수 있으면 `true`, 버려야 하면 `false`다
    fn finish_upload(
        &mut self,
        key: &str,
        token: &str,
        local_id: u64,
        result: super::client::PaneChatResult<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let draft = self.composer.drafts.entry(key);
        if draft
            .owner
            .as_ref()
            .map(|owner| owner.target_token.as_str())
            != Some(token)
        {
            return false;
        }
        let Some(status) = draft
            .attachments
            .iter()
            .find(|item| item.local_id == local_id)
            .map(|item| item.status)
        else {
            return false;
        };
        cx.notify();
        if result.is_ok() && status == AttachmentStatus::Removing {
            draft.attachments.retain(|item| item.local_id != local_id);
            return false;
        }
        let Some(item) = draft.attachment_mut(local_id) else {
            return false;
        };
        match result {
            Ok(staged_id) => {
                item.staged_id = Some(staged_id);
                item.status = AttachmentStatus::Ready;
                item.error = None;
            }
            Err(error) => {
                item.status = match item.staged_id.is_some() {
                    true => AttachmentStatus::Ready,
                    false => AttachmentStatus::Failed,
                };
                item.error = Some(AttachmentError::Service(error.message));
            }
        }
        true
    }

    /// 첨부 제거. 올리는 중이면 올라온 뒤 지우고, 지우지 못하면 남겨 다시 지울 수 있게 한다
    pub fn remove_attachment(
        &mut self,
        local_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = self.composer.draft_key.clone();
        let token = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.target_token.clone());
        let draft = self.composer.drafts.entry(&key);
        if draft.is_sending {
            return;
        }
        let Some(item) = draft
            .attachment_mut(local_id)
            .filter(|item| item.status != AttachmentStatus::Removing)
        else {
            return;
        };
        if item.status == AttachmentStatus::Uploading {
            item.status = AttachmentStatus::Removing;
            cx.notify();
            return;
        }
        let (Some(staged_id), Some(token)) = (item.staged_id.clone(), token) else {
            draft.attachments.retain(|item| item.local_id != local_id);
            cx.notify();
            return;
        };
        item.status = AttachmentStatus::Removing;
        item.error = None;
        let request = self
            .client
            .remove_attachment(&self.session.task_id, &token, &staged_id, cx);
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |view, cx| {
                view.finish_removal(&key, local_id, result, cx)
            });
        })
        .detach();
        cx.notify();
    }

    /// 지웠으면 목록에서 빼고, 지우지 못했으면 다시 지울 수 있게 남기고 이유를 보인다
    fn finish_removal(
        &mut self,
        key: &str,
        local_id: u64,
        result: super::client::PaneChatResult<bool>,
        cx: &mut Context<Self>,
    ) {
        let draft = self.composer.drafts.entry(key);
        match result {
            Ok(_) => draft.attachments.retain(|item| item.local_id != local_id),
            Err(error) => {
                if let Some(item) = draft.attachment_mut(local_id) {
                    item.status = AttachmentStatus::Ready;
                    item.error = Some(AttachmentError::Service(error.message));
                }
            }
        }
        cx.notify();
    }

    /// 실패한 첨부를 같은 파일로 다시 올린다. 크기 상한을 넘은 파일은 다시 올리지 않는다
    pub fn retry_attachment(&mut self, local_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.session.snapshot() else {
            return;
        };
        let max_bytes = snapshot
            .composer
            .as_ref()
            .map(|composer| composer.max_attachment_bytes)
            .unwrap_or(0);
        let Some(token) = snapshot.target_token.clone() else {
            return;
        };
        let key = self.composer.draft_key.clone();
        let is_retryable = self.composer.draft().attachments.iter().any(|item| {
            item.local_id == local_id
                && item.status == AttachmentStatus::Failed
                && item.size_bytes <= max_bytes
        });
        if is_retryable {
            self.upload_attachment(&key, &token, local_id, window, cx);
            cx.notify();
        }
    }

    /// 대상이 바뀌었거나 화면을 닫아 더 쓰지 않을 올린 첨부를 서비스에서 지운다
    pub(super) fn discard_staged(
        &self,
        owner: &DraftOwner,
        staged_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        for staged_id in staged_ids {
            self.client
                .remove_attachment(&owner.task_id, &owner.target_token, &staged_id, cx)
                .detach();
        }
    }

    /// 화면이 닫힐 때 보내는 중이 아닌 모든 초안의 올린 첨부를 지운다. 보내는 중인 초안은 결과를 안 뒤에 정리된다
    pub(super) fn discard_abandoned_drafts(&self, cx: &mut gpui_kit::App) {
        for draft in self.composer.drafts.all().filter(|draft| !draft.is_sending) {
            let Some(owner) = draft.owner.as_ref() else {
                continue;
            };
            for staged_id in draft.staged_ids() {
                self.client
                    .remove_attachment(&owner.task_id, &owner.target_token, &staged_id, cx)
                    .detach();
            }
        }
    }

    /// "파일"·"사진" 버튼. 고른 파일을 읽어 첨부로 더한다
    pub fn pick_attachments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let files = cx
                .background_spawn(async move { read_picked_files(paths) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| view.add_files(files, window, cx));
        })
        .detach();
    }

    /// 입력창에 붙여넣은 이미지. 첨부를 받는 CLI면 글로 붙이지 않고 첨부로 더한다
    pub fn paste_clipboard_image(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let can_attach = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.composer.as_ref())
            .is_some_and(|composer| composer.can_attach);
        let image = cx.read_from_clipboard().and_then(|item| {
            item.entries().iter().find_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = image.filter(|_| can_attach) else {
            return false;
        };
        let file = clipboard_image_file(&image);
        self.add_files(vec![file], window, cx);
        true
    }

    /// 입력창에 끌어 놓은 파일
    pub fn drop_paths(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = paths.paths().to_vec();
        cx.spawn_in(window, async move |this, cx| {
            let files = cx
                .background_spawn(async move { read_picked_files(paths) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| view.add_files(files, window, cx));
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_url_matches_file_reader() {
        assert_eq!(
            data_url("image/png", &[137, 80, 78, 71]),
            "data:image/png;base64,iVBORw=="
        );
        assert_eq!(mime_type_for("notes.txt"), "text/plain");
        assert_eq!(mime_type_for("archive.bin"), "application/octet-stream");
    }

    #[test]
    fn a_pasted_clipboard_image_keeps_its_own_format() {
        let jpeg = Image::from_bytes(ImageFormat::Jpeg, vec![0xFF, 0xD8, 0xFF]);
        let gif = Image::from_bytes(ImageFormat::Gif, b"GIF89a".to_vec());

        let jpeg_file = clipboard_image_file(&jpeg);
        let gif_file = clipboard_image_file(&gif);

        assert_eq!(
            (jpeg_file.name.as_str(), jpeg_file.mime_type.as_str()),
            ("image.jpg", "image/jpeg")
        );
        assert_eq!(*jpeg_file.bytes, vec![0xFF, 0xD8, 0xFF]);
        assert_eq!(
            (gif_file.name.as_str(), gif_file.mime_type.as_str()),
            ("image.gif", "image/gif")
        );
    }
}
