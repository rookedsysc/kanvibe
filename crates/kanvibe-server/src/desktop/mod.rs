//! 서버 경계([`crate::bridge::MobileBridge`], [`crate::routes::hooks::HookBackend`])를 실제 데스크톱 서비스에 잇는 조립.
//!
//! 서비스 그래프(pane 미러·pane chat·다이어그램 렌더러·보드 알림 전달 포함)는 `kanvibe-runtime`이 만들고, 여기서는
//! 그 위에 모바일 업무 계층(`kanvibe-mobile`)만 얹는다. GPUI 앱도 같은 조립을 쓰도록 공개한다.

mod bridge_impl;
mod dates;
mod errors;
mod feature_calls;
pub mod hook_backend;
pub mod in_process;
mod login_spawner;
pub mod remote;

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
use kanvibe_pane_mirror::{PaneChatRuntime, PaneMirror};
use kanvibe_runtime::DesktopRuntime;

/// 런타임 위에 얹는 값. 프로세스 환경은 호출자가 시작할 때 읽어 넘긴다
pub struct DesktopServerConfig {
    pub home_directory: PathBuf,
    pub mobile_tls_port: u16,
    /// `KANVIBE_MOBILE_PUSH_RELAY_URL`·`_TOKEN`. 둘 다 있어야 푸시를 보낸다
    pub push_relay: Option<PushRelayConfig>,
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
    /// 런타임의 pane 미러(데스크톱 채팅 화면과 같은 인스턴스)
    pub mirror: Arc<PaneMirror>,
    /// 런타임의 pane chat(데스크톱 채팅 화면과 같은 인스턴스)
    pane_chat: Arc<PaneChatRuntime>,
}

impl DesktopServices {
    pub fn new(runtime: Arc<DesktopRuntime>, config: DesktopServerConfig) -> Self {
        let devices = Arc::new(MobileDevices::new(MobileDevicesParts {
            database: runtime.database().clone(),
            clock: system_clock(),
        }));
        let mirror = Arc::clone(runtime.pane_mirror());
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
            pane_chat: Arc::clone(runtime.pane_chat()),
            lan_usage: Arc::new(LanPlaintextUsage::new(
                runtime.database().clone(),
                cfg!(windows),
            )),
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

    /// 푸시·이벤트 전달만 멈춘다. pane 미러와 원격 agent는 런타임 `shutdown()`이 멈춘다
    pub fn stop(&self) {
        self.notifications.stop();
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

/// TS `startMobileNotifications`의 `KANVIBE_MOBILE_PUSH_RELAY_URL`·`_TOKEN`. 둘 다 있어야 푸시를 보낸다.
/// 값은 호출자가 시작할 때 잡아 둔 환경 스냅샷에서만 읽는다
pub fn push_relay_config(
    environment: &kanvibe_process::shell_environment::ShellEnvironment,
) -> Option<PushRelayConfig> {
    let relay_url = environment.get("KANVIBE_MOBILE_PUSH_RELAY_URL")?.clone();
    let relay_token = environment.get("KANVIBE_MOBILE_PUSH_RELAY_TOKEN")?.clone();
    Some(PushRelayConfig {
        relay_url,
        relay_token,
    })
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
