//! hook·task CLI·모바일 HTTP/WebSocket/TLS 서버(TS `electron/hookServer.js`와 라우트 파일들)와 서비스 조립.
//!
//! 라우트는 [`bridge::MobileBridge`]와 [`routes::hooks::HookBackend`] 경계만 보고 응답 바이트를 정한다.
//! 실제 서비스(kanvibe-mobile, kanvibe-pane-mirror, kanvibe-services …)를 그 경계에 잇는 일은 `runtime`이 맡는다.

pub mod bridge;
pub mod desktop;
pub mod diagram;
pub mod http;
pub mod launch;
pub mod qa_oracle;
pub mod routes;
pub mod server;
pub mod tls;
pub mod ws;
