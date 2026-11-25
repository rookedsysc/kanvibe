//! 채팅 카드에서 고른 답을 CLI의 선택 화면에 키 입력으로 전달한다.
//!
//! 키를 보내기 전에 같은 pane의 같은 CLI 프로세스에 카드가 보여 준 것과 같은 선택 화면이 떠 있는지 화면을 다시 읽어 확인하고, 아니면 보내지 않는다.
//! 그사이 터미널에서 직접 답했거나 다음 질문으로 넘어갔으면 같은 키가 엉뚱한 선택지를 고르기 때문이다.
//! 보낸 뒤에는 화면이 그 키에 반응할 때까지 잠깐 기다려, 이어지는 조회가 바뀌기 전 화면을 카드에 싣지 않게 한다.

use std::sync::Arc;

use kanvibe_ai::javascript::is_js_whitespace;
use kanvibe_ai::pane_chat::claude_prompt::is_blank_claude_text_entry;
use kanvibe_ai::pane_chat::snapshot::PaneChatPrompt;
use kanvibe_domain::pane_chat::{
    PaneChatKey, PaneChatPromptAnswer, PaneChatPromptStep, parse_pane_chat_prompt_answer,
};
use serde_json::Value;

use super::actions::{PROMPT_SUBMIT_DELAY_MS, bracketed_paste, key_bytes};
use super::conversation::read_shown_prompt;
use super::reveal::SCREEN_READ_ATTEMPTS;
use super::target::TargetLease;
use super::{
    PaneChatError, PaneChatService, changed_prompt_error, input_error, unconfirmed_input_error,
};

/// 답을 보낸 뒤 선택 화면이 그 키에 반응하기를 기다리며 화면을 다시 읽는 횟수. 반응이 없는 키(첫 질문에서의 이전 이동)도 있어 짧게 둔다
const PROMPT_REACTION_READ_ATTEMPTS: usize = 6;

fn without_whitespace(text: &str) -> String {
    text.chars()
        .filter(|character| !is_js_whitespace(*character))
        .collect()
}

/// 직접 입력 줄에 지금 보이는 글. 같은 질문에서 그 줄이 글을 받는 중일 때만 읽는다
fn typed_entry_text(
    prompt: &PaneChatPrompt,
    shown: Option<&PaneChatPrompt>,
    number: u64,
) -> Option<String> {
    let shown = shown.filter(|shown| {
        shown.screen.is_text_entry_active
            && shown.screen.highlighted_number == number
            && shown.screen.title == prompt.screen.title
    })?;
    let entry = shown
        .screen
        .options
        .iter()
        .find(|option| option.number == number)?;
    Some(without_whitespace(&format!(
        "{}{}",
        entry.label, entry.description
    )))
}

/// 선택 화면을 읽고 키를 누르는 손. 같은 pane의 같은 CLI 실행에만 닿는다
struct PromptControl<'a> {
    service: &'a PaneChatService,
    task_id: &'a str,
    token: &'a str,
    lease: &'a Arc<TargetLease>,
}

