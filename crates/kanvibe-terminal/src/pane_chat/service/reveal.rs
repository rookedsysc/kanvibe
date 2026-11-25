//! Codex "대화 ID 표시". CLAUDE.md의 승인된 예외(Codex `/statusline thread-id` on User Request)를 구현한다.
//!
//! 공유 데몬에서 도는 Codex는 화면 밖에서 대화를 알려 주지 않으므로 footer가 유일한 식별 신호다. 사용자가 버튼을 눌렀을 때만,
//! footer 배치를 실제로 확인한 버전에서, Codex 고유의 dim 빈 입력란이 확인될 때만 명령을 한 번 붙여넣는다.
//! 붙여넣기 전후와 Enter 직전에 같은 pane·같은 CLI 프로세스인지 다시 확인하고, 입력란에 명령이 그대로 보이지 않으면 Enter를 보내지 않는다.
//! 글자를 하나씩 치거나, 최근 대화를 추정하거나, Codex 설정을 바꾸는 대체 경로는 없다.

use std::cell::Cell;

use kanvibe_ai::pane_chat::codex_display::is_same_composer_text;
use kanvibe_ai::pane_chat::process::{PaneChatProcess, PaneChatProvider};
use kanvibe_ai::pane_chat::snapshot::PaneChatInputState;
use serde::Serialize;

use super::actions::{PROMPT_SUBMIT_DELAY_MS, bracketed_paste};
use super::codex_display::codex_display;
use super::input_state::read_input_state;
use super::target::identity_for;
use super::{PaneChatError, PaneChatService, input_error};
use crate::pane_chat::surface::PaneChatSurface;

/// 사용자가 버튼을 눌렀을 때만 Codex 하단 상태줄을 대화 ID로 바꾸는 명령. 그 TUI 세션에만 적용되고 설정 파일에는 저장되지 않는다
const CODEX_THREAD_FOOTER_COMMAND: &str = "/statusline thread-id";
/// 키를 보낸 뒤 그 결과가 화면에 나타나기까지 화면을 다시 읽는 횟수와 간격
pub(super) const SCREEN_READ_ATTEMPTS: usize = 20;
const SCREEN_READ_INTERVAL_MS: u64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaneChatCodexThreadRevealOutcome {
    Revealed,
    InputNotEmpty,
    UnsupportedVersion,
    ThreadNotFound,
}

/// 입력란에 대화 ID 표시 명령이 그대로 보이는지
fn shows_footer_command(surface: &PaneChatSurface) -> bool {
    let composer_text = codex_display(surface)
        .map(|display| display.composer_text)
        .unwrap_or_default();
    is_same_composer_text(
        &composer_text,
        CODEX_THREAD_FOOTER_COMMAND,
        surface.viewport_columns,
    )
}

fn shows_thread_id(surface: &PaneChatSurface) -> bool {
    codex_display(surface).is_some_and(|display| display.thread_id.is_some())
}

impl PaneChatService {
    /// 화면이 기대한 모습이 될 때까지 정해진 횟수만큼 다시 읽는다
    pub(super) fn wait_for_screen<T>(
        &self,
        read: impl Fn() -> Result<T, PaneChatError>,
        is_expected: impl Fn(&T) -> bool,
        attempts: usize,
    ) -> Result<bool, PaneChatError> {
        for _ in 0..attempts {
            if is_expected(&read()?) {
                return Ok(true);
            }
            self.sleep_ms(SCREEN_READ_INTERVAL_MS);
        }
        Ok(false)
    }

    /// 사용자가 "대화 ID 표시"를 눌렀을 때만 Codex에 `/statusline thread-id`를 붙여넣고 실행해, footer에 대화 ID를 띄운다.
    /// 다른 pane 입력과 겹치지 않도록 태스크의 입력 큐에서 실행한다
    pub fn reveal_pane_chat_codex_thread(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
    ) -> Result<PaneChatCodexThreadRevealOutcome, PaneChatError> {
        if task_id.is_empty() || pane_id == Some("") {
            return Err(input_error("태스크와 pane 식별자가 올바르지 않습니다."));
        }
        let outcome = Cell::new(PaneChatCodexThreadRevealOutcome::ThreadNotFound);
        self.queue_mutation(task_id, || {
            outcome.set(self.reveal_codex_thread(task_id, pane_id)?);
            Ok(())
        })?;
        Ok(outcome.get())
    }

    /// 지금 pane에서 도는 Codex. Codex가 아니면 대상이 바뀐 것이다
    fn read_codex_target(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
    ) -> Result<(PaneChatSurface, PaneChatProcess), PaneChatError> {
        let surface = self
            .surfaces
            .read_pane_chat_surface(task_id, pane_id, true)?;
        let process = match &surface {
            Some(surface) => self.inspect_process(surface)?,
            None => None,
        };
        match (surface, process) {
            (Some(surface), Some(process)) if process.provider == PaneChatProvider::Codex => {
                Ok((surface, process))
            }
            _ => Err(PaneChatError::TargetChanged),
        }
    }

    fn reveal_codex_thread(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
    ) -> Result<PaneChatCodexThreadRevealOutcome, PaneChatError> {
        let (surface, process) = self.read_codex_target(task_id, pane_id)?;
        if self.codex_display_execution(&surface, &process).is_none() {
            return Ok(PaneChatCodexThreadRevealOutcome::UnsupportedVersion);
        }
        if shows_thread_id(&surface) {
            return Ok(PaneChatCodexThreadRevealOutcome::Revealed);
        }
        let identity = identity_for(&surface, &process);
        let read_surface = || self.same_execution_surface(task_id, &surface.pane_id, &identity);
        if read_input_state(&read_surface()?, &process, true) != PaneChatInputState::Ready {
            return Ok(PaneChatCodexThreadRevealOutcome::InputNotEmpty);
        }
        self.write_to_pane(
            task_id,
            &surface.pane_id,
            &bracketed_paste(CODEX_THREAD_FOOTER_COMMAND),
        )?;
        if !self.wait_for_screen(read_surface, shows_footer_command, SCREEN_READ_ATTEMPTS)? {
            return Err(input_error(
                "Codex 입력란에서 붙여넣은 명령을 확인하지 못해 실행하지 않았습니다. 터미널에서 입력란을 확인해 주세요.",
            ));
        }
        self.sleep_ms(PROMPT_SUBMIT_DELAY_MS);
        if !shows_footer_command(&read_surface()?) {
            return Err(input_error(
                "Codex 입력란이 바뀌어 대화 ID 표시 명령을 실행하지 않았습니다. 터미널에서 입력란을 확인해 주세요.",
            ));
        }
        self.write_to_pane(task_id, &surface.pane_id, "\r")?;
        let is_revealed =
            self.wait_for_screen(read_surface, shows_thread_id, SCREEN_READ_ATTEMPTS)?;
        Ok(match is_revealed {
            true => PaneChatCodexThreadRevealOutcome::Revealed,
            false => PaneChatCodexThreadRevealOutcome::ThreadNotFound,
        })
    }
}
