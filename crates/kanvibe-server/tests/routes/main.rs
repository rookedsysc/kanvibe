//! TS `electron/*Routes.test.mjs`·`mobileCommands`·`*Guard`·`mobileTlsCertificate` 테스트를 실제 서버에 대고 옮긴 것.
//!
//! 모든 테스트는 127.0.0.1:0과 임시 폴더만 쓴다.

mod harness;

mod ai_history;
mod ai_routes;
mod auth_boundary;
mod event_commands;
mod feature_routes;
mod hook_routes;
mod mobile_commands;
mod notifications;
mod pane_chat;
mod pane_stream;
mod remote_pane;
mod tasks;
mod tls_certificate;
