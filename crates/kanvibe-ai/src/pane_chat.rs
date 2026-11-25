//! pane 채팅. 터미널 화면에서 Claude·Codex의 입력란, 선택 화면, 대화 ID footer를 읽고, 기록에서 모델 설정을 읽고,
//! pane을 소유한 CLI 프로세스와 대화 기록을 잇는다. 키 입력·붙여넣기는 터미널 계층(M3)의 몫이고 여기에는 판정만 있다.

pub mod claude_composer;
pub mod claude_prompt;
pub mod codex_display;
pub mod codex_prompt;
pub mod model_settings;
pub mod process;
mod screen_text;
pub mod session_bindings;
pub mod session_catalog;
pub mod snapshot;
pub mod timeline;
