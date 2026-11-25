//! Codex 대화 ID footer를 읽는 실행별 상태.
//!
//! footer 배치를 실제로 확인한 Codex 버전에서만 화면 하단의 대화 ID로 대화를 잇는다. 버전 확인은 실행 하나에 한 번이고,
//! 동시에 들어온 조회는 먼저 시작한 확인을 함께 기다린다. 화면을 읽기만 하며 어떤 입력도 하지 않는다.

use std::sync::{Arc, Mutex, OnceLock};

use kanvibe_ai::javascript::{is_js_whitespace, stringify, utf16_last, utf16_length};
use kanvibe_ai::pane_chat::codex_display::{
    CodexDisplay, is_codex_thread_footer_supported, read_codex_display,
};
use kanvibe_ai::pane_chat::codex_prompt::read_codex_prompt_with_conversation;
use kanvibe_ai::pane_chat::process::{PaneChatProcess, PaneChatProvider};
use kanvibe_domain::task::SessionType;
use kanvibe_process::worktree::quote_for_posix_shell;
use serde_json::json;

use super::target::{MAX_TARGET_LEASES, identity_for};
use super::{PaneChatError, PaneChatService, lock};
use crate::pane_chat::surface::PaneChatSurface;

/// 선택 화면이 대화 ID를 가리는 동안, 대화 ID나 그 선택 화면을 이 시간 안에 다시 확인하지 못했으면 대화가 이어진다고 보지 않는다
const CODEX_HELD_THREAD_MAX_GAP_MS: f64 = 5_000.0;
/// 같은 대화인지 견줄 때 쓰는, 선택 화면 바로 위 대화 내용의 끝 글자 수. pane 너비가 바뀌어 줄이 다시 접혀도 대화의 끝부분은 화면에 남는다
const CODEX_HELD_CONVERSATION_TAIL_LENGTH: usize = 200;

/// footer로 대화를 잇는 Codex 실행 하나의 관찰 상태
#[derive(Debug, Clone, PartialEq)]
pub struct CodexDisplayExecution {
    pub observed_thread: Option<String>,
    pub revision: u64,
    /// 대화를 잇지 못한 이유. 대화 기록 대신 보여 줄 상태다
    pub reason: String,
    pub observation: u64,
    /// 대화 ID를, 또는 그 ID를 가린 선택 화면을 마지막으로 확인한 시각
    pub confirmed_at: f64,
    /// 대화 ID를 가린 채 지켜보고 있는 화면. 대화 ID가 보이는 동안에는 `None`이다
    held_behind_prompt: Option<CodexHeldScreen>,
}

/// 대화 ID를 가린 화면이 같은 화면인지 견줄 값. 줄바꿈과 공백을 뺀 글자만 담아, pane 너비가 바뀌어 줄이 다시 접혀도 같다
#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexHeldScreen {
    prompt: String,
    /// 선택 화면 바로 위 대화 내용의 끝부분
    conversation_tail: String,
}

pub(super) type SharedCodexDisplay = Arc<Mutex<CodexDisplayExecution>>;

/// 버전 확인이 끝났거나 진행 중인 실행 하나. 확인하는 동안 온 조회는 같은 결과를 기다린다
type CodexDisplaySlot = Arc<OnceLock<Option<SharedCodexDisplay>>>;

/// 실행 식별값마다의 상태. 넣은 순서를 지켜 가득 차면 가장 오래된 것부터 버린다
#[derive(Default)]
pub(super) struct CodexDisplaySlots {
    entries: Vec<(String, CodexDisplaySlot)>,
}

impl CodexDisplaySlots {
    fn find(&self, identity: &str) -> Option<CodexDisplaySlot> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate == identity)
            .map(|(_, slot)| slot.clone())
    }
}

pub(super) fn codex_display(surface: &PaneChatSurface) -> Option<CodexDisplay> {
    let lines = surface.viewport_lines.as_ref()?;
    read_codex_display(lines, surface.cursor_row?, surface.is_input_placeholder)
}

fn without_whitespace(text: &str) -> String {
    text.chars()
        .filter(|character| !is_js_whitespace(*character))
        .collect()
}

/// 터미널에서 화살표로 강조 줄만 옮긴 것은 같은 화면이므로 강조된 선택지는 담지 않는다
fn describe_codex_held_screen(viewport_lines: &[String]) -> Option<CodexHeldScreen> {
    let shown = read_codex_prompt_with_conversation(viewport_lines)?;
    let screen = &shown.screen;
    let options: Vec<_> = screen
        .options
        .iter()
        .map(|option| json!([option.label, option.description]))
        .collect();
    let conversation = without_whitespace(&shown.conversation_rows.join(""));
    Some(CodexHeldScreen {
        prompt: without_whitespace(&stringify(&json!([screen.title, screen.detail, options]))),
        conversation_tail: utf16_last(&conversation, CODEX_HELD_CONVERSATION_TAIL_LENGTH)
            .to_string(),
    })
}

/// pane 너비가 바뀌면 화면 위로 밀려나는 줄 수가 달라져 보이는 양이 다르다. 덜 보이는 쪽이 더 보이는 쪽의 끝과 같으면 같은 대화의 끝부분이다
fn is_same_conversation_tail(held_tail: &str, shown_tail: &str) -> bool {
    let (shorter, longer) = if utf16_length(held_tail) <= utf16_length(shown_tail) {
        (held_tail, shown_tail)
    } else {
        (shown_tail, held_tail)
    };
    match shorter.is_empty() {
        true => longer.is_empty(),
        false => longer.ends_with(shorter),
    }
}

