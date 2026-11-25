//! 데스크톱 앱의 조립 지점. TS가 `electron/main.js`와 서비스 모듈 전역 값으로 암묵적으로 만들던 객체 그래프를
//! [`DesktopRuntime`] 하나로 한 번만 만든다. hook·모바일 서버와 GPUI 앱이 같은 인스턴스를 함께 쓴다.
//!
//! 이 crate는 프로세스 환경 변수를 읽거나 바꾸지 않는다. 호출자가 시작할 때 잡아 둔 환경 스냅샷을 [`RuntimeConfig`]로 받고,
//! 자식 프로세스에는 그 스냅샷을 `create_local_shell_environment`로 좁힌 값만 넘긴다(CLAUDE.md 런타임 환경 규칙).

mod ai;
mod board;
mod clock;
mod config;
mod desktop;
pub mod development_database;
pub mod diagnostics;
mod diagram;
mod foundation;
mod hooks;
mod notifications;
mod pane_chat;
mod qa_network;
mod runtime;
mod task_cli;
mod terminal;

pub use clock::AppClock;
pub use config::{NoRemoteHookRouting, NoRemoteHostConnections, RemoteHookRouting, RuntimeConfig};
pub use diagram::{DiagramFontFiles, SharedDiagramRenderer};
pub use hooks::{DesktopHookClientRuntime, HookServerAddress, TaskCliTokens};
pub use notifications::{
    DEFAULT_NOTIFICATION_LOCALE, NotificationLocaleSource, SilentNotificationPresenter,
    system_notification_locale,
};
pub use pane_chat::NoRemotePaneGrants;
pub use qa_network::{
    NetworkResponse, NetworkResponseTable, NetworkTableLoadError, TableHttpTransport,
    TableIconHttpClient, TableReleaseFeedClient,
};
pub use runtime::{DesktopRuntime, RuntimeError};
pub use task_cli::DesktopTaskCliBackend;
pub use terminal::{BoardTerminalEvents, RegistryTaskTerminalSessions, ScpRemoteImageTransfer};
