//! AI 세션 기록·실시간 세션·사용량·계정 계층. TS `src/lib/aiSessions`, `src/lib/aiUsage`와
//! main 서비스(`aiUsageService`, `aiAccountService`, 로그인 브리지)의 UI가 아닌 부분을 옮긴다.
//!
//! 홈 디렉터리, provider 환경 변수, 원격 명령 실행기는 모두 [`host::SessionHost`]로 주입한다.
//! 프로세스 환경을 직접 읽거나 바꾸지 않는다(CLAUDE.md 런타임 환경 규칙).

pub mod accounts;
pub mod additional_file_session_formats;
pub mod additional_file_sessions;
pub mod agent_call_graph;
pub mod aggregate;
pub mod claude_sessions;
pub mod codex_app_server;
pub mod codex_sessions;
pub mod codex_thread;
pub mod devin_session_index;
pub mod error;
pub mod gemini_sessions;
pub mod host;
pub mod javascript;
pub mod jsonl;
pub mod legacy_opencode_sessions;
pub mod live_sessions;
pub mod message_attachments;
pub mod message_files;
pub mod message_instructions;
pub mod message_text;
pub mod node_path;
pub mod opencode_sessions;
pub mod pane_chat;
pub mod parse_cache;
pub mod providers;
pub mod record_fields;
pub mod running_agent_panes;
pub mod session_attachment;
pub mod session_listing;
pub mod sqlite_agent_sessions;
pub mod sqlite_rows;
pub mod types;
pub mod usage;

#[cfg(test)]
pub(crate) mod test_support;

pub use error::{AiError, AiResult};
pub use providers::AiSessionProvider;
