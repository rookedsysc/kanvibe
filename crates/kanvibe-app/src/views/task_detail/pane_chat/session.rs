//! 한 pane 채팅의 상태(TS `usePaneChat`). 마지막으로 읽은 pane 상태, 동작 중 표시, 보낸 뒤 기록되기를 기다리는 말풍선,
//! 다중 선택 답의 차례를 갖는다. 화면(`view.rs`)이 서비스 호출 결과를 여기에 넣고 그린다.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kanvibe_ai::pane_chat::snapshot::{
    PaneChatAgentActivity, PaneChatPrompt, PaneChatSnapshot, PaneChatTranscriptStatus,
    get_pane_chat_prompt_dialog_key,
};
use kanvibe_ai::types::{AggregatedAiMessage, AiMessageRole};

const POLL_INTERVAL: Duration = Duration::from_millis(1_500);
const UNAVAILABLE_BACKOFF_THRESHOLD: u32 = 3;
const UNAVAILABLE_POLL_INTERVAL: Duration = Duration::from_millis(5_000);
/// 입력창이 비지 않았거나 CLI가 작업 중이라 Codex 대화를 아직 잇지 못한 상태. 이때는 기다리는 횟수를 세지 않는다
pub const CODEX_INPUT_BUSY_REASON: &str = "codex-thread-input-busy";

/// TS `crypto.randomUUID()` 자리. 이 프로세스 안에서 겹치지 않는 요청 식별값이면 된다
pub fn new_request_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("pane-chat-{nanos:x}-{sequence}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingState {
    Sending,
    Sent,
    Failed,
}

/// 보냈지만 아직 대화 기록에 나타나지 않은 메시지
#[derive(Debug, Clone, PartialEq)]
pub struct PendingMessage {
    pub id: String,
    conversation: String,
    pub message: AggregatedAiMessage,
    baseline: Vec<String>,
    pub state: PendingState,
}

/// 답을 보내는 중에 눌러 차례를 기다리는 다중 선택의 선택지
#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedPromptOption {
    dialog_key: String,
    number: u64,
    label: String,
}

/// 선택 화면에 답을 보내지 못한 이유. 그 선택 화면의 카드에만 보인다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptError {
    pub dialog_key: String,
    pub message: String,
}

fn message_key(message: &AggregatedAiMessage) -> String {
    let text = match message.full_text.is_empty() {
        true => &message.text,
        false => &message.full_text,
    };
    let attachment_ids: Option<Vec<&str>> = message
        .attachments
        .as_ref()
        .map(|items| items.iter().map(|item| item.id.as_str()).collect());
    format!("{:?}|{text:?}|{attachment_ids:?}", message.timestamp)
}

