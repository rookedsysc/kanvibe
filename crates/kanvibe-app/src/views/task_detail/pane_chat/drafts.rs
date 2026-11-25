//! 입력창 초안(TS `usePaneChatComposer`의 상태). 초안은 작업과 대화(없으면 CLI 대상)마다 따로 두어, pane이나 대화가 바뀌면
//! 그 대화의 글과 첨부가 돌아온다. 같은 대화에서 대상 토큰만 새로 받으면 올려 둔 첨부는 새 대상에 다시 올려야 한다.

use std::collections::HashMap;
use std::sync::Arc;

use kanvibe_ai::pane_chat::snapshot::PaneChatSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentStatus {
    Uploading,
    Ready,
    Failed,
    Removing,
}

/// 첨부 한 개의 안내. 번역 문구 둘과 서비스가 쓴 글이다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentError {
    TooLarge,
    TargetChanged,
    Service(String),
}

#[derive(Debug, Clone)]
pub struct DraftAttachment {
    pub local_id: u64,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub bytes: Arc<Vec<u8>>,
    /// 이미지면 미리보기
    pub preview: Option<Arc<gpui_kit::Image>>,
    pub staged_id: Option<String>,
    pub status: AttachmentStatus,
    pub error: Option<AttachmentError>,
}

/// 초안 전체의 안내(TS `composer.error`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftError {
    SendFailed,
    AttachmentLimit,
}

/// 첨부를 올린 작업과 CLI 대상
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftOwner {
    pub task_id: String,
    pub target_token: String,
}

/// 같은 글·첨부로 다시 보내면 같은 요청 식별값을 쓴다. 전달됐는지 모르는 요청을 두 번 넣지 않기 위해서다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftRequest {
    pub fingerprint: String,
    pub id: String,
}

#[derive(Debug, Clone, Default)]
pub struct Draft {
    pub owner: Option<DraftOwner>,
    pub text: String,
    pub revision: u64,
    pub attachments: Vec<DraftAttachment>,
    pub is_sending: bool,
    pub error: Option<DraftError>,
    pub request: Option<DraftRequest>,
}

impl Draft {
    /// 모두 올렸고 안내가 없는지. 아니면 보낼 수 없다
    pub fn has_unsettled_attachment(&self) -> bool {
        self.attachments
            .iter()
            .any(|item| item.status != AttachmentStatus::Ready || item.error.is_some())
    }

    pub fn has_content(&self) -> bool {
        !self.text.trim().is_empty() || !self.attachments.is_empty()
    }

    pub fn fingerprint(&self) -> String {
        let local_ids: Vec<u64> = self.attachments.iter().map(|item| item.local_id).collect();
        format!("{:?}|{local_ids:?}", self.text)
    }

    pub fn attachment_mut(&mut self, local_id: u64) -> Option<&mut DraftAttachment> {
        self.attachments
            .iter_mut()
            .find(|item| item.local_id == local_id)
    }

    /// 아직 서비스에 올라가 있는 첨부(지우는 중인 것은 빼고)
    pub fn staged_ids(&self) -> Vec<String> {
        self.attachments
            .iter()
            .filter(|item| item.status != AttachmentStatus::Removing)
            .filter_map(|item| item.staged_id.clone())
            .collect()
    }
}

/// 초안이 속한 대화(TS `JSON.stringify([taskId, conversationKey ?? targetToken ?? "unavailable"])`)
pub fn draft_key(task_id: &str, snapshot: Option<&PaneChatSnapshot>) -> String {
    let conversation = snapshot
        .and_then(|snapshot| {
            snapshot
                .conversation_key
                .clone()
                .or_else(|| snapshot.target_token.clone())
        })
        .unwrap_or_else(|| "unavailable".to_string());
    format!("{task_id:?}|{conversation:?}")
}

#[derive(Debug, Default)]
pub struct Drafts {
    drafts: HashMap<String, Draft>,
}

impl Drafts {
    pub fn get(&self, key: &str) -> Option<&Draft> {
        self.drafts.get(key)
    }

    pub fn entry(&mut self, key: &str) -> &mut Draft {
        self.drafts.entry(key.to_string()).or_default()
    }

    pub fn all(&self) -> impl Iterator<Item = &Draft> {
        self.drafts.values()
    }

    /// 같은 대화의 대상 토큰이 새로 바뀌었으면, 올려 둔 첨부를 버릴 목록으로 돌려주고 첨부를 다시 올리게 표시한다.
    /// 보내는 중인 초안은 결과를 알 때까지 건드리지 않는다
    pub fn renew_owner(
        &mut self,
        key: &str,
        owner: DraftOwner,
    ) -> Option<(DraftOwner, Vec<String>)> {
        let draft = self.drafts.get_mut(key)?;
        let previous = draft.owner.clone()?;
        if previous.target_token == owner.target_token || draft.is_sending {
            return None;
        }
        let abandoned = draft.staged_ids();
        draft.owner = Some(owner);
        for item in &mut draft.attachments {
            item.staged_id = None;
            item.status = AttachmentStatus::Failed;
            item.error = Some(AttachmentError::TargetChanged);
        }
        Some((previous, abandoned))
    }
}
