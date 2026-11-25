//! 작업 상세의 네이티브 터미널. 레지스트리 화면(VT 해석은 한 번)을 그리고 입력을 PTY로 보낸다.

mod image_paste;
pub mod input;
pub mod mouse;
pub mod paint;
pub mod pane;
pub mod runtime;
pub mod status;

pub use pane::{TerminalPane, TerminalPaneEvent, TerminalTarget};
