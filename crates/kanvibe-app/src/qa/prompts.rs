//! 네이티브 확인 대화상자(TS `window.confirm`). QA 재생(`--qa-harness`)에서는 기준 이미지를 찍은 Playwright가 대화상자를
//! 자동으로 받아들인 것과 같게 묻지 않고 받아들인다. 화면은 대화상자를 직접 띄우지 않고 이 함수를 거친다.

use gpui_kit::{App, PromptButton, PromptLevel, Task, Window};

use crate::qa::QaRuntime;

/// 확인을 받으면 `true`. QA 모드에서는 바로 `true`다
pub fn confirm_action(
    message: &str,
    detail: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) -> Task<bool> {
    if cx.has_global::<QaRuntime>() {
        return Task::ready(true);
    }
    let answer = window.prompt(
        PromptLevel::Warning,
        message,
        detail,
        &[PromptButton::ok("OK"), PromptButton::cancel("Cancel")],
        cx,
    );
    cx.foreground_executor()
        .spawn(async move { answer.await == Ok(0) })
}
