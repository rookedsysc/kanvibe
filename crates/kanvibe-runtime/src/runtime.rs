//! 데스크톱 런타임. 한 프로세스에 하나를 만들고, 서버와 GPUI 앱이 접근자로 같은 서비스 인스턴스를 받는다.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kanvibe_ai::accounts::account_service::AiAccountService;
use kanvibe_ai::accounts::desktop_login_bridge::DesktopLoginBridge;
use kanvibe_ai::accounts::usage_service::AiUsageService;
use kanvibe_ai::host::SessionHost;
use kanvibe_ai::pane_chat::session_bindings::PaneChatSessionBindings;
use kanvibe_ai::running_agent_panes::RunningAgentPaneSource;
use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_pane_mirror::{PaneChatRuntime, PaneMirror};
use kanvibe_process::exec::ShellCommandRunner;
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::github_cli_dependency::GitHubCliDependency;
use kanvibe_process::hooks::install_bundle::HookInstallScripts;
use kanvibe_process::hooks::installer::KanvibeHooksInstaller;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_process::task_target_propagation::TaskTargetPropagation;
use kanvibe_services::background_task_sync::BackgroundTaskSync;
use kanvibe_services::board_notifier::BoardNotifier;
use kanvibe_services::desktop_notification::DesktopNotificationService;
use kanvibe_services::diff::DiffService;
use kanvibe_services::hook_status::HookStatusService;
use kanvibe_services::kanban::KanbanService;
use kanvibe_services::notification_store::NotificationStore;
use kanvibe_services::project::ProjectService;
use kanvibe_services::release_update::ReleaseUpdateService;
use kanvibe_services::remote_image_paste::RemoteImagePaste;
use kanvibe_services::session_dependency::SessionDependencyService;
use kanvibe_services::terminal_ports::TaskTerminalSessions;
use kanvibe_terminal::environment::TerminalHostEnvironment;
use kanvibe_terminal::herdr::HerdrService;
use kanvibe_terminal::terminal_bridge::TerminalBridge;
use kanvibe_terminal::terminal_registry::TerminalRegistry;
use kanvibe_terminal::terminal_tab_service::TerminalTabService;
use kanvibe_terminal::terminal_tabs::ZellijIdTargetingSupportCache;

use crate::ai::{AiParts, build_ai};
use crate::board::{BoardInputs, BoardParts, build_board, leak_process_lifetime_references};
use crate::clock::AppClock;
use crate::config::RuntimeConfig;
use crate::desktop::{DesktopParts, build_desktop};
use crate::diagram::SharedDiagramRenderer;
use crate::foundation::{Foundation, build_foundation};
use crate::hooks::{HookParts, HookServerAddress, TaskCliTokens, build_hooks};
use crate::pane_chat::{PaneChatInputs, PaneChatParts, build_pane_chat};
use crate::task_cli::DesktopTaskCliBackend;
use crate::terminal::{TerminalParts, build_terminal, build_terminal_bridge};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Database(#[from] kanvibe_db::DatabaseError),
}

pub struct DesktopRuntime {
    app_data_directory: PathBuf,
    shortcut_platform: ShortcutPlatform,
    clock: AppClock,
    foundation: Foundation,
    terminal: TerminalParts,
    terminal_bridge: Arc<TerminalBridge>,
    hooks: HookParts,
    ai: AiParts,
    board: BoardParts,
    task_cli_backend: Arc<DesktopTaskCliBackend>,
    pane_chat: PaneChatParts,
    desktop: DesktopParts,
    /// [`DesktopRuntime::start`]를 이미 불렀는지. 두 번째 호출은 아무것도 하지 않는다
    is_started: AtomicBool,
}

impl DesktopRuntime {
    /// DB를 열고(필요한 마이그레이션만 실행) 서비스 그래프 전체를 만든다. 타이머나 서버는 시작하지 않는다
    pub fn new(config: RuntimeConfig) -> Result<Self, RuntimeError> {
        let foundation = build_foundation(&config)?;
        let terminal = build_terminal(&foundation, &config);
        let hooks = build_hooks(&foundation, &config);
        let terminal_bridge = build_terminal_bridge(&foundation, &terminal, &hooks.installer);
        let ai = build_ai(&foundation, &config, &terminal.herdr);
        let references = leak_process_lifetime_references(&foundation, &hooks.address);
        let board = build_board(&BoardInputs {
            config: &config,
            foundation: &foundation,
            terminal: &terminal,
            hooks: &hooks,
            ai: &ai,
            references,
        });
        let task_cli_backend =
            build_task_cli_backend(&foundation, &hooks, &board, &terminal_bridge);
        let pane_chat = build_pane_chat(&PaneChatInputs {
            config: &config,
            foundation: &foundation,
            terminal: &terminal,
            terminal_bridge: &terminal_bridge,
            board: &board,
            ai: &ai,
        });
        let desktop = build_desktop(&config, &foundation, &board);
        Ok(Self {
            app_data_directory: config.app_data_directory,
            shortcut_platform: config.shortcut_platform,
            clock: config.clock,
            foundation,
            terminal,
            terminal_bridge,
            hooks,
            ai,
            board,
            task_cli_backend,
            pane_chat,
            desktop,
            is_started: AtomicBool::new(false),
        })
    }

