//! GPUI 앱이 같은 프로세스에서 직접 부르는 서비스 계층(계획 AD-8). 범용 invoke 버스 없이,
//! 렌더러가 실제로 부르던 IPC 메서드만 타입이 있는 메서드로 드러낸다.
//!
//! DB·명령 실행기·홈 디렉터리·HTTP 클라이언트·터미널 계층은 모두 생성 시점에 주입한다.
//! 프로세스 환경 변수를 읽거나 바꾸지 않는다(CLAUDE.md 런타임 환경 규칙).

pub mod attachment_content;
pub mod background_task_sync;
pub mod board_notifier;
pub mod desktop_notification;
pub mod diff;
pub mod error;
pub mod git_ref;
pub mod hook_status;
pub mod kanban;
pub mod notification_store;
pub mod project;
pub mod release_update;
pub mod remote_image_paste;
pub mod request_scope;
pub mod session_dependency;
pub mod terminal_ports;

pub use error::{ServiceError, ServiceResult};

#[cfg(test)]
pub(crate) mod test_support;
