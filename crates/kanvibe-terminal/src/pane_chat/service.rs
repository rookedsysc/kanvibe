//! pane chat 서비스(TS `paneChatService.ts`). pane에서 돌고 있는 에이전트 CLI를 확인하고, 그 대화 기록과 화면을 읽고,
//! 검증한 같은 pane·같은 CLI 프로세스·같은 대화에만 입력을 넣는다.
//!
//! TS 모듈 전역 상태(대상 lease, 태스크별 입력 큐, 요청 영수증, Codex 표시 상태, 모델 목록 캐시, 공유 조회)는
//! 앱이 한 번 만드는 [`PaneChatService`]의 필드다. 화면 판정은 `kanvibe_ai::pane_chat`의 것을 쓴다.
//!
//! Codex `/statusline thread-id` 입력은 CLAUDE.md의 승인된 예외다. 사용자가 "대화 ID 표시"를 누른
//! [`PaneChatService::reveal_pane_chat_codex_thread`]에서만, 확인된 버전·빈 입력란·같은 pane과 프로세스를 거듭 확인한 뒤에만 친다.

mod actions;
mod codex_display;
mod conversation;
mod dependencies;
mod input_state;
mod prompt_answer;
mod queue;
mod reading;
mod reveal;
mod send;
mod target;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use kanvibe_ai::pane_chat::model_settings::PaneChatModelOption;
use kanvibe_ai::pane_chat::snapshot::PaneChatSnapshot;
use kanvibe_domain::pane_chat::PaneChatSuggestion;
use kanvibe_process::exec::ShellCommandRunner;
use serde::Serialize;

use super::attachments::{PaneChatAttachmentError, PaneChatAttachmentStore};
use super::surface::{
    PaneChatMobileBridge, PaneChatSurfaceError, PaneChatSurfaces, PaneChatTerminals,
};

pub use codex_display::CodexDisplayExecution;
pub use dependencies::{
    AiPaneChatSessionRecords, CodexThreadAddress, CodexThreadDelivery, PaneChatClock,
    PaneChatReaderScope, PaneChatSessionRecords, PaneChatSuggestionSource, SystemPaneChatClock,
};
pub use reveal::PaneChatCodexThreadRevealOutcome;

/// pane chat 동작의 실패. 이름은 TS 오류의 `name`과 같은 갈래다
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaneChatError {
    /// TS `PaneChatInputError`. 사용자에게 그대로 보여 주는 입력·상태 오류다
    #[error("{0}")]
    Input(String),
    /// TS `PaneChatTargetChangedError`
    #[error("CLI 대상이 변경되었습니다. 현재 pane을 다시 확인해 주세요.")]
    TargetChanged,
    /// TS `PaneChatDeliveryUncertainError`. 쓰기를 시도한 뒤 실패해 CLI가 받았는지 알 수 없다
    #[error(
        "CLI 전송 결과를 확인할 수 없습니다. 터미널에서 확인해 주세요. 같은 요청은 다시 전송하지 않습니다."
    )]
    DeliveryUncertain { cause: Box<PaneChatError> },
    /// 그 밖의 실패(명령·파일·API 오류)
    #[error("{0}")]
    Other(String),
}

impl From<PaneChatSurfaceError> for PaneChatError {
    fn from(error: PaneChatSurfaceError) -> Self {
        Self::Other(error.0)
    }
}

impl From<PaneChatAttachmentError> for PaneChatError {
    fn from(error: PaneChatAttachmentError) -> Self {
        match error {
            PaneChatAttachmentError::Input(message) => Self::Input(message),
            other => Self::Other(other.to_string()),
        }
    }
}

pub(crate) fn input_error(message: &str) -> PaneChatError {
    PaneChatError::Input(message.to_string())
}

fn unconfirmed_input_error() -> PaneChatError {
    input_error(
        "CLI 입력란에서 보낸 내용을 확인하지 못해 제출하지 않았습니다. 터미널에서 확인해 주세요.",
    )
}

fn changed_prompt_error() -> PaneChatError {
    input_error(
        "CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다. 화면을 확인한 뒤 다시 골라 주세요.",
    )
}

/// 조회 한 번의 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatReading {
    pub snapshot: PaneChatSnapshot,
    /// 이 데스크탑의 파일 시스템에서 바로 감시할 수 있는 대화 기록 파일. 원격 태스크이거나 기록을 연결하지 못했으면 `None`이다
    pub local_transcript_file: Option<String>,
}