fn without_whitespace(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

/// 기록된 사용자 메시지가 보낸 글의 기록인지. CLI는 끝 공백을 떼고 탭을 공백으로 바꿔 기록하므로 공백은 빼고 견준다.
/// 첨부 파일은 글 뒤에 `@경로`로 붙어 기록되므로, 보낸 글 바로 뒤에 파일 언급이 이어져도 같은 메시지다
pub fn is_record_of_sent_text(recorded: &AggregatedAiMessage, sent_text: &str) -> bool {
    let recorded_text = match recorded.full_text.is_empty() {
        true => recorded.text.as_str(),
        false => recorded.full_text.as_str(),
    };
    if sent_text.trim().is_empty() {
        let has_attachments = recorded
            .attachments
            .as_ref()
            .is_some_and(|items| !items.is_empty());
        return has_attachments || recorded_text.starts_with('@');
    }
    let recorded_characters = without_whitespace(recorded_text);
    let sent_characters = without_whitespace(sent_text);
    recorded_characters == sent_characters
        || recorded_text.starts_with(&format!("{sent_text}\n\n"))
        || recorded_characters.starts_with(&format!("{sent_characters}@"))
}

fn conversation_key(snapshot: &PaneChatSnapshot) -> String {
    let conversation = snapshot
        .conversation_key
        .as_ref()
        .or(snapshot.target_token.as_ref());
    format!(
        "{:?}|{:?}|{:?}",
        snapshot.task_id, snapshot.pane_id, conversation
    )
}

fn is_slash_command(text: &str) -> bool {
    text.trim_start().starts_with('/')
}

pub struct ChatSession {
    pub task_id: String,
    pub pane_id: Option<String>,
    /// 화면에 보이는 마지막 상태
    displayed: Option<PaneChatSnapshot>,
    /// 보이는 스냅샷이 바뀔 때마다 오른다. 보일 대화를 다시 만들지 가르는 데 쓴다
    snapshot_revision: u64,
    /// 동작을 보낼 수 있는 확인된 상태인지(TS `current.current`가 있는지)
    has_current: bool,
    is_ready: bool,
    pub has_read_error: bool,
    pub action_error: Option<String>,
    pub prompt_error: Option<PromptError>,
    pub is_busy: bool,
    pending: Vec<PendingMessage>,
    confirmed_operations: HashSet<String>,
    /// 보이기·대상이 바뀔 때마다 올린다. 그 전의 응답은 버린다
    pub generation: u64,
    unavailable_reads: u32,
    queued_prompt_options: Vec<QueuedPromptOption>,
    sending_prompt_option: Option<u64>,
}

impl ChatSession {
    pub fn new(task_id: String, pane_id: Option<String>) -> Self {
        Self {
            task_id,
            pane_id,
            displayed: None,
            snapshot_revision: 0,
            has_current: false,
            is_ready: false,
            has_read_error: false,
            action_error: None,
            prompt_error: None,
            is_busy: false,
            pending: Vec::new(),
            confirmed_operations: HashSet::new(),
            generation: 0,
            unavailable_reads: 0,
            queued_prompt_options: Vec::new(),
            sending_prompt_option: None,
        }
    }

    pub fn snapshot_revision(&self) -> u64 {
        self.snapshot_revision
    }

    pub fn snapshot(&self) -> Option<&PaneChatSnapshot> {
        self.displayed.as_ref()
    }

    /// 지금 보이는 상태가 이 pane의 확인된 상태인지(TS `isReady`)
    pub fn is_ready(&self) -> bool {
        self.is_ready
    }

    /// 동작을 보낼 확인된 대상
    fn current(&self) -> Option<&PaneChatSnapshot> {
        self.displayed.as_ref().filter(|_| self.has_current)
    }

    /// 숨겨지면 동작 대상을 잊는다. 보이던 대화는 그대로 두되 다시 보일 때 새로 확인할 때까지 입력을 막는다
    pub fn hide(&mut self) {
        self.generation += 1;
        self.has_current = false;
        self.is_ready = false;
    }

    /// 다시 보이기 시작할 때. 읽기 실패 표시를 걷고 새 세대로 읽는다
    pub fn show(&mut self) {
        self.generation += 1;
        self.has_read_error = false;
        self.unavailable_reads = 0;
    }

    pub fn apply_snapshot(&mut self, snapshot: PaneChatSnapshot) {
        let is_waiting_for_input =
            snapshot.transcript.reason.as_deref() == Some(CODEX_INPUT_BUSY_REASON);
        let is_unavailable = snapshot.transcript.status == PaneChatTranscriptStatus::Unavailable;
        self.unavailable_reads = match is_unavailable && !is_waiting_for_input {
            true => self.unavailable_reads + 1,
            false => 0,
        };
        self.is_ready = self.pane_id.is_none() || snapshot.pane_id == self.pane_id;
        let shown_dialog = snapshot
            .prompt
            .as_ref()
            .map(get_pane_chat_prompt_dialog_key);
        if self.prompt_error.as_ref().map(|error| &error.dialog_key) != shown_dialog.as_ref() {
            self.prompt_error = None;
        }
        self.displayed = Some(snapshot);
        self.snapshot_revision += 1;
        self.has_current = true;
        self.has_read_error = false;
        self.acknowledge_recorded_messages();
    }

    pub fn apply_read_failure(&mut self) {
        self.unavailable_reads += 1;
        self.has_current = false;
        self.is_ready = false;
        self.has_read_error = true;
    }

    /// 다음 조회까지 기다릴 시간. 연결하지 못한 상태가 이어지면 늦춘다
    pub fn poll_delay(&self) -> Duration {
        match self.unavailable_reads >= UNAVAILABLE_BACKOFF_THRESHOLD {
            true => UNAVAILABLE_POLL_INTERVAL,
            false => POLL_INTERVAL,
        }
    }

    /// 동작을 보낼 수 있으면 대상 토큰을 돌려주고 바쁨 표시를 켠다(TS `run`의 앞부분).
    /// 지난 오류는 부른 쪽이 자기 자리(입력창 또는 선택 카드)에서 지운다
    pub fn begin_action(&mut self) -> Option<String> {
        if self.is_busy {
            return None;
        }
        let target = self.current()?;
        let is_same_pane = self.pane_id.is_none() || target.pane_id == self.pane_id;
        let token = target
            .target_token
            .clone()
            .filter(|_| target.can_send && target.task_id == self.task_id && is_same_pane)?;
        self.is_busy = true;
        Some(token)
    }

    /// 아직 대상 토큰이 없는 Codex의 "대화 ID 표시". 바쁘지 않으면 바쁨 표시를 켠다
    pub fn begin_reveal(&mut self) -> bool {
        if self.is_busy {
            return false;
        }
        self.is_busy = true;
        self.action_error = None;
        true
    }

    pub fn finish_action(&mut self) {
        self.is_busy = false;
    }

    /// 동작이 시작된 뒤 대상이 그대로인지. 바뀌었으면 그 결과의 오류는 보여 주지 않는다
    pub fn is_same_target(&self, generation: u64, token: &str) -> bool {
        self.generation == generation
            && self
                .current()
                .is_some_and(|current| current.target_token.as_deref() == Some(token))
    }
}

/// 보낸 메시지의 말풍선
impl ChatSession {
    /// 보낼 글을 말풍선으로 먼저 보여 준다. 슬래시 명령은 대화가 아니라 CLI 조작이라 말풍선을 만들지 않는다
    pub fn add_pending(&mut self, id: &str, text: &str, now_iso: String) {
        let Some(target) = self.current() else {
            return;
        };
        if is_slash_command(text) || self.confirmed_operations.contains(id) {
            return;
        }
        let row = PendingMessage {
            id: id.to_string(),
            conversation: conversation_key(target),
            baseline: target.transcript.messages.iter().map(message_key).collect(),
            message: AggregatedAiMessage::plain(AiMessageRole::User, Some(&now_iso), text),
            state: PendingState::Sending,
        };
        self.pending.retain(|pending| pending.id != id);
        self.pending.push(row);
    }

    /// CLI에 들어가지 않은 것이 확실한 글은 입력창에 그대로 남으므로 말풍선을 지운다. 전달됐을 수도 있는 글만 남긴다
    pub fn settle_pending(&mut self, id: &str, is_accepted: bool, may_have_been_delivered: bool) {
        if !is_accepted && !may_have_been_delivered {
            self.pending.retain(|pending| pending.id != id);
            return;
        }
        let state = match is_accepted {
            true => PendingState::Sent,
            false => PendingState::Failed,
        };
        if let Some(row) = self.pending.iter_mut().find(|pending| pending.id == id) {
            row.state = state;
        }
        self.acknowledge_recorded_messages();
    }

    /// 지금 대화에 속하고 아직 기록되지 않은 말풍선
    pub fn pending_messages(&self) -> Vec<&PendingMessage> {
        let Some(snapshot) = self.displayed.as_ref() else {
            return Vec::new();
        };
        let conversation = conversation_key(snapshot);
        self.pending
            .iter()
            .filter(|pending| pending.conversation == conversation)
            .collect()
    }

    /// 대화 기록에 나타난 말풍선을 걷고, 같은 요청을 다시 보내도 말풍선을 또 만들지 않게 기억한다
    fn acknowledge_recorded_messages(&mut self) {
        let Some(snapshot) = self.displayed.as_ref() else {
            return;
        };
        let conversation = conversation_key(snapshot);
        let mut used_rows: HashSet<String> = HashSet::new();
        let mut acknowledged: Vec<String> = Vec::new();
        for row in self
            .pending
            .iter()
            .filter(|row| row.conversation == conversation)
        {
            let recorded = snapshot.transcript.messages.iter().find(|message| {
                let key = message_key(message);
                message.role == AiMessageRole::User
                    && !used_rows.contains(&key)
                    && !row.baseline.contains(&key)
                    && is_record_of_sent_text(message, &row.message.text)
            });
            if let Some(recorded) = recorded {
                used_rows.insert(message_key(recorded));
                acknowledged.push(row.id.clone());
            }
        }
        self.pending.retain(|row| !acknowledged.contains(&row.id));
        self.confirmed_operations.extend(acknowledged);
    }

    /// CLI 화면으로 작업 중인지 모르는 경우에도, 보낸 메시지가 기록되기 전까지는 에이전트가 처리하는 중으로 본다
    pub fn is_agent_working(&self) -> bool {
        let is_working = self
            .displayed
            .as_ref()
            .is_some_and(|snapshot| snapshot.agent_activity == PaneChatAgentActivity::Working);
        let has_unrecorded_send = self
            .pending_messages()
            .iter()
            .any(|row| row.state != PendingState::Failed);
        is_working || has_unrecorded_send
    }
}

/// 다중 선택 답의 차례
impl ChatSession {
    /// 답을 보내는 중에 누른 다중 선택의 다른 선택지를 기억한다. 보내는 중이거나 이미 기다리는 선택지는 한 번으로 친다
    pub fn queue_prompt_option(&mut self, prompt: &PaneChatPrompt, number: u64) {
        if !prompt.screen.is_multi_select {
            return;
        }
        let is_already_waiting = self.sending_prompt_option == Some(number)
            || self
                .queued_prompt_options
                .iter()
                .any(|queued| queued.number == number);
        let option = prompt
            .screen
            .options
            .iter()
            .find(|option| option.number == number);
        if let (Some(option), false) = (option, is_already_waiting) {
            self.queued_prompt_options.push(QueuedPromptOption {
                dialog_key: get_pane_chat_prompt_dialog_key(prompt),
                number,
                label: option.label.clone(),
            });
        }
    }

    pub fn set_sending_prompt_option(&mut self, number: Option<u64>) {
        self.sending_prompt_option = number;
    }

    /// 답이 받아들여진 뒤 다음으로 보낼 선택지. 같은 선택 화면에 그 선택지가 그대로 있을 때만 이어 보내고, 아니면 모두 버린다
    pub fn next_queued_prompt_option(
        &mut self,
        is_accepted: bool,
    ) -> Option<(PaneChatPrompt, u64)> {
        let next = match self.queued_prompt_options.is_empty() {
            true => None,
            false => Some(self.queued_prompt_options.remove(0)),
        };
        let shown = self.current().and_then(|current| current.prompt.clone());
        let follow_up = next.zip(shown).filter(|(next, shown)| {
            get_pane_chat_prompt_dialog_key(shown) == next.dialog_key
                && shown
                    .screen
                    .options
                    .iter()
                    .any(|option| option.number == next.number && option.label == next.label)
        });
        match follow_up.filter(|_| is_accepted) {
            Some((next, shown)) => Some((shown, next.number)),
            None => {
                self.queued_prompt_options.clear();
                None
            }
        }
    }
}
