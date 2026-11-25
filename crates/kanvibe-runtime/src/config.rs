//! 런타임을 만드는 데 필요한 값. 모든 값은 호출자가 명시적으로 넘기고, 이 crate는 프로세스 환경을 읽지도 바꾸지도 않는다.

use std::path::PathBuf;
use std::sync::Arc;

use kanvibe_db::DatabaseLocation;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_pane_mirror::RemotePaneGrants;
use kanvibe_process::shell_environment::ShellEnvironment;
use kanvibe_services::desktop_notification::{
    DesktopNotificationPresenter, NotificationDeliveryCallbacks,
};
use kanvibe_terminal::terminal_registry::{RemoteHostConnection, RemoteHostConnections};

use crate::clock::AppClock;
use crate::diagram::DiagramFontFiles;
use crate::notifications::SilentNotificationPresenter;
use crate::pane_chat::NoRemotePaneGrants;
use crate::qa_network::NetworkResponseTable;

/// 원격 hook이 데스크톱 hook 서버로 돌아오는 경로(TS `getRemoteHostNamespace`, `ensureHookTunnel`).
/// 원격 host daemon과 터널은 kanvibe-remote-host가 구현해 넘긴다
pub trait RemoteHookRouting: Send + Sync {
    /// 호스트에 원격 host daemon이 붙어 있을 때 그 daemon의 namespace(`dev`, `prod`, `qa-*`). 없으면 `None`이다
    fn remote_host_namespace(&self, ssh_host: &str) -> Option<String>;
    /// 원격 hook이 돌아올 터널을 연다. 결과를 기다리지 않는다
    fn ensure_hook_tunnel(&self, ssh_host: &str);
}

/// 원격 daemon과 터널이 없는 실행. namespace가 없고 터널도 열지 않는다
pub struct NoRemoteHookRouting;

impl RemoteHookRouting for NoRemoteHookRouting {
    fn remote_host_namespace(&self, _ssh_host: &str) -> Option<String> {
        None
    }

    fn ensure_hook_tunnel(&self, _ssh_host: &str) {}
}

/// 원격 host daemon이 없는 실행. 원격 세션은 모두 순수 ssh 경로로 간다
pub struct NoRemoteHostConnections;

impl RemoteHostConnections for NoRemoteHostConnections {
    fn connection(&self, _ssh_host: &str) -> Option<Arc<dyn RemoteHostConnection>> {
        None
    }
}

pub struct RuntimeConfig {
    /// 앱 데이터 디렉터리(TS `app.getPath("userData")`). 자식 프로세스 환경에는 넣지 않는다
    pub app_data_directory: PathBuf,
    /// DB 파일과 첫 실행 시드. [`RuntimeConfig::new`]는 데이터 디렉터리의 `kanvibe.db`를 시드 없이 쓴다
    pub database_location: DatabaseLocation,
    /// hook 서버가 듣는 평문 포트(TS `setHookServerPort`). 로컬 hook은 `http://localhost:<port>`로,
    /// 원격 hook은 SSH 연결이 나가는 로컬 IPv4의 이 포트로 상태를 알린다(TS `getHookServerUrl`)
    pub hook_server_port: u16,
    /// 사용자 홈. `~/.ssh/config`, AI 기록, 사용자 hook 설정의 기준이고, 자식 환경의 `HOME`도 이 값으로 맞춘다
    pub home_directory: String,
    /// 호출자가 시작할 때 한 번 읽어 둔 프로세스 환경. 자식 프로세스에는 `create_local_shell_environment`로 좁힌 값만 넘긴다
    pub base_environment: ShellEnvironment,
    /// 원격 host daemon 연결. 기본값은 연결 없음이다
    pub remote_host_connections: Arc<dyn RemoteHostConnections>,
    /// 원격 hook의 namespace와 터널. 기본값은 둘 다 없음이다
    pub remote_hook_routing: Arc<dyn RemoteHookRouting>,
    /// 원격 Herdr pane 직접 연결 권한(모바일). 기본값은 권한을 내지 않음이다
    pub remote_pane_grants: Arc<dyn RemotePaneGrants>,
    /// AppSettings 단축키 표가 따르는 플랫폼. QA가 macOS 렌더러 상태를 재현할 때만 바꾼다
    pub shortcut_platform: ShortcutPlatform,
    /// 서비스가 보는 현재 시각. QA 모드는 고정 시각에서 출발한다
    pub clock: AppClock,
    /// QA 모드의 바깥 HTTP 응답 표. 있으면 AI 사용량·릴리스 확인·아이콘 요청이 실제 네트워크로 나가지 않는다
    pub qa_network_responses: Option<Arc<NetworkResponseTable>>,
    /// OS 알림 표시기. 기본값은 띄우지 않음이다
    pub notification_presenter: Arc<dyn DesktopNotificationPresenter>,
    /// 알림 목록이 바뀌었을 때와 OS 알림을 눌렀을 때 화면이 할 일
    pub notification_callbacks: NotificationDeliveryCallbacks,
    /// 다이어그램 렌더러가 첫 요청 때 읽을 글꼴. 기본값은 빈 목록이라 렌더가 실패한다
    pub diagram_font_files: DiagramFontFiles,
    /// 릴리스 확인이 비교할 지금 앱 버전
    pub app_version: String,
}

impl RuntimeConfig {
    pub fn new(
        app_data_directory: impl Into<PathBuf>,
        hook_server_port: u16,
        home_directory: impl Into<String>,
        base_environment: ShellEnvironment,
    ) -> Self {
        let app_data_directory = app_data_directory.into();
        Self {
            database_location: DatabaseLocation::in_app_data_directory(&app_data_directory, None),
            app_data_directory,
            hook_server_port,
            home_directory: home_directory.into(),
            base_environment,
            remote_host_connections: Arc::new(NoRemoteHostConnections),
            remote_hook_routing: Arc::new(NoRemoteHookRouting),
            remote_pane_grants: Arc::new(NoRemotePaneGrants),
            shortcut_platform: ShortcutPlatform::current(),
            clock: AppClock::system(),
            qa_network_responses: None,
            notification_presenter: Arc::new(SilentNotificationPresenter),
            notification_callbacks: NotificationDeliveryCallbacks::default(),
            diagram_font_files: Arc::new(Vec::new),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// 좁히기 전의 부모 환경. `HOME`만 설정한 홈으로 맞추고 나머지는 스냅샷 그대로다
    pub(crate) fn parent_environment(&self) -> ShellEnvironment {
        let mut parent_environment = self.base_environment.clone();
        parent_environment.insert("HOME".into(), self.home_directory.clone());
        parent_environment
    }

    /// TS `KANVIBE_DEBUG_TERMINAL=true`. 스냅샷에서 한 번 읽는다
    pub(crate) fn is_terminal_debug_logging_enabled(&self) -> bool {
        self.base_environment
            .get("KANVIBE_DEBUG_TERMINAL")
            .is_some_and(|value| value == "true")
    }
}
