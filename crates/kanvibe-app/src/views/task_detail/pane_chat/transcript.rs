//! 최신 조회 결과와 사용자가 펼친 이전 페이지를 같은 대화에만 합친다(TS `usePaneChatTranscript`).
//! 기록은 최신순이고, 이전 페이지는 그 뒤에 붙는다.

use std::collections::HashSet;

use kanvibe_ai::pane_chat::snapshot::{PaneChatSnapshot, PaneChatTranscript};
use kanvibe_ai::types::AggregatedAiMessage;

/// 펼친 이전 페이지가 어느 대화의 것인지 가리는 값
pub fn transcript_key(snapshot: Option<&PaneChatSnapshot>) -> String {
    let Some(snapshot) = snapshot else {
        return String::new();
    };
    let conversation = snapshot
        .conversation_key
        .as_ref()
        .or(snapshot.transcript.session_id.as_ref());
    format!(
        "{:?}|{:?}|{:?}",
        snapshot.task_id, snapshot.pane_id, conversation
    )
}

fn message_identity(message: &AggregatedAiMessage) -> String {
    let text = match message.full_text.is_empty() {
        true => &message.text,
        false => &message.full_text,
    };
    let attachment_ids: Option<Vec<&str>> = message
        .attachments
        .as_ref()
        .map(|items| items.iter().map(|item| item.id.as_str()).collect());
    format!(
        "{:?}|{:?}|{text:?}|{attachment_ids:?}",
        message.role, message.timestamp
    )
}

#[derive(Debug, Clone)]
struct OlderPage {
    key: String,
    messages: Vec<AggregatedAiMessage>,
    next_cursor: Option<String>,
}

#[derive(Debug, Default)]
pub struct TranscriptPages {
    page: Option<OlderPage>,
    /// 이전 페이지를 읽는 중인 대화
    loading_key: Option<String>,
    error_key: Option<String>,
}

impl TranscriptPages {
    fn own_page(&self, key: &str) -> Option<&OlderPage> {
        self.page.as_ref().filter(|page| page.key == key)
    }

    /// 최신 조회에 새로 들어온 메시지를 펼친 페이지 앞에 붙여, 조회가 앞부분만 돌려줘도 펼친 기록이 끊기지 않게 한다
    pub fn absorb_latest(&mut self, snapshot: &PaneChatSnapshot) {
        let key = transcript_key(Some(snapshot));
        let Some(page) = self.page.as_mut().filter(|page| page.key == key) else {
            return;
        };
        let known: HashSet<String> = page.messages.iter().map(message_identity).collect();
        let mut added: Vec<AggregatedAiMessage> = snapshot
            .transcript
            .messages
            .iter()
            .filter(|message| !known.contains(&message_identity(message)))
            .cloned()
            .collect();
        if !added.is_empty() {
            added.append(&mut page.messages);
            page.messages = added;
        }
    }

    /// 최신 기록과 펼친 페이지를 합쳐 같은 메시지는 한 번만 남긴다
    pub fn messages(&self, snapshot: Option<&PaneChatSnapshot>) -> Vec<AggregatedAiMessage> {
        let key = transcript_key(snapshot);
        let latest = snapshot.map(|snapshot| snapshot.transcript.messages.as_slice());
        let older = self.own_page(&key).map(|page| page.messages.as_slice());
        let mut seen: HashSet<String> = HashSet::new();
        latest
            .unwrap_or_default()
            .iter()
            .chain(older.unwrap_or_default())
            .filter(|message| seen.insert(message_identity(message)))
            .cloned()
            .collect()
    }

    pub fn next_cursor(&self, snapshot: Option<&PaneChatSnapshot>) -> Option<String> {
        let key = transcript_key(snapshot);
        match self.own_page(&key) {
            Some(page) => page.next_cursor.clone(),
            None => snapshot.and_then(|snapshot| snapshot.transcript.next_cursor.clone()),
        }
    }

    pub fn is_loading(&self, snapshot: Option<&PaneChatSnapshot>) -> bool {
        self.loading_key.as_deref() == Some(transcript_key(snapshot).as_str())
    }

    pub fn has_error(&self, snapshot: Option<&PaneChatSnapshot>) -> bool {
        self.error_key.as_deref() == Some(transcript_key(snapshot).as_str())
    }

    /// 이전 페이지 읽기를 시작한다. 같은 대화를 이미 읽는 중이면 `None`이다
    pub fn begin_loading(&mut self, snapshot: &PaneChatSnapshot) -> Option<String> {
        let key = transcript_key(Some(snapshot));
        if self.loading_key.as_deref() == Some(key.as_str()) {
            return None;
        }
        self.loading_key = Some(key.clone());
        self.error_key = None;
        Some(key)
    }

    /// 읽은 이전 페이지를 붙인다. 그사이 대화가 바뀌었으면 버린다
    pub fn finish_loading(
        &mut self,
        key: &str,
        snapshot: Option<&PaneChatSnapshot>,
        result: Result<PaneChatTranscript, ()>,
    ) {
        if self.loading_key.as_deref() == Some(key) {
            self.loading_key = None;
        }
        let Some(snapshot) = snapshot.filter(|snapshot| transcript_key(Some(snapshot)) == key)
        else {
            return;
        };
        let Ok(older) = result else {
            self.error_key = Some(key.to_string());
            return;
        };
        if older.session_id != snapshot.transcript.session_id {
            return;
        }
        let mut messages = self
            .own_page(key)
            .map(|page| page.messages.clone())
            .unwrap_or_default();
        messages.extend(snapshot.transcript.messages.iter().cloned());
        messages.extend(older.messages);
        self.page = Some(OlderPage {
            key: key.to_string(),
            messages,
            next_cursor: older.next_cursor,
        });
    }
}
