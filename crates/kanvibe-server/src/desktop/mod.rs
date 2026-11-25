//! 서버 경계([`crate::bridge::MobileBridge`], [`crate::routes::hooks::HookBackend`])를 실제 데스크톱 서비스에 잇는 조립.
//!
//! 서비스 그래프는 `kanvibe-runtime`이 만들고, 여기서는 그 위에 모바일 업무 계층(`kanvibe-mobile`), pane 미러와
//! pane chat(`kanvibe-pane-mirror`), 다이어그램 렌더러를 얹는다. GPUI 앱도 같은 조립을 쓰도록 공개한다.

pub mod board_notifications;
mod bridge_impl;
mod dates;
mod errors;
mod feature_calls;
pub mod hook_backend;
mod login_spawner;
pub mod remote;
mod remote_grants;

use std::path::PathBuf;
use std::sync::Arc;

use kanvibe_mobile::adapters::{DiffFileAccess, ProcessPeerTaskRegistration};
use kanvibe_mobile::bridge::{MobileBridge as MobileBoardBridge, MobileBridgeParts};
use kanvibe_mobile::clock::system_clock;
use kanvibe_mobile::devices::{MobileDevices, MobileDevicesParts};
use kanvibe_mobile::features::ai::{MobileAi, MobileAiParts};
use kanvibe_mobile::features::{MobileFeatures, MobileFeaturesParts};
use kanvibe_mobile::lan_plaintext::LanPlaintextUsage;
use kanvibe_mobile::notifications::{
    MobileNotifications, MobileNotificationsParts, PushRelayConfig,
};
use kanvibe_mobile::pairing::thread_sleep_delay;
use kanvibe_pane_mirror::{
    KanbanMirrorTaskStore, KanbanPaneChatTaskLookup, MirrorTimings, PaneChatRuntime,
    PaneChatRuntimeParts, PaneMirror, PaneMirrorParts, RemoteImagePasteUploader, RemotePaneGrants,
    SystemMirrorProcessSpawner,
};
use kanvibe_runtime::DesktopRuntime;

use crate::diagram::{DiagramRenderer, UnavailableDiagramRenderer};

/// 런타임 위에 얹는 값. 프로세스 환경은 호출자가 시작할 때 읽어 넘긴다
pub struct DesktopServerConfig {
    pub home_directory: PathBuf,
    pub mobile_tls_port: u16,
    /// `KANVIBE_MOBILE_PUSH_RELAY_URL`·`_TOKEN`. 둘 다 있어야 푸시를 보낸다
    pub push_relay: Option<PushRelayConfig>,
    /// 원격 Herdr 직접 연결 권한. 원격 호스트 연결이 없으면 `None`이라 권한을 내지 않는다
    pub remote_grants: Option<Arc<dyn RemotePaneGrants>>,
    pub diagram_renderer: Option<Arc<dyn DiagramRenderer>>,
}

/// 모바일 서버가 부르는 서비스 묶음
pub struct DesktopServices {
    pub runtime: Arc<DesktopRuntime>,
    pub devices: Arc<MobileDevices>,
    board: Arc<MobileBoardBridge>,
    features: Arc<MobileFeatures>,
    ai: Arc<MobileAi>,
    pub notifications: Arc<MobileNotifications>,
    pub lan_usage: Arc<LanPlaintextUsage>,
    pub mirror: Arc<PaneMirror>,
    pane_chat: Arc<PaneChatRuntime>,
    diagram: Arc<dyn DiagramRenderer>,
}

