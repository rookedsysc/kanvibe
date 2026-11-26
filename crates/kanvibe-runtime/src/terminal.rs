//! 터미널 계층 조립: Herdr, PTY 레지스트리, 탭 서비스, 데스크톱·모바일이 함께 쓰는 터미널 브리지.
//! kanvibe-terminal과 kanvibe-services는 서로 모르므로, 두 crate의 trait을 잇는 어댑터도 여기 둔다.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use kanvibe_process::hooks::installer::KanvibeHooksInstaller;
use kanvibe_process::remote_session_dependency::SessionDependencies;
use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_process::worktree::HerdrTaskWorkspaces;
use kanvibe_services::board_notifier::{
    BoardNotifier, HerdrMachineRegistrationFailedPayload, HerdrServerHandoffRequiredPayload,
    TaskHookInstallFailedPayload,
};
use kanvibe_services::remote_image_paste::{
    RemoteImagePaste, SystemScpTransport, decode_data_url_to_bytes,
};
use kanvibe_services::terminal_ports::TaskTerminalSessions;
use kanvibe_terminal::environment::TerminalHostEnvironment;
use kanvibe_terminal::herdr::HerdrService;
use kanvibe_terminal::herdr_client_proxy::NativeHerdrClientRelayFactory;
use kanvibe_terminal::pty::NativePtySpawner;
use kanvibe_terminal::terminal_bridge::{
    RemoteImageTransfer, TaskHookInstallFailure, TerminalBridge, TerminalBridgeDependencies,
    TerminalBridgeEvents, TerminalSessions,
};
use kanvibe_terminal::terminal_registry::{
    TerminalEventSink, TerminalRegistry, TerminalRegistryDependencies,
};
use kanvibe_terminal::terminal_tab_service::{TerminalTabService, TerminalTabServiceDependencies};
use kanvibe_terminal::terminal_tabs::ZellijIdTargetingSupportCache;

use crate::config::RuntimeConfig;
use crate::foundation::Foundation;

/// 터미널 계층이 내는 알림을 보드 알림으로 잇는다(TS `boardNotifier`의 Herdr·hook 설치 실패 알림)
pub struct BoardTerminalEvents {
    board: Arc<BoardNotifier>,
}

impl TerminalEventSink for BoardTerminalEvents {
    fn herdr_server_handoff_required(&self, ssh_host: &str, herdr_session: &str) {
        self.board
            .broadcast_herdr_server_handoff_required(HerdrServerHandoffRequiredPayload {
                ssh_host: ssh_host.to_string(),
                herdr_session: herdr_session.to_string(),
            });
    }

    fn herdr_machine_registration_failed(&self, ssh_host: &str, herdr_session: &str, reason: &str) {
        self.board.broadcast_herdr_machine_registration_failed(
            HerdrMachineRegistrationFailedPayload {
                ssh_host: ssh_host.to_string(),
                herdr_session: herdr_session.to_string(),
                reason: reason.to_string(),
            },
        );
    }
}

impl TerminalBridgeEvents for BoardTerminalEvents {
    fn task_hook_install_failed(&self, failure: &TaskHookInstallFailure) {
        self.board
            .broadcast_task_hook_install_failed(TaskHookInstallFailedPayload {
                task_id: failure.task_id.clone(),
                task_title: failure.task_title.clone(),
                error: failure.error.clone(),
            });
    }
}

/// 보드·프로젝트 서비스가 부르는 task 세션 조작(TS `lib/terminal.ts`의 `detachSession`, `replaceHerdrRemoteServer`)을
/// PTY 레지스트리와 Herdr 서비스로 잇는다
pub struct RegistryTaskTerminalSessions {
    registry: Arc<TerminalRegistry>,
    herdr: Arc<HerdrService>,
}

impl HerdrTaskWorkspaces for RegistryTaskTerminalSessions {
    fn close_task_workspace(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String> {
        self.herdr.close_task_workspace(session_name, ssh_host)
    }

    fn is_task_alive(&self, session_name: &str, ssh_host: Option<&str>) -> bool {
        self.herdr.is_task_alive(session_name, ssh_host)
    }
}

impl TaskTerminalSessions for RegistryTaskTerminalSessions {
    fn detach_session(&self, task_id: &str, trigger_label: &str) {
        self.registry.detach_session(task_id, Some(trigger_label));
    }

    fn replace_herdr_remote_server(
        &self,
        ssh_host: &str,
        herdr_session: &str,
    ) -> Result<(), String> {
        self.registry
            .replace_herdr_remote_server(ssh_host, herdr_session);
        Ok(())
    }
}

/// 붙여넣은 이미지를 scp로 원격 호스트에 보낸다(TS `remoteImagePaste`)
pub struct ScpRemoteImageTransfer {
    paste: Arc<RemoteImagePaste>,
}

impl RemoteImageTransfer for ScpRemoteImageTransfer {
    fn decode_data_url(&self, image_data_url: &str) -> Result<Vec<u8>, String> {
        decode_data_url_to_bytes(image_data_url).map_err(|error| error.message)
    }

