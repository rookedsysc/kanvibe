//! TS `paneChatService.test.ts`. TS가 vi.mock으로 바꾸던 surface·execGit·writeToPane·세션 리더·카탈로그를 가짜로 넣고,
//! 첨부 저장소는 TS처럼 실제 구현을 임시 디렉터리에 둔다. 시계는 기다리지 않고 대기 시간만 기록한다.

mod codex_footer;
mod exact_control;
mod harness;
mod prompt_answers;

pub(super) use harness::*;
