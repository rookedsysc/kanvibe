//! 모바일 pane 미러(TS `mobileBridgeService`의 표면·스트림·viewport 부분)와 pane chat 연결.
//!
//! [`PaneMirror`]는 모바일 서버가 `/api/mobile/stream` WebSocket, 표면·탭·단축키·직접 연결 HTTP 경로,
//! events 소켓 명령에서 부르는 블로킹 API다. [`PaneChatRuntime`]은 pane chat 서비스와 push 구독을
//! 실제 협력자로 조립해 pane chat 경로가 부르는 메서드를 내보낸다.
//!
//! 프로세스 환경 변수는 읽지도 바꾸지도 않는다. 미러 자식 프로세스에는 좁힌 로컬 셸 환경만 넘긴다(CLAUDE.md 런타임 환경 규칙).

mod adapters;
mod error;
mod flight;
mod herdr_stream;
mod herdr_viewport;
mod input;
mod mirror;
mod pane_chat_runtime;
mod pane_operations;
mod ports;
mod process;
mod remote;
mod snapshot;
mod streams;
mod subscription;
mod surfaces;
mod viewport;
mod viewport_drivers;

#[cfg(test)]
mod tests;

pub use adapters::KanbanMirrorTaskStore;
pub use error::{MirrorResult, PaneMirrorError, SurfaceUnavailableReason};
pub use mirror::{MirrorTimings, PaneMirror, PaneMirrorParts};
pub use pane_chat_runtime::{
    KanbanPaneChatTaskLookup, PaneChatRuntime, PaneChatRuntimeParts, RemoteImagePasteUploader,
};
pub use pane_operations::{ClosedPane, CreatedTerminalTab, PaneCloseInfo, ShortcutOutcome};
pub use ports::{
    HerdrMirrorQueries, MirrorProcessSpawner, MirrorTaskStore, MobileTerminalTabs, RelayClose,
    RelayOutput, RemotePaneGrants, RemotePaneRelay, TaskTerminalAttacher,
};
pub use process::{MirrorProcess, MirrorProcessControl, SystemMirrorProcessSpawner};
pub use remote::TaskSessionWatch;
pub use subscription::{PaneEnd, PaneOutput, PaneStreamSubscription};
pub use surfaces::{MirrorTab, SurfaceQuery, TaskSurfaces};
pub use viewport::PaneViewportClaim;