    fn transfer_image(
        &self,
        ssh_config: &SshHostConfig,
        image_bytes: &[u8],
    ) -> Result<String, String> {
        self.paste
            .transfer_image_to_remote_host(ssh_config, image_bytes)
            .map_err(|error| error.message)
    }
}

/// Node `os.tmpdir()`처럼 좁힌 환경의 `TMPDIR`, `TMP`, `TEMP`, `/tmp` 순으로 고르고 끝의 `/` 하나를 뗀다
pub(crate) fn temporary_directory(environment: &TerminalHostEnvironment) -> PathBuf {
    let configured = ["TMPDIR", "TMP", "TEMP"]
        .into_iter()
        .find_map(|key| environment.variable(key))
        .unwrap_or("/tmp");
    match configured.strip_suffix('/') {
        Some(stripped) if configured.len() > 1 => PathBuf::from(stripped),
        _ => PathBuf::from(configured),
    }
}

pub(crate) struct TerminalParts {
    pub herdr: Arc<HerdrService>,
    pub registry: Arc<TerminalRegistry>,
    pub task_sessions: Arc<RegistryTaskTerminalSessions>,
    pub events: Arc<BoardTerminalEvents>,
    pub session_dependencies: Arc<SessionDependencies>,
    pub remote_image_paste: Arc<RemoteImagePaste>,
    pub zellij_id_targeting_support: Arc<ZellijIdTargetingSupportCache>,
    pub tab_service: Arc<TerminalTabService>,
}

/// hook 설치기 없이 만들 수 있는 터미널 부분. 브리지는 설치기가 생긴 뒤 [`build_terminal_bridge`]가 만든다
pub(crate) fn build_terminal(foundation: &Foundation, config: &RuntimeConfig) -> TerminalParts {
    let environment = &foundation.environment;
    let herdr = Arc::new(HerdrService::new(
        Arc::clone(&foundation.files),
        Arc::clone(environment),
        Arc::clone(&foundation.ssh_config),
    ));
    let events = Arc::new(BoardTerminalEvents {
        board: Arc::clone(&foundation.board),
    });
    let registry = build_registry(foundation, config, &herdr, &events);
    let zellij_id_targeting_support = Arc::new(ZellijIdTargetingSupportCache::new());
    let tab_service = Arc::new(TerminalTabService::new(TerminalTabServiceDependencies {
        database: Arc::clone(&foundation.database),
        runner: Arc::clone(&foundation.runner),
        registry: registry.clone(),
        herdr: herdr.clone(),
        zellij_id_targeting_support: Arc::clone(&zellij_id_targeting_support),
    }));
    let remote_image_paste = Arc::new(RemoteImagePaste::new(
        Arc::new(SystemScpTransport::new(
            environment.shell_environment.clone(),
        )),
        temporary_directory(environment),
    ));
    TerminalParts {
        task_sessions: Arc::new(RegistryTaskTerminalSessions {
            registry: Arc::clone(&registry),
            herdr: Arc::clone(&herdr),
        }),
        session_dependencies: Arc::new(SessionDependencies::new(Arc::clone(&foundation.runner))),
        herdr,
        registry,
        events,
        remote_image_paste,
        zellij_id_targeting_support,
        tab_service,
    }
}

/// PTY의 유일한 소유자. 자식 PTY에는 좁힌 환경만 넘어간다
fn build_registry(
    foundation: &Foundation,
    config: &RuntimeConfig,
    herdr: &Arc<HerdrService>,
    events: &Arc<BoardTerminalEvents>,
) -> Arc<TerminalRegistry> {
    let environment = &foundation.environment;
    TerminalRegistry::with_debug_logging(
        TerminalRegistryDependencies {
            runner: Arc::clone(&foundation.runner),
            environment: Arc::clone(environment),
            pty_spawner: Arc::new(NativePtySpawner),
            herdr: herdr.clone(),
            herdr_relays: Arc::new(NativeHerdrClientRelayFactory::from_host_environment(
                environment,
            )),
            remote_hosts: Arc::clone(&config.remote_host_connections),
            events: events.clone(),
        },
        config.is_terminal_debug_logging_enabled(),
    )
}

/// 데스크톱 화면과 모바일이 함께 지나는 터미널 브리지(TS `terminalBridge`)
pub(crate) fn build_terminal_bridge(
    foundation: &Foundation,
    terminal: &TerminalParts,
    installer: &Arc<KanvibeHooksInstaller>,
) -> Arc<TerminalBridge> {
    let sessions: Arc<dyn TerminalSessions> = Arc::new(Arc::clone(&terminal.registry));
    Arc::new(TerminalBridge::new(TerminalBridgeDependencies {
        sessions,
        store: foundation.database.clone(),
        ssh_hosts: Arc::clone(&foundation.ssh_config),
        session_dependencies: terminal.session_dependencies.clone(),
        transport_cooldowns: foundation.system_runner.clone(),
        hooks: installer.clone(),
        image_transfer: Arc::new(ScpRemoteImageTransfer {
            paste: Arc::clone(&terminal.remote_image_paste),
        }),
        events: terminal.events.clone(),
        clock: Arc::new(Instant::now),
    }))
}