/// TS `PaneChatReadOptions`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaneChatReadOptions {
    /// 대화 기록을 연결했어도 CLI 화면을 함께 읽는다. 기록을 연결하지 못했으면 이 값과 상관없이 화면을 읽는다
    pub should_include_screen: bool,
    /// 같은 pane을 보는 다른 조회가 이 결과를 잠깐 재사용하도록 남긴다. 주기적으로 pane을 읽는 구독 감시만 켠다
    pub is_shared_with_other_viewers: bool,
}

/// 입력창 메뉴에 보여 줄 제안. 어느 CLI 대상의 결과인지 함께 돌려준다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatSuggestions {
    pub target_token: String,
    pub items: Vec<PaneChatSuggestion>,
    #[serde(rename = "truncated")]
    pub is_truncated: bool,
}

/// 앱이 서비스를 만들 때 넘기는 의존성
pub struct PaneChatServiceDependencies {
    pub surfaces: Arc<dyn PaneChatSurfaces>,
    pub runner: Arc<dyn ShellCommandRunner>,
    pub terminals: Arc<dyn PaneChatTerminals>,
    pub mobile_bridge: Arc<dyn PaneChatMobileBridge>,
    pub sessions: Arc<dyn PaneChatSessionRecords>,
    pub suggestions: Arc<dyn PaneChatSuggestionSource>,
    pub attachments: Arc<PaneChatAttachmentStore>,
    pub clock: Arc<dyn PaneChatClock>,
}

/// pane chat 서비스. 앱이 하나 만들어 데스크톱 IPC, 모바일 HTTP, 구독 감시가 함께 쓴다
pub struct PaneChatService {
    surfaces: Arc<dyn PaneChatSurfaces>,
    runner: Arc<dyn ShellCommandRunner>,
    terminals: Arc<dyn PaneChatTerminals>,
    mobile_bridge: Arc<dyn PaneChatMobileBridge>,
    sessions: Arc<dyn PaneChatSessionRecords>,
    suggestions: Arc<dyn PaneChatSuggestionSource>,
    attachments: Arc<PaneChatAttachmentStore>,
    clock: Arc<dyn PaneChatClock>,
    leases: Mutex<target::LeaseTable>,
    mutations: queue::MutationQueue,
    receipts: Mutex<HashMap<String, HashMap<String, queue::Receipt>>>,
    codex_displays: Mutex<codex_display::CodexDisplaySlots>,
    /// 호스트(로컬은 빈 문자열)마다 Codex 모델 목록과 읽은 시각
    codex_model_catalogs: Mutex<HashMap<String, (f64, Vec<PaneChatModelOption>)>>,
    shared_readings: Mutex<Vec<(String, reading::SharedReading)>>,
}

impl PaneChatService {
    pub fn new(dependencies: PaneChatServiceDependencies) -> Self {
        Self {
            surfaces: dependencies.surfaces,
            runner: dependencies.runner,
            terminals: dependencies.terminals,
            mobile_bridge: dependencies.mobile_bridge,
            sessions: dependencies.sessions,
            suggestions: dependencies.suggestions,
            attachments: dependencies.attachments,
            clock: dependencies.clock,
            leases: Mutex::new(target::LeaseTable::default()),
            mutations: queue::MutationQueue::default(),
            receipts: Mutex::new(HashMap::new()),
            codex_displays: Mutex::new(codex_display::CodexDisplaySlots::default()),
            codex_model_catalogs: Mutex::new(HashMap::new()),
            shared_readings: Mutex::new(Vec::new()),
        }
    }

    fn now_ms(&self) -> f64 {
        self.clock.now_ms()
    }

    fn sleep_ms(&self, milliseconds: u64) {
        self.clock
            .sleep(std::time::Duration::from_millis(milliseconds));
    }
}

/// 잠금을 쥔 쪽이 패닉으로 끝나도 남은 상태를 계속 쓴다. 상태는 한 번의 대입으로만 바뀌어 반쯤 바뀐 값이 남지 않는다
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl super::subscription::PaneChatSubscriptionSource for PaneChatService {
    fn read_pane_chat(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        options: PaneChatReadOptions,
    ) -> Result<PaneChatReading, PaneChatError> {
        PaneChatService::read_pane_chat(self, task_id, pane_id, options)
    }

    fn read_recent_pane_chat(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: bool,
    ) -> Result<PaneChatReading, PaneChatError> {
        PaneChatService::read_recent_pane_chat(self, task_id, pane_id, should_include_screen)
    }

    fn get_pane_chat_agent_provider(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> Option<kanvibe_ai::providers::AiSessionProvider> {
        PaneChatService::get_pane_chat_agent_provider(self, task_id, pane_id)
    }

    fn is_pane_chat_action_in_progress(&self, task_id: &str) -> bool {
        PaneChatService::is_pane_chat_action_in_progress(self, task_id)
    }
}
