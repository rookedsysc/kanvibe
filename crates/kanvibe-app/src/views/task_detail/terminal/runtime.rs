//! 앱 프로세스에 하나만 있는 터미널 레지스트리와 브리지(TS Electron main의 `terminal.ts`·`terminalBridge.ts`).
//!
//! 통합 단계가 앱 조립 지점을 만들 때까지는 작업 상세 화면이 처음 터미널을 열 때 GPUI 전역으로 한 번 만든다.
//! 프로세스 환경은 시작할 때 읽기만 하고, 자식 프로세스에는 `create_local_shell_environment`로 좁힌 환경만 넘긴다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{App, Global};
use kanvibe_db::Database;
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::remote_session_dependency::SessionDependencies;
use kanvibe_process::shell_environment::{ShellPlatform, current_user_home_directory};
use kanvibe_process::ssh_config::{SshConfigReader, SshHostConfig};
use kanvibe_services::remote_image_paste::{
    RemoteImagePaste, SystemScpTransport, decode_data_url_to_bytes,
};
use kanvibe_terminal::environment::TerminalHostEnvironment;
use kanvibe_terminal::herdr::HerdrService;
use kanvibe_terminal::herdr_client_proxy::NativeHerdrClientRelayFactory;
use kanvibe_terminal::pty::NativePtySpawner;
use kanvibe_terminal::terminal_bridge::{
    RemoteImageTransfer, TaskHookInstallFailure, TerminalBridge, TerminalBridgeDependencies,
    TerminalBridgeError, TerminalBridgeEvents, TerminalHookVerifier, TerminalWindow,
};
use kanvibe_terminal::terminal_registry::{
    RemoteHostConnection, RemoteHostConnections, TerminalEventSink, TerminalRegistry,
    TerminalRegistryDependencies,
};
use kanvibe_terminal::terminal_tab_service::{TerminalTabService, TerminalTabServiceDependencies};
use kanvibe_terminal::terminal_tabs::ZellijIdTargetingSupportCache;

use crate::services::AppServices;

/// 레지스트리와 브리지 한 벌. 창마다 만들면 같은 세션 PTY가 둘 생긴다
pub struct TerminalRuntime {
    pub registry: Arc<TerminalRegistry>,
    pub bridge: TerminalBridge,
    /// tmux window·zellij tab·Herdr tab·terminal 탭 조작
    pub tabs: TerminalTabService,
    next_window_id: AtomicU64,
}

/// GPUI 전역 자리. DB가 없는 실행(QA 없이 데이터 디렉터리를 주지 않은 경우)에는 비어 있다
#[derive(Default)]
pub struct TerminalRuntimeSlot {
    runtime: Option<Arc<TerminalRuntime>>,
}

impl Global for TerminalRuntimeSlot {}

/// 처음 부를 때 만든다. DB가 없으면 `None`이다
pub fn terminal_runtime(cx: &mut App) -> Option<Arc<TerminalRuntime>> {
    if let Some(runtime) = cx
        .try_global::<TerminalRuntimeSlot>()
        .and_then(|slot| slot.runtime.clone())
    {
        return Some(runtime);
    }
    let database = cx.global::<AppServices>().database()?.clone();
    let runtime = Arc::new(TerminalRuntime::for_current_process(database));
    cx.set_global(TerminalRuntimeSlot {
        runtime: Some(Arc::clone(&runtime)),
    });
    Some(runtime)
}

impl TerminalRuntime {
    fn for_current_process(database: Arc<Database>) -> Self {
        let parent_environment = std::env::vars().collect();
        let home_directory = current_user_home_directory();
        let environment = Arc::new(TerminalHostEnvironment::from_parent_environment(
            &parent_environment,
            ShellPlatform::current(),
            &home_directory,
        ));
        let runner = Arc::new(SystemShellCommandRunner::for_current_process());
        let ssh_hosts = Arc::new(SshConfigReader::new(&environment.home_directory));
        let files = Arc::new(HostFileAccess::new(
            runner.clone(),
            environment.home_directory.clone(),
        ));
        let herdr = Arc::new(HerdrService::new(
            files,
            Arc::clone(&environment),
            Arc::clone(&ssh_hosts),
        ));
        let registry = TerminalRegistry::new(TerminalRegistryDependencies {
            runner: runner.clone(),
            environment: Arc::clone(&environment),
            pty_spawner: Arc::new(NativePtySpawner),
            herdr: herdr.clone(),
            herdr_relays: Arc::new(NativeHerdrClientRelayFactory::from_host_environment(
                &environment,
            )),
            remote_hosts: Arc::new(DirectSshHosts),
            events: Arc::new(LoggedTerminalEvents),
        });
        let image_transfer = ScpImageTransfer {
            paste: RemoteImagePaste::new(
                Arc::new(SystemScpTransport::new(
                    environment.shell_environment.clone(),
                )),
                std::env::temp_dir(),
            ),
        };
        let tabs = TerminalTabService::new(TerminalTabServiceDependencies {
            database: Arc::clone(&database),
            runner: runner.clone(),
            registry: registry.clone(),
            herdr,
            zellij_id_targeting_support: Arc::new(ZellijIdTargetingSupportCache::new()),
        });
        let bridge = TerminalBridge::new(TerminalBridgeDependencies {
            sessions: Arc::new(Arc::clone(&registry)),
            store: database,
            ssh_hosts,
            session_dependencies: Arc::new(SessionDependencies::new(runner.clone())),
            transport_cooldowns: runner,
            hooks: Arc::new(HookServerNotComposed),
            image_transfer: Arc::new(image_transfer),
            events: Arc::new(LoggedTerminalEvents),
            clock: Arc::new(Instant::now),
        });
        Self {
            registry,
            bridge,
            tabs,
            next_window_id: AtomicU64::new(1),
        }
    }