impl DesktopServices {
    pub fn new(runtime: Arc<DesktopRuntime>, config: DesktopServerConfig) -> Self {
        let devices = Arc::new(MobileDevices::new(MobileDevicesParts {
            database: runtime.database().clone(),
            clock: system_clock(),
            pairing_failure_delay: thread_sleep_delay(),
        }));
        let remote_grants = config
            .remote_grants
            .clone()
            .unwrap_or_else(|| Arc::new(remote_grants::NoRemotePaneGrants));
        let mirror = build_mirror(&runtime, remote_grants);
        let notifications = Arc::new(MobileNotifications::new(MobileNotificationsParts {
            database: runtime.database().clone(),
            board: runtime.board_notifier().clone(),
            registry: devices.clone(),
            relay: config.push_relay.clone(),
            clock: system_clock(),
        }));
        Self {
            board: build_board_bridge(&runtime, &config),
            features: build_features(&runtime, &mirror),
            ai: build_ai(&runtime),
            pane_chat: build_pane_chat(&runtime, &mirror),
            lan_usage: Arc::new(LanPlaintextUsage::new(
                runtime.database().clone(),
                cfg!(windows),
            )),
            diagram: config
                .diagram_renderer
                .unwrap_or_else(|| Arc::new(UnavailableDiagramRenderer)),
            devices,
            notifications,
            mirror,
            runtime,
        }
    }

    /// 앱이 시작할 때 TS가 하던 일: 푸시·이벤트 전달 시작
    pub fn start(&self) {
        self.notifications.start();
    }

    pub fn stop(&self) {
        self.notifications.stop();
        self.mirror.stop_all_pane_mirrors(true);
    }
}

fn build_board_bridge(
    runtime: &Arc<DesktopRuntime>,
    config: &DesktopServerConfig,
) -> Arc<MobileBoardBridge> {
    let address = runtime.hook_server_address().clone();
    Arc::new(MobileBoardBridge::new(MobileBridgeParts {
        database: runtime.database().clone(),
        board: runtime.board_notifier().clone(),
        tasks: runtime.kanban().clone(),
        projects: runtime.project().clone(),
        diff: Arc::new(DiffFileAccess {
            diff: (**runtime.diff()).clone(),
            files: runtime.host_files().clone(),
        }),
        session_dependencies: runtime.session_dependency().clone(),
        peers: Arc::new(ProcessPeerTaskRegistration {
            files: runtime.host_files().clone(),
            hook_server_url: Arc::new(move |ssh_host| {
                address.url(ssh_host).map_err(|error| error.to_string())
            }),
        }),
        notifications: runtime.notification_store().clone(),
        home_directory: config.home_directory.clone(),
        mobile_tls_port: config.mobile_tls_port,
    }))
}

fn build_features(runtime: &Arc<DesktopRuntime>, mirror: &Arc<PaneMirror>) -> Arc<MobileFeatures> {
    Arc::new(MobileFeatures::new(MobileFeaturesParts {
        database: runtime.database().clone(),
        tasks: runtime.kanban().clone(),
        projects: runtime.project().clone(),
        diff: Arc::new(DiffFileAccess {
            diff: (**runtime.diff()).clone(),
            files: runtime.host_files().clone(),
        }),
        tabs: runtime.terminal_tab_service().clone(),
        session_dependencies: runtime.session_dependency().clone(),
        background_sync: runtime.background_task_sync().clone(),
        notifications: runtime.notification_store().clone(),
        panes: Arc::new(bridge_impl::MirrorPaneAccess(mirror.clone())),
        images: runtime.terminal_bridge().clone(),
    }))
}

fn build_ai(runtime: &Arc<DesktopRuntime>) -> Arc<MobileAi> {
    let logins = kanvibe_ai::accounts::mobile_login_bridge::MobileLoginBridge::new(
        runtime.ai_usage_context().clone(),
        Arc::new(login_spawner::NodePtyCompatibleLoginSpawner::new(Arc::new(
            kanvibe_terminal::login_terminal::PtyLoginTerminalSpawner::new(Arc::new(
                kanvibe_terminal::pty::NativePtySpawner,
            )),
        ))),
        Arc::new(kanvibe_ai::accounts::mobile_login_bridge::ThreadTimerScheduler),
    );
    Arc::new(MobileAi::new(MobileAiParts {
        tasks: runtime.kanban().clone(),
        projects: runtime.project().clone(),
        accounts: runtime.ai_account_service().clone(),
        usage: runtime.ai_usage_service().clone(),
        logins: Arc::new(logins),
    }))
}