    /// TS `app.whenReady` 뒤 main.js가 켜던 UI 밖 서비스: 백그라운드 task sync 예약 루프와 보드 이벤트 알림 전달.
    /// 프로세스에서 한 번만 켠다. 두 번째 호출은 무시해 같은 이벤트가 알림 두 개로 남지 않게 한다
    pub fn start(&self) {
        if self.is_started.swap(true, Ordering::SeqCst) {
            return;
        }
        self.board.background_task_sync.start_background_task_sync();
        self.desktop.notifications.start(&self.foundation.board);
    }

    /// TS `before-quit` 경로 가운데 UI 밖 서비스 정리. 알림 전달과 sync 예약을 멈추고 pane 미러와 로컬 PTY를 닫는다
    /// (원격 daemon PTY는 detach만 한다). hook 서버 정리는 그 서버를 띄운 쪽이 맡는다
    pub fn shutdown(&self) {
        self.desktop.notifications.stop();
        self.board.background_task_sync.stop_background_task_sync();
        self.pane_chat.mirror.stop_all_pane_mirrors(true);
        self.terminal.registry.kill_all_terminal_sessions();
    }
}

fn build_task_cli_backend(
    foundation: &Foundation,
    hooks: &HookParts,
    board: &BoardParts,
    terminal_bridge: &Arc<TerminalBridge>,
) -> Arc<DesktopTaskCliBackend> {
    Arc::new(DesktopTaskCliBackend {
        database: Arc::clone(&foundation.database),
        tokens: Arc::clone(&hooks.tokens),
        kanban: Arc::clone(&board.kanban),
        project: Arc::clone(&board.project),
        board: Arc::clone(&foundation.board),
        terminal_bridge: Arc::clone(terminal_bridge),
        task_target_propagation: Arc::clone(&board.task_target_propagation),
    })
}

/// 바닥 계층 접근자
impl DesktopRuntime {
    pub fn app_data_directory(&self) -> &Path {
        &self.app_data_directory
    }

    pub fn database(&self) -> &Arc<Database> {
        &self.foundation.database
    }

    /// AppSettings. 앱 전역 UI 설정의 원본이다
    pub fn app_settings(&self) -> AppSettingsService<'_> {
        AppSettingsService::new(&self.foundation.database, self.shortcut_platform)
    }

    /// AppSettings 단축키 표가 따르는 플랫폼
    pub fn shortcut_platform(&self) -> ShortcutPlatform {
        self.shortcut_platform
    }

    /// 서비스와 화면이 함께 보는 현재 시각
    pub fn clock(&self) -> AppClock {
        self.clock
    }

    /// 자식 프로세스·PTY에 넘기는 좁힌 환경(`PORT`, `HOST`, `NODE_ENV`, `KANVIBE_*` 제외)
    pub fn terminal_environment(&self) -> &Arc<TerminalHostEnvironment> {
        &self.foundation.environment
    }

    pub fn shell_runner(&self) -> &Arc<dyn ShellCommandRunner> {
        &self.foundation.runner
    }

    pub fn system_shell_runner(&self) -> &Arc<SystemShellCommandRunner> {
        &self.foundation.system_runner
    }

    pub fn host_files(&self) -> &Arc<HostFileAccess> {
        &self.foundation.files
    }

    pub fn ssh_config(&self) -> &Arc<SshConfigReader> {
        &self.foundation.ssh_config
    }

    pub fn board_notifier(&self) -> &Arc<BoardNotifier> {
        &self.foundation.board
    }
}

/// 터미널 계층 접근자
impl DesktopRuntime {
    pub fn herdr(&self) -> &Arc<HerdrService> {
        &self.terminal.herdr
    }

    pub fn terminal_registry(&self) -> &Arc<TerminalRegistry> {
        &self.terminal.registry
    }

    pub fn terminal_bridge(&self) -> &Arc<TerminalBridge> {
        &self.terminal_bridge
    }

    pub fn task_terminal_sessions(&self) -> Arc<dyn TaskTerminalSessions> {
        self.terminal.task_sessions.clone()
    }

    pub fn terminal_tab_service(&self) -> &Arc<TerminalTabService> {
        &self.terminal.tab_service
    }

    /// 탭 서비스와 pane 축 서비스가 함께 쓰는 zellij 버전 판정 캐시
    pub fn zellij_id_targeting_support(&self) -> &Arc<ZellijIdTargetingSupportCache> {
        &self.terminal.zellij_id_targeting_support
    }

    pub fn remote_image_paste(&self) -> &Arc<RemoteImagePaste> {
        &self.terminal.remote_image_paste
    }
}