    /// 터미널 뷰 하나가 쓰는 창 자리. 브리지는 (창, task, 탭)마다 클라이언트를 하나 둔다
    pub fn open_window(
        &self,
    ) -> (
        Arc<ViewTerminalWindow>,
        UnboundedReceiver<TerminalWindowEvent>,
    ) {
        let (sender, receiver) = unbounded();
        let window = Arc::new(ViewTerminalWindow {
            window_id: self.next_window_id.fetch_add(1, Ordering::Relaxed),
            sender,
            is_destroyed: AtomicBool::new(false),
        });
        (window, receiver)
    }
}

/// 브리지가 터미널 뷰에 알리는 일
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalWindowEvent {
    /// 레지스트리 화면에 이미 반영된 출력. OSC 52 판정에만 원본 바이트를 쓴다
    Output(String),
    Closed(Option<String>),
}

/// TS `WebContents` 자리. 출력은 채널로 뷰에 넘긴다
pub struct ViewTerminalWindow {
    window_id: u64,
    sender: UnboundedSender<TerminalWindowEvent>,
    is_destroyed: AtomicBool,
}

impl ViewTerminalWindow {
    pub fn id(&self) -> u64 {
        self.window_id
    }

    /// 뷰가 사라질 때 부른다. 이후 출력은 버린다
    pub fn mark_destroyed(&self) {
        self.is_destroyed.store(true, Ordering::Relaxed);
    }
}

impl TerminalWindow for ViewTerminalWindow {
    fn window_id(&self) -> u64 {
        self.window_id
    }

    fn is_destroyed(&self) -> bool {
        self.is_destroyed.load(Ordering::Relaxed)
    }

    fn send_terminal_output(&self, _task_id: &str, _tab_id: Option<&str>, output: &str) {
        let _ = self
            .sender
            .unbounded_send(TerminalWindowEvent::Output(output.to_string()));
    }

    fn send_terminal_closed(&self, _task_id: &str, _tab_id: Option<&str>, reason: Option<&str>) {
        let _ = self
            .sender
            .unbounded_send(TerminalWindowEvent::Closed(reason.map(str::to_string)));
    }
}

/// 원격 host daemon(M4)이 아직 없어 모든 원격 세션은 순수 ssh 경로로 연다
struct DirectSshHosts;

impl RemoteHostConnections for DirectSshHosts {
    fn connection(&self, _ssh_host: &str) -> Option<Arc<dyn RemoteHostConnection>> {
        None
    }
}

/// 보드 알림(boardNotifier)이 앱에 조립되기 전까지는 진단 로그로 남긴다
struct LoggedTerminalEvents;

impl TerminalEventSink for LoggedTerminalEvents {
    fn herdr_server_handoff_required(&self, ssh_host: &str, herdr_session: &str) {
        eprintln!("[터미널] herdr 서버 교체 필요: {ssh_host} {herdr_session}");
    }

    fn herdr_machine_registration_failed(&self, ssh_host: &str, herdr_session: &str, reason: &str) {
        eprintln!("[터미널] herdr machine 등록 실패: {ssh_host} {herdr_session} {reason}");
    }
}

impl TerminalBridgeEvents for LoggedTerminalEvents {
    fn task_hook_install_failed(&self, failure: &TaskHookInstallFailure) {
        eprintln!("[터미널] hook 설치 실패: {failure:?}");
    }
}

/// hook 서버(kanvibe-server)와 그 런타임이 앱에 조립되기 전 자리. 그 전에는 설치할 hook이 가리킬 서버가 없어
/// 확인을 건너뛰고(아무것도 고치지 않았다는 `false`) 진단 로그만 남긴다. 조립 지점이 생기면 `KanvibeHooksInstaller`로 바꾼다
struct HookServerNotComposed;

impl TerminalHookVerifier for HookServerNotComposed {
    fn ensure_hooks_current(
        &self,
        target_path: &str,
        task_id: &str,
        _ssh_host: Option<&str>,
    ) -> Result<bool, TerminalBridgeError> {
        eprintln!(
            "[터미널] hook 서버가 아직 조립되지 않아 hook 확인을 건너뜀: {task_id} {target_path}"
        );
        Ok(false)
    }
}

struct ScpImageTransfer {
    paste: RemoteImagePaste,
}

impl RemoteImageTransfer for ScpImageTransfer {
    fn decode_data_url(&self, image_data_url: &str) -> Result<Vec<u8>, String> {
        decode_data_url_to_bytes(image_data_url).map_err(|error| error.to_string())
    }

    fn transfer_image(
        &self,
        ssh_config: &SshHostConfig,
        image_bytes: &[u8],
    ) -> Result<String, String> {
        self.paste
            .transfer_image_to_remote_host(ssh_config, image_bytes)
            .map_err(|error| error.to_string())
    }
}
