//! 공용 네이티브 터미널 뷰(TS `Terminal`). 작업 상세·보드 워크스페이스·로그인 터미널이 함께 쓴다. 레지스트리 화면(VT 해석은 한 번)을 그리고 입력을 PTY로 보낸다.

mod image_paste;
pub mod input;
mod lifecycle;
pub mod mouse;
pub mod paint;
pub mod pane;
mod scroll_bar;
pub mod status;
pub mod transport;
pub mod window;

pub use pane::{TerminalPane, TerminalPaneEvent, TerminalTarget};
pub use transport::{ImagePasteOutcome, TerminalTransport};