impl PromptControl<'_> {
    /// 같은 pane의 같은 CLI 프로세스가 지금 띄우고 있는 선택 화면. 없으면 `None`이다
    fn read(&self) -> Result<Option<PaneChatPrompt>, PaneChatError> {
        let lease = self.lease;
        let surface =
            self.service
                .same_execution_surface(self.task_id, &lease.pane_id, &lease.identity)?;
        Ok(read_shown_prompt(&surface, &lease.process))
    }

    fn press(&self, bytes: &str) -> Result<(), PaneChatError> {
        self.service
            .write_to_verified_target(self.task_id, self.token, self.lease, bytes)
    }

    fn wait_for(
        &self,
        is_expected: impl Fn(&Option<PaneChatPrompt>) -> bool,
        attempts: usize,
    ) -> Result<bool, PaneChatError> {
        self.service
            .wait_for_screen(|| self.read(), is_expected, attempts)
    }

    /// 글을 입력받는 중이면 숫자·화살표 키가 글로 들어간다. 위 화살표로 그 줄을 벗어난 것을 확인하고, 벗어난 뒤의 화면을 돌려준다
    fn leave_text_entry(&self, prompt: &PaneChatPrompt) -> Result<PaneChatPrompt, PaneChatError> {
        if !prompt.screen.is_text_entry_active {
            return Ok(prompt.clone());
        }
        self.press(key_bytes(PaneChatKey::Up))?;
        let has_left = self.wait_for(
            |shown| {
                shown.as_ref().is_some_and(|shown| {
                    shown.id == prompt.id && !shown.screen.is_text_entry_active
                })
            },
            SCREEN_READ_ATTEMPTS,
        )?;
        if !has_left {
            return Err(changed_prompt_error());
        }
        let mut left = prompt.clone();
        left.screen.is_text_entry_active = false;
        Ok(left)
    }

    /// 직접 입력 줄로 강조를 옮겨 글을 받는 상태로 만든다.
    /// 단일 선택은 그 줄의 숫자 키가 강조를 옮기지만, 다중 선택은 숫자 키가 체크만 뒤집으므로 화살표 키로 한 줄씩 옮긴다
    fn focus_text_entry(
        &self,
        prompt: &PaneChatPrompt,
        number: u64,
    ) -> Result<PaneChatPrompt, PaneChatError> {
        let is_typing_here = |shown: &PaneChatPrompt| {
            shown.screen.is_text_entry_active && shown.screen.highlighted_number == number
        };
        let mut shown = prompt.clone();
        for _ in 0..prompt.screen.options.len() {
            if is_typing_here(&shown) {
                break;
            }
            shown = self.move_highlight_toward(prompt, &shown, number)?;
        }
        if !is_typing_here(&shown) {
            return Err(changed_prompt_error());
        }
        Ok(shown)
    }

    fn move_highlight_toward(
        &self,
        prompt: &PaneChatPrompt,
        shown: &PaneChatPrompt,
        number: u64,
    ) -> Result<PaneChatPrompt, PaneChatError> {
        if shown.screen.is_text_entry_active {
            return Err(changed_prompt_error());
        }
        let highlighted = shown.screen.highlighted_number;
        let key = match (prompt.screen.is_multi_select, highlighted < number) {
            (true, true) => key_bytes(PaneChatKey::Down).to_string(),
            (true, false) => key_bytes(PaneChatKey::Up).to_string(),
            (false, _) => number.to_string(),
        };
        self.press(&key)?;
        let has_moved = self.wait_for(
            |moved| {
                moved.as_ref().is_some_and(|moved| {
                    moved.id == prompt.id
                        && (moved.screen.highlighted_number != highlighted
                            || moved.screen.is_text_entry_active)
                })
            },
            SCREEN_READ_ATTEMPTS,
        )?;
        if !has_moved {
            return Err(changed_prompt_error());
        }
        match self.read()? {
            Some(moved) if moved.id == prompt.id => Ok(moved),
            _ => Err(changed_prompt_error()),
        }
    }

    /// 직접 입력 줄에 글을 넣어 답한다. 넣은 글이 그 줄에 그대로 보이는지 확인한 뒤에만 다음 키를 보낸다.
    /// 단일 선택은 Enter가 그 글로 답하고, 다중 선택은 글을 넣으면 그 줄이 체크되므로 Enter 없이 줄을 벗어나 다른 선택지를 이어 고를 수 있게 둔다
    fn type_prompt_text(
        &self,
        prompt: &PaneChatPrompt,
        number: u64,
        text: &str,
    ) -> Result<(), PaneChatError> {
        let is_typing_here =
            prompt.screen.is_text_entry_active && prompt.screen.highlighted_number == number;
        let typing = match is_typing_here {
            true => prompt.clone(),
            false => self.focus_text_entry(prompt, number)?,
        };
        let typed_text = |shown: Option<&PaneChatPrompt>| typed_entry_text(prompt, shown, number);
        let wanted_text = Some(without_whitespace(text));
        if typed_text(Some(&typing)) != wanted_text {
            let entry = typing
                .screen
                .options
                .iter()
                .find(|option| option.number == number);
            if !entry.is_some_and(is_blank_claude_text_entry) {
                return Err(input_error(
                    "직접 입력 줄에 이미 다른 글이 있습니다. 터미널에서 확인해 주세요.",
                ));
            }
            self.press(&bracketed_paste(text))?;
            if !self.wait_for(
                |shown| typed_text(shown.as_ref()) == wanted_text,
                SCREEN_READ_ATTEMPTS,
            )? {
                return Err(unconfirmed_input_error());
            }
        }
        if prompt.screen.is_multi_select {
            return self.press(key_bytes(PaneChatKey::Up));
        }
        self.service.sleep_ms(PROMPT_SUBMIT_DELAY_MS);
        if typed_text(self.read()?.as_ref()) != wanted_text {
            return Err(unconfirmed_input_error());
        }
        self.press(key_bytes(PaneChatKey::Enter))
    }

    /// 답 하나를 선택 화면이 받는 키로 바꿔 보낸다
    fn send_answer(
        &self,
        prompt: &PaneChatPrompt,
        answer: &PaneChatPromptAnswer,
    ) -> Result<(), PaneChatError> {
        let options = &prompt.screen.options;
        match answer {
            PaneChatPromptAnswer::Cancel => self.press(key_bytes(PaneChatKey::Escape)),
            PaneChatPromptAnswer::Text { number, text } => {
                let number = u64::from(*number);
                if !options
                    .iter()
                    .any(|option| option.number == number && option.is_text_entry)
                {
                    return Err(changed_prompt_error());
                }
                self.type_prompt_text(prompt, number, text)
            }
            PaneChatPromptAnswer::Step { direction } => {
                let ready = self.leave_text_entry(prompt)?;
                if ready.screen.steps_line.is_none() {
                    return Err(changed_prompt_error());
                }
                let key = match direction {
                    PaneChatPromptStep::Next => PaneChatKey::Right,
                    PaneChatPromptStep::Previous => PaneChatKey::Left,
                };
                self.press(key_bytes(key))
            }
            PaneChatPromptAnswer::Option { number } => {
                let ready = self.leave_text_entry(prompt)?;
                let option = ready
                    .screen
                    .options
                    .iter()
                    .find(|option| option.number == u64::from(*number));
                // 비어 있는 직접 입력 줄은 숫자 키로 고르면 글 없이 강조만 옮겨지거나 빈 체크가 된다
                if !option.is_some_and(|option| !is_blank_claude_text_entry(option)) {
                    return Err(changed_prompt_error());
                }
                self.press(&number.to_string())
            }
        }
    }
}

impl PaneChatService {
    pub fn answer_pane_chat_prompt(
        &self,
        task_id: &str,
        target_token: &str,
        prompt_id: &str,
        requested_answer: &Value,
    ) -> Result<(), PaneChatError> {
        let answer = parse_pane_chat_prompt_answer(requested_answer)
            .filter(|_| !prompt_id.is_empty())
            .ok_or_else(|| input_error("선택 화면에 보낼 답의 형식이 올바르지 않습니다."))?;
        self.serialize_action(task_id, target_token, |lease| {
            let control = PromptControl {
                service: self,
                task_id,
                token: target_token,
                lease: &lease,
            };
            let prompt = control
                .read()?
                .filter(|prompt| prompt.id == prompt_id)
                .ok_or_else(changed_prompt_error)?;
            control.send_answer(&prompt, &answer)?;
            control.wait_for(
                |shown| shown.as_ref().is_none_or(|shown| shown.id != prompt.id),
                PROMPT_REACTION_READ_ATTEMPTS,
            )?;
            Ok(())
        })
    }
}