impl CodexDisplayExecution {
    fn unconfirmed() -> Self {
        Self {
            observed_thread: None,
            revision: 0,
            reason: "codex-thread-unconfirmed".to_string(),
            observation: 0,
            confirmed_at: 0.0,
            held_behind_prompt: None,
        }
    }

    /// Codex의 선택 화면은 대화 ID가 있던 하단을 가린다. 같은 선택 화면이 떠 있는 동안에는 대화를 바꿀 수 없으므로 직전에 확인한 대화가 이어진다.
    ///
    /// 대화 ID를 본 직후부터 그 선택 화면을 끊김 없이 지켜본 경우에만 그렇게 본다. 채팅을 꺼 둔 동안처럼 지켜보지 않은 사이나,
    /// 대화 ID를 다시 보지 못한 채 다른 선택 화면으로 넘어가는 사이에는 터미널에서 대화를 바꿨을 수 있다.
    /// 새 대화에서 글자가 같은 선택 화면이 다시 뜰 수도 있으므로, 같은 화면인지는 선택 화면 바로 위의 대화 내용까지 견준다
    pub(super) fn hold_thread_behind_prompt(
        &mut self,
        surface: &PaneChatSurface,
        now: f64,
    ) -> Option<String> {
        let shown = describe_codex_held_screen(surface.viewport_lines.as_ref()?)?;
        let observed = self.observed_thread.clone()?;
        if now - self.confirmed_at > CODEX_HELD_THREAD_MAX_GAP_MS {
            return None;
        }
        if let Some(held) = &self.held_behind_prompt {
            let is_same_screen = held.prompt == shown.prompt
                && is_same_conversation_tail(&held.conversation_tail, &shown.conversation_tail);
            if !is_same_screen {
                return None;
            }
        }
        // 더 길게 보인 쪽을 기준으로 남겨, 짧아진 끝부분만으로 다른 대화를 같은 대화로 보는 일이 없게 한다
        let keeps_held = self.held_behind_prompt.as_ref().is_some_and(|held| {
            utf16_length(&held.conversation_tail) > utf16_length(&shown.conversation_tail)
        });
        if !keeps_held {
            self.held_behind_prompt = Some(shown);
        }
        self.confirmed_at = now;
        Some(observed)
    }

    /// 화면에서 확인한 대화(또는 확인하지 못함)를 기록한다. 더 최근 관찰이 이미 다른 대화를 본 뒤 늦게 끝난 관찰이면 대상이 바뀐 것이다
    pub(super) fn observe_thread(
        &mut self,
        id: Option<&str>,
        observation: Option<u64>,
        now: f64,
    ) -> Result<(), PaneChatError> {
        if let Some(observation) = observation
            && observation < self.observation
            && id != self.observed_thread.as_deref()
        {
            return Err(PaneChatError::TargetChanged);
        }
        self.observation = self
            .observation
            .max(observation.unwrap_or(self.observation));
        self.held_behind_prompt = None;
        if id.is_some() {
            self.confirmed_at = now;
        }
        if self.observed_thread.as_deref() == id {
            return Ok(());
        }
        self.observed_thread = id.map(str::to_string);
        self.revision += 1;
        Ok(())
    }
}

impl PaneChatService {
    /// footer로 대화를 잇는 Codex 실행이면 그 상태를, 아니면 `None`을 돌려준다. Zellij는 화면 좌표를 주지 않아 쓰지 않는다
    pub(super) fn codex_display_execution(
        &self,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
    ) -> Option<SharedCodexDisplay> {
        if process.provider != PaneChatProvider::Codex
            || surface.task.session_type == Some(SessionType::Zellij)
        {
            return None;
        }
        let identity = identity_for(surface, process);
        let slot = {
            let mut slots = lock(&self.codex_displays);
            slots.find(&identity).unwrap_or_else(|| {
                if slots.entries.len() >= MAX_TARGET_LEASES {
                    slots.entries.remove(0);
                }
                let slot = CodexDisplaySlot::default();
                slots.entries.push((identity, slot.clone()));
                slot
            })
        };
        slot.get_or_init(|| self.probe_codex_display(surface, process))
            .clone()
    }

    /// 이미 확인을 시작한 실행의 상태. 확인 중이면 끝날 때까지 기다리고, 한 번도 확인하지 않았으면 `None`이다
    pub(super) fn existing_codex_display(&self, identity: &str) -> Option<SharedCodexDisplay> {
        let slot = lock(&self.codex_displays).find(identity)?;
        slot.wait().clone()
    }

    /// 실행 파일이 하나로 확인되고 그 버전이 footer 배치를 확인한 버전일 때만 표시 대상으로 본다
    fn probe_codex_display(
        &self,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
    ) -> Option<SharedCodexDisplay> {
        let ssh_host = surface.task.ssh_host();
        let output = self
            .exec_probe(&format!("lsof -a -p {} -d txt -Fn", process.pid), ssh_host)
            .ok()?;
        let mut binaries: Vec<&str> = Vec::new();
        for line in output.split('\n') {
            let is_codex_binary = line.starts_with("n/") && line.ends_with("/codex");
            if is_codex_binary && !binaries.contains(&&line[1..]) {
                binaries.push(&line[1..]);
            }
        }
        let [binary] = binaries.as_slice() else {
            return None;
        };
        let version = self
            .exec_probe(
                &format!("{} --version", quote_for_posix_shell(binary)),
                ssh_host,
            )
            .ok()?;
        is_codex_thread_footer_supported(&version)
            .then(|| Arc::new(Mutex::new(CodexDisplayExecution::unconfirmed())))
    }
}