/// hook·task CLI 접근자
impl DesktopRuntime {
    pub fn hooks_installer(&self) -> &Arc<KanvibeHooksInstaller> {
        &self.hooks.installer
    }

    /// hook 서버의 `/api/install/hooks.sh`가 읽는 설치 스크립트 보관소. 설치기와 같은 인스턴스다
    pub fn hook_install_scripts(&self) -> &Arc<HookInstallScripts> {
        &self.hooks.install_scripts
    }

    pub fn hook_server_address(&self) -> &Arc<HookServerAddress> {
        &self.hooks.address
    }

    pub fn task_cli_tokens(&self) -> &Arc<TaskCliTokens> {
        &self.hooks.tokens
    }

    /// `TaskCliService::new(runtime.task_cli_backend().as_ref())`로 요청을 처리한다
    pub fn task_cli_backend(&self) -> &Arc<DesktopTaskCliBackend> {
        &self.task_cli_backend
    }

    pub fn hook_status(&self) -> &Arc<HookStatusService> {
        &self.board.hook_status
    }
}

/// 보드·프로젝트 계층 접근자
impl DesktopRuntime {
    pub fn kanban(&self) -> &Arc<KanbanService> {
        &self.board.kanban
    }

    pub fn project(&self) -> &Arc<ProjectService> {
        &self.board.project
    }

    pub fn task_target_propagation(&self) -> &Arc<TaskTargetPropagation<'static>> {
        &self.board.task_target_propagation
    }

    pub fn diff(&self) -> &Arc<DiffService> {
        &self.board.diff
    }

    pub fn notification_store(&self) -> &Arc<NotificationStore> {
        &self.board.notification_store
    }

    pub fn background_task_sync(&self) -> &Arc<BackgroundTaskSync> {
        &self.board.background_task_sync
    }

    pub fn session_dependency(&self) -> &Arc<SessionDependencyService> {
        &self.board.session_dependency
    }

    /// `gh` 설치 확인·설치. 원격 작업의 PR을 찾기 전에 쓴다
    pub fn github_cli_dependency(&self) -> &Arc<GitHubCliDependency> {
        &self.board.github_cli_dependency
    }
}

/// AI 계층 접근자
impl DesktopRuntime {
    /// 로컬 홈 기준 세션 기록 호스트. 원격은 `with_ssh_host`로 바꿔 쓴다
    pub fn ai_session_host(&self) -> &SessionHost {
        &self.ai.session_host
    }

    pub fn running_agent_panes(&self) -> &Arc<dyn RunningAgentPaneSource> {
        &self.ai.running_agent_panes
    }

    pub fn pane_chat_session_bindings(&self) -> &Arc<PaneChatSessionBindings> {
        &self.ai.pane_chat_session_bindings
    }

    pub fn ai_account_service(&self) -> &Arc<AiAccountService> {
        &self.ai.account_service
    }

    pub fn ai_usage_service(&self) -> &Arc<AiUsageService> {
        &self.ai.usage_service
    }

    /// 사용량 조회와 계정 CLI가 쓰는 문맥(홈, 좁힌 환경, HTTP·프로세스 실행기)
    pub fn ai_usage_context(&self) -> &kanvibe_ai::usage::context::UsageContext {
        &self.ai.usage_context
    }

    /// 원격·로컬 세션 도구(tmux·zellij·herdr) 확인 캐시. 터미널 브리지와 pane 미러가 함께 쓴다
    pub fn session_dependencies(
        &self,
    ) -> &Arc<kanvibe_process::remote_session_dependency::SessionDependencies> {
        &self.terminal.session_dependencies
    }
}

/// pane chat·알림·릴리스·다이어그램 접근자
impl DesktopRuntime {
    /// 데스크톱 채팅 화면과 모바일이 함께 쓰는 pane 미러
    pub fn pane_mirror(&self) -> &Arc<PaneMirror> {
        &self.pane_chat.mirror
    }

    /// pane chat 서비스(협력자 8개)와 구독 감시·모드 저장소
    pub fn pane_chat(&self) -> &Arc<PaneChatRuntime> {
        &self.pane_chat.pane_chat
    }

    pub fn ai_desktop_login_bridge(&self) -> &Arc<DesktopLoginBridge> {
        &self.ai.desktop_login_bridge
    }

    /// 보드 이벤트를 알림 목록과 OS 알림으로 옮기는 서비스. 전달 구독은 [`DesktopRuntime::start`]가 건다
    pub fn desktop_notifications(&self) -> &Arc<DesktopNotificationService> {
        &self.desktop.notifications.service
    }

    pub fn release_update(&self) -> &Arc<ReleaseUpdateService> {
        &self.desktop.release_update
    }

    /// 모바일 다이어그램 라우트가 쓰는 렌더러(TS `mobileDiagramRenderService`)
    pub fn diagram_renderer(&self) -> &Arc<SharedDiagramRenderer> {
        &self.desktop.diagram
    }
}
