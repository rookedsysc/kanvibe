//! pane chat 서비스(TS `desktop/main/services/paneChat*.ts`). 화면 파싱 판정은 `kanvibe_ai::pane_chat`에 있고,
//! 여기에는 pane을 읽고 입력을 넣는 서비스와 저장소가 있다.

pub mod attachments;
pub mod composer_catalog;
pub mod connection_signals;
pub mod mode_store;
pub mod service;
pub mod subscription;
pub mod surface;

#[cfg(test)]
mod session_bindings_integration;