fn build_mirror(
    runtime: &Arc<DesktopRuntime>,
    remote_grants: Arc<dyn RemotePaneGrants>,
) -> Arc<PaneMirror> {
    Arc::new(PaneMirror::new(PaneMirrorParts {
        tasks: Arc::new(KanbanMirrorTaskStore::new(
            runtime.kanban().clone(),
            runtime.database().clone(),
        )),
        runner: runtime.shell_runner().clone(),
        processes: Arc::new(SystemMirrorProcessSpawner::new(
            runtime.terminal_environment().clone(),
            runtime.ssh_config().clone(),
        )),
        terminals: runtime.terminal_registry().clone(),
        attacher: runtime.terminal_bridge().clone(),
        tabs: runtime.terminal_tab_service().clone(),
        herdr: runtime.herdr().clone(),
        session_dependencies: runtime.session_dependencies().clone(),
        zellij_id_targeting_support: runtime.zellij_id_targeting_support().clone(),
        remote_grants,
        board: runtime.board_notifier().clone(),
        timings: MirrorTimings::default(),
    }))
}

fn build_pane_chat(
    runtime: &Arc<DesktopRuntime>,
    mirror: &Arc<PaneMirror>,
) -> Arc<PaneChatRuntime> {
    use kanvibe_terminal::pane_chat::attachments::PaneChatAttachmentStore;
    use kanvibe_terminal::pane_chat::composer_catalog::PaneChatComposerCatalog;
    use kanvibe_terminal::pane_chat::mode_store::PaneChatModeStore;
    use kanvibe_terminal::pane_chat::service::{AiPaneChatSessionRecords, SystemPaneChatClock};
    use kanvibe_terminal::pane_chat::subscription::NotifyTranscriptFileWatchers;
    use kanvibe_terminal::pane_chat::subscription::ThreadPaneChatTimers;
    let attachments = PaneChatAttachmentStore::new(
        runtime.app_data_directory().to_path_buf(),
        runtime.shell_runner().clone(),
        runtime.ssh_config().clone(),
        Arc::new(RemoteImagePasteUploader::new(
            runtime.remote_image_paste().clone(),
        )),
    );
    let sessions = AiPaneChatSessionRecords::new(
        runtime.ai_session_host().clone(),
        None,
        runtime.pane_chat_session_bindings().clone(),
    );
    Arc::new(PaneChatRuntime::new(PaneChatRuntimeParts {
        mirror: mirror.clone(),
        tasks: Arc::new(KanbanPaneChatTaskLookup::new(runtime.kanban().clone())),
        runner: runtime.shell_runner().clone(),
        terminals: runtime.terminal_registry().clone(),
        herdr: runtime.herdr().clone(),
        sessions: Arc::new(sessions),
        suggestions: Arc::new(PaneChatComposerCatalog::new(runtime.host_files().clone())),
        attachments: Arc::new(attachments),
        modes: Arc::new(PaneChatModeStore::new(runtime.database().clone())),
        clock: Arc::new(SystemPaneChatClock),
        timers: Arc::new(ThreadPaneChatTimers::new()),
        file_watchers: Arc::new(NotifyTranscriptFileWatchers),
    }))
}

/// 평문 응답 기록을 LAN 사용 서비스에 넘긴다
pub struct LanPlaintextRecorder(pub Arc<LanPlaintextUsage>);

impl crate::server::LanPlaintextUsage for LanPlaintextRecorder {
    fn record_lan_plaintext_response(
        &self,
        remote_address: std::net::IpAddr,
        url: &str,
        status: u16,
    ) {
        let now_ms = kanvibe_ai::javascript::now_ms() as i64;
        self.0.record_lan_plaintext_response(
            Some(&remote_address.to_string()),
            Some(url),
            status,
            now_ms,
        );
    }
}
