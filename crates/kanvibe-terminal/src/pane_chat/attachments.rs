//! pane 채팅 첨부 파일 보관(TS `paneChatAttachmentService.ts`). 입력창에 올린 파일을 CLI가 읽을 경로에 두고,
//! 그 파일을 올린 CLI(작업·pane 토큰·CLI 식별자·호스트)만 다시 쓸 수 있게 묶어 둔다.

mod data_url;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use kanvibe_ai::javascript::{js_trim, utf16_length};
use kanvibe_ai::node_path;
use kanvibe_domain::pane_chat::MAX_PANE_CHAT_ATTACHMENTS;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use kanvibe_process::ssh_config::{SshConfigReader, SshHostConfig};
use kanvibe_process::worktree::quote_for_posix_shell;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use data_url::{DecodedAttachment, decode_attachment_data_url};

const ATTACHMENT_DIRECTORY_NAME: &str = "pane-chat-attachments";
const REMOTE_REMOVE_TIMEOUT: Duration = Duration::from_secs(5);
static ATTACHMENT_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i-u)\A[0-9a-f]{8}-[0-9a-f-]{27}\z").unwrap());

/// 첨부 파일을 올린 CLI. 네 값이 모두 같아야 같은 주인이다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatAttachmentOwner {
    pub task_id: String,
    pub target_token: String,
    pub identity: String,
    pub ssh_host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatAttachmentInput {
    pub name: String,
    pub mime_type: String,
    pub data_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatAttachmentKind {
    Image,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatAttachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub kind: PaneChatAttachmentKind,
    pub size_bytes: usize,
}

/// CLI에 넘길 첨부 파일 경로
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPaneChatAttachment {
    pub id: String,
    pub path: String,
    pub kind: PaneChatAttachmentKind,
}

/// 원격 CLI가 읽을 수 있도록 파일을 SSH 호스트에 올린다(TS `transferFileToRemoteHost`). 올린 원격 경로를 돌려준다
pub trait RemoteAttachmentUploader: Send + Sync {
    fn transfer_file_to_remote_host(
        &self,
        config: &SshHostConfig,
        content: &[u8],
        extension: &str,
    ) -> Result<String, String>;
}

#[derive(Debug, thiserror::Error)]
pub enum PaneChatAttachmentError {
    /// TS `PaneChatInputError`. 사용자에게 그대로 보여 주는 입력 오류다
    #[error("{0}")]
    Input(String),
    #[error("{0}")]
    RemoteTransfer(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Exec(#[from] ExecGitError),
}

fn input_error(message: &str) -> PaneChatAttachmentError {
    PaneChatAttachmentError::Input(message.to_string())
}

#[derive(Debug, Clone)]
struct StoredAttachment {
    owner: PaneChatAttachmentOwner,
    attachment: PaneChatAttachment,
    path: String,
    /// 로컬에 만든 draft 디렉터리. 원격 첨부는 `None`이다
    directory: Option<PathBuf>,
    is_retained: bool,
    content_hash: String,
}

#[derive(Default)]
struct AttachmentRegistry {
    staged: HashMap<String, StoredAttachment>,
    /// 주인별로 아직 저장 중인 첨부 수. 동시에 올린 파일이 개수 상한을 함께 넘지 않게 센다
    pending: Vec<(PaneChatAttachmentOwner, usize)>,
}

impl AttachmentRegistry {
    fn pending_count(&self, owner: &PaneChatAttachmentOwner) -> usize {
        self.pending
            .iter()
            .find(|(pending_owner, _)| pending_owner == owner)
            .map_or(0, |(_, count)| *count)
    }

    fn add_pending(&mut self, owner: &PaneChatAttachmentOwner) {
        match self
            .pending
            .iter_mut()
            .find(|(pending_owner, _)| pending_owner == owner)
        {
            Some((_, count)) => *count += 1,
            None => self.pending.push((owner.clone(), 1)),
        }
    }

    fn release_pending(&mut self, owner: &PaneChatAttachmentOwner) {
        if let Some((_, count)) = self
            .pending
            .iter_mut()
            .find(|(pending_owner, _)| pending_owner == owner)
        {
            *count -= 1;
        }
        self.pending.retain(|(_, count)| *count > 0);
    }

    fn unsent_count(&self, owner: &PaneChatAttachmentOwner) -> usize {
        self.staged
            .values()
            .filter(|entry| &entry.owner == owner && !entry.is_retained)
            .count()
    }
}

/// 앱이 하나를 만들어 pane 채팅 서비스가 쓴다
pub struct PaneChatAttachmentStore {
    app_data_directory: PathBuf,
    runner: Arc<dyn ShellCommandRunner>,
    ssh_config_reader: Arc<SshConfigReader>,
    remote_uploader: Arc<dyn RemoteAttachmentUploader>,
    registry: Mutex<AttachmentRegistry>,
}

fn validate_attachment_name(name: &str) -> Result<(), PaneChatAttachmentError> {
    let has_control_character = name
        .chars()
        .any(|character| character <= '\u{1f}' || character == '\u{7f}');
    if js_trim(name).is_empty() || utf16_length(name) > 255 || has_control_character {
        return Err(input_error("첨부 파일 이름·형식이 올바르지 않습니다."));
    }
    Ok(())
}

fn image_extension(mime_type: &str) -> Option<&'static str> {
    match mime_type {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// 이미지가 아니면 원래 이름의 확장자를 영숫자 12자까지만 남겨 쓴다
fn file_extension(name: &str) -> String {
    let lowered = node_path::extname(name).to_lowercase();
    let sanitized: String = lowered
        .chars()
        .skip(1)
        .filter(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        .take(12)
        .collect();
    if sanitized.is_empty() {
        "bin".to_string()
    } else {
        sanitized
    }
}

/// Node `mkdtemp(prefix)`처럼 무작위 6글자를 붙인 새 디렉터리를 0700으로 만든다
fn create_draft_directory(root: &Path) -> std::io::Result<PathBuf> {
    loop {
        let suffix: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(6)
            .collect();
        let directory = root.join(format!("draft-{suffix}"));
        match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

fn write_private_file(file_path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(file_path)?;
    file.write_all(content)
}

fn remove_directory_forcefully(directory: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(directory) {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// 목록의 첨부가 모두 `owner`의 것일 때만 경로를 돌려준다. 하나라도 아니면 전체를 거절한다
fn resolve_owned_attachments(
    registry: &AttachmentRegistry,
    owner: &PaneChatAttachmentOwner,
    ids: &[String],
) -> Result<Vec<ResolvedPaneChatAttachment>, PaneChatAttachmentError> {
    let unique_ids: HashSet<&String> = ids.iter().collect();
    if ids.len() > MAX_PANE_CHAT_ATTACHMENTS || unique_ids.len() != ids.len() {
        return Err(input_error("첨부 파일 목록이 올바르지 않습니다."));
    }
    ids.iter()
        .map(|id| {
            let entry = ATTACHMENT_ID
                .is_match(id)
                .then(|| registry.staged.get(id))
                .flatten()
                .filter(|entry| &entry.owner == owner)
                .ok_or_else(|| input_error("현재 CLI에 속하지 않거나 만료된 첨부 파일입니다."))?;
            Ok(ResolvedPaneChatAttachment {
                id: id.clone(),
                path: entry.path.clone(),
                kind: entry.attachment.kind,
            })
        })
        .collect()
}

/// 저장 결과. 로컬 draft 디렉터리는 실패하면 지워야 하므로 경로와 함께 돌려준다
struct StagedLocation {
    path: String,
    directory: Option<PathBuf>,
}

impl PaneChatAttachmentStore {
    /// `app_data_directory`는 앱 데이터 디렉터리다. 로컬 첨부는 그 아래 `pane-chat-attachments`에 둔다
    pub fn new(
        app_data_directory: PathBuf,
        runner: Arc<dyn ShellCommandRunner>,
        ssh_config_reader: Arc<SshConfigReader>,
        remote_uploader: Arc<dyn RemoteAttachmentUploader>,
    ) -> Self {
        Self {
            app_data_directory,
            runner,
            ssh_config_reader,
            remote_uploader,
            registry: Mutex::new(AttachmentRegistry::default()),
        }
    }

    fn registry(&self) -> std::sync::MutexGuard<'_, AttachmentRegistry> {
        self.registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn store_pane_chat_attachment(
        &self,
        owner: &PaneChatAttachmentOwner,
        input: &PaneChatAttachmentInput,
    ) -> Result<PaneChatAttachment, PaneChatAttachmentError> {
        validate_attachment_name(&input.name)?;
        let content = decode_attachment_data_url(&input.data_url, &input.mime_type)
            .map_err(|message| PaneChatAttachmentError::Input(message.to_string()))?;
        {
            let mut registry = self.registry();
            if registry.unsent_count(owner) + registry.pending_count(owner)
                >= MAX_PANE_CHAT_ATTACHMENTS
            {
                return Err(input_error(
                    "첨부 파일은 한 번에 10개까지 추가할 수 있습니다.",
                ));
            }
            registry.add_pending(owner);
        }
        let staged = self.stage_attachment(owner, input, content);
        // TS는 staged 등록과 pending 해제가 같은 동기 구간이라 다른 요청이 둘을 겹쳐 세지 않는다. 같은 잠금 안에서 함께 바꾼다
        let mut registry = self.registry();
        registry.release_pending(owner);
        let (id, stored) = staged?;
        let attachment = stored.attachment.clone();
        registry.staged.insert(id, stored);
        Ok(attachment)
    }

    fn stage_attachment(
        &self,
        owner: &PaneChatAttachmentOwner,
        input: &PaneChatAttachmentInput,
        content: DecodedAttachment,
    ) -> Result<(String, StoredAttachment), PaneChatAttachmentError> {
        let id = uuid::Uuid::new_v4().to_string();
        let name = node_path::basename(&input.name.replace('\\', "/"));
        let image_extension = image_extension(&content.mime_type);
        let extension = image_extension.map_or_else(|| file_extension(&name), str::to_string);
        let location = match owner.ssh_host.as_deref().filter(|host| !host.is_empty()) {
            Some(ssh_host) => {
                self.upload_remote_attachment(ssh_host, &content.bytes, &extension)?
            }
            None => self.write_local_attachment(&id, &content.bytes, &extension)?,
        };
        let attachment = PaneChatAttachment {
            id: id.clone(),
            name,
            mime_type: content.mime_type,
            kind: if image_extension.is_some() {
                PaneChatAttachmentKind::Image
            } else {
                PaneChatAttachmentKind::File
            },
            size_bytes: content.bytes.len(),
        };
        let stored = StoredAttachment {
            owner: owner.clone(),
            attachment,
            path: location.path,
            directory: location.directory,
            is_retained: false,
            content_hash: hex::encode(Sha256::digest(&content.bytes)),
        };
        Ok((id, stored))
    }

    fn upload_remote_attachment(
        &self,
        ssh_host: &str,
        content: &[u8],
        extension: &str,
    ) -> Result<StagedLocation, PaneChatAttachmentError> {
        let config = self
            .ssh_config_reader
            .parse_ssh_config()
            .into_iter()
            .find(|entry| entry.host == ssh_host)
            .ok_or_else(|| input_error("첨부 파일을 받을 SSH 호스트를 찾을 수 없습니다."))?;
        let remote_path = self
            .remote_uploader
            .transfer_file_to_remote_host(&config, content, extension)
            .map_err(PaneChatAttachmentError::RemoteTransfer)?;
        Ok(StagedLocation {
            path: remote_path,
            directory: None,
        })
    }

    /// 실패하면 만든 draft 디렉터리를 지우고 오류를 그대로 돌려준다
    fn write_local_attachment(
        &self,
        id: &str,
        content: &[u8],
        extension: &str,
    ) -> Result<StagedLocation, PaneChatAttachmentError> {
        fs::create_dir_all(&self.app_data_directory)?;
        let root = self.app_data_directory.join(ATTACHMENT_DIRECTORY_NAME);
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)?;
        let directory = create_draft_directory(&root)?;
        let file_path = directory.join(format!("{id}.{extension}"));
        if let Err(error) = write_private_file(&file_path, content) {
            remove_directory_forcefully(&directory)?;
            return Err(error.into());
        }
        Ok(StagedLocation {
            path: file_path.to_string_lossy().into_owned(),
            directory: Some(directory),
        })
    }

    pub fn resolve_pane_chat_attachments(
        &self,
        owner: &PaneChatAttachmentOwner,
        ids: &[String],
    ) -> Result<Vec<ResolvedPaneChatAttachment>, PaneChatAttachmentError> {
        resolve_owned_attachments(&self.registry(), owner, ids)
    }

    /// 전송한 첨부는 입력창에서 칩을 지워도 CLI가 계속 읽을 수 있게 남긴다
    pub fn retain_pane_chat_attachments(
        &self,
        owner: &PaneChatAttachmentOwner,
        ids: &[String],
    ) -> Result<(), PaneChatAttachmentError> {
        let mut registry = self.registry();
        for entry in resolve_owned_attachments(&registry, owner, ids)? {
            if let Some(stored) = registry.staged.get_mut(&entry.id) {
                stored.is_retained = true;
            }
        }
        Ok(())
    }

    /// 재등록으로 경로·ID가 바뀌어도 같은 전송의 첨부인지 확인한다. 바이트나 다른 owner의 파일을 외부에 노출하지 않는다
    pub fn fingerprint_pane_chat_attachments(
        &self,
        owner: &PaneChatAttachmentOwner,
        ids: &[String],
    ) -> Result<String, PaneChatAttachmentError> {
        let registry = self.registry();
        let resolved = resolve_owned_attachments(&registry, owner, ids)?;
        let fingerprints: Vec<serde_json::Value> = resolved
            .iter()
            .map(|entry| {
                let stored = &registry.staged[&entry.id];
                let attachment = &stored.attachment;
                serde_json::json!([
                    attachment.name,
                    attachment.mime_type,
                    attachment.size_bytes,
                    stored.content_hash
                ])
            })
            .collect();
        Ok(serde_json::Value::Array(fingerprints).to_string())
    }

    pub fn discard_pane_chat_attachment(
        &self,
        task_id: &str,
        target_token: &str,
        id: &str,
    ) -> Result<(), PaneChatAttachmentError> {
        if !ATTACHMENT_ID.is_match(id) {
            return Err(input_error("첨부 파일 식별자가 올바르지 않습니다."));
        }
        let Some(entry) = self.registry().staged.get(id).cloned() else {
            return Ok(());
        };
        if entry.owner.task_id != task_id || entry.owner.target_token != target_token {
            return Err(input_error("현재 CLI에 속하지 않는 첨부 파일입니다."));
        }
        if !entry.is_retained {
            self.remove_staged_file(&entry)?;
        }
        self.registry().staged.remove(id);
        Ok(())
    }

    fn remove_staged_file(&self, entry: &StoredAttachment) -> Result<(), PaneChatAttachmentError> {
        if let Some(ssh_host) = entry
            .owner
            .ssh_host
            .as_deref()
            .filter(|host| !host.is_empty())
        {
            let options = ExecGitOptions {
                timeout: Some(REMOTE_REMOVE_TIMEOUT),
                ..ExecGitOptions::default()
            };
            let command = format!("rm -f -- {}", quote_for_posix_shell(&entry.path));
            self.runner.exec_git(&command, Some(ssh_host), &options)?;
        } else if let Some(directory) = &entry.directory {
            remove_directory_forcefully(directory)?;
        }
        Ok(())
    }
}
