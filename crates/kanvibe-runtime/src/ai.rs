//! AI 세션 기록·실시간 세션·계정·사용량 조립(TS `aiAccountService`, `aiUsageService`, `runningAgentPanes`,
//! pane chat 세션 바인딩). 홈과 환경 변수는 시작 스냅샷을 좁힌 값만 넘긴다.

use std::sync::{Arc, Mutex};

use kanvibe_ai::accounts::account_service::AiAccountService;
use kanvibe_ai::accounts::desktop_login_bridge::DesktopLoginBridge;
use kanvibe_ai::accounts::settings_store::AppSettingStore;
use kanvibe_ai::accounts::usage_service::AiUsageService;
use kanvibe_ai::host::SessionHost;
use kanvibe_ai::pane_chat::session_bindings::PaneChatSessionBindings;
use kanvibe_ai::pane_chat::session_catalog::RecordedSessionCatalog;
use kanvibe_ai::running_agent_panes::RunningAgentPaneSource;
use kanvibe_ai::usage::context::{HttpTransport, UsageContext, UsagePlatform, WarningSink};
use kanvibe_ai::usage::system_transport::{SystemProcessLauncher, UreqHttpTransport};
use kanvibe_terminal::herdr::HerdrService;
use kanvibe_terminal::login_terminal::PtyLoginTerminalSpawner;
use kanvibe_terminal::pty::NativePtySpawner;
use kanvibe_terminal::running_agent_panes::MultiplexerRunningAgentPanes;
use kanvibe_terminal::session_command_runner::ShellSessionCommandRunner;

use crate::config::RuntimeConfig;
use crate::foundation::Foundation;
use crate::qa_network::TableHttpTransport;

/// GUI로 띄운 앱은 로그인 셸 PATH를 받지 못할 수 있어 CLI를 PATH 밖에서도 찾는다(TS `findGeminiBinary`)
const SYSTEM_BINARY_DIRECTORIES: [&str; 2] = ["/usr/local/bin", "/opt/homebrew/bin"];

pub(crate) struct AiParts {
    pub session_host: SessionHost,
    pub running_agent_panes: Arc<dyn RunningAgentPaneSource>,
    pub pane_chat_session_bindings: Arc<PaneChatSessionBindings>,
    pub account_service: Arc<AiAccountService>,
    pub usage_service: Arc<AiUsageService>,
    /// AI 계정 화면의 provider 로그인 PTY(TS `aiAccountLoginBridge`)
    pub desktop_login_bridge: Arc<DesktopLoginBridge>,
    /// 모바일 로그인 브리지처럼 계정 CLI를 직접 띄우는 쪽이 같은 문맥을 쓴다
    pub usage_context: UsageContext,
}

pub(crate) fn build_ai(
    foundation: &Foundation,
    config: &RuntimeConfig,
    herdr: &Arc<HerdrService>,
) -> AiParts {
    let session_host = SessionHost::local(
        config.home_directory.as_str(),
        foundation.environment.shell_environment.clone(),
        Arc::new(ShellSessionCommandRunner::new(Arc::clone(
            &foundation.runner,
        ))),
    );
    let usage_context = build_usage_context(foundation, config);
    let store: Arc<dyn AppSettingStore> = foundation.database.clone();
    AiParts {
        session_host,
        running_agent_panes: Arc::new(MultiplexerRunningAgentPanes::new(
            Arc::clone(&foundation.runner),
            herdr.clone(),
        )),
        pane_chat_session_bindings: Arc::new(PaneChatSessionBindings::new(Arc::new(
            RecordedSessionCatalog,
        ))),
        account_service: Arc::new(AiAccountService::new(
            Arc::clone(&store),
            usage_context.clone(),
            Arc::new(usage_context.clone()),
        )),
        usage_service: Arc::new(AiUsageService::new(
            store,
            Arc::new(usage_context.clone()),
            Arc::clone(&usage_context.clock),
            usage_context.warn.clone(),
        )),
        desktop_login_bridge: Arc::new(DesktopLoginBridge::new(
            usage_context.clone(),
            Arc::new(PtyLoginTerminalSpawner::new(Arc::new(NativePtySpawner))),
            usage_context.warn.clone(),
        )),
        usage_context,
    }
}

fn current_usage_platform() -> UsagePlatform {
    if cfg!(target_os = "macos") {
        UsagePlatform::Darwin
    } else if cfg!(target_os = "linux") {
        UsagePlatform::Linux
    } else {
        UsagePlatform::Other
    }
}

/// 사용량 조회와 계정 CLI가 쓰는 문맥. 사용자명은 스냅샷의 `USER`(없으면 `LOGNAME`)다
fn build_usage_context(foundation: &Foundation, config: &RuntimeConfig) -> UsageContext {
    let user_name = ["USER", "LOGNAME"]
        .into_iter()
        .find_map(|key| config.base_environment.get(key))
        .cloned()
        .unwrap_or_default();
    // 호출자가 `[ai-usage]`·`[ai-login]` 접두어를 이미 붙인다
    let warn: WarningSink = Arc::new(|message: &str| eprintln!("{message}"));
    UsageContext {
        home_directory: config.home_directory.clone(),
        process_variables: Arc::new(foundation.environment.shell_environment.clone()),
        platform: current_usage_platform(),
        user_name,
        system_binary_directories: SYSTEM_BINARY_DIRECTORIES.map(str::to_string).to_vec(),
        http: build_http_transport(config),
        processes: Arc::new(SystemProcessLauncher),
        clock: Arc::new(config.clock),
        warn,
        claude_retry_allowed_at_ms: Arc::new(Mutex::new(0.0)),
    }
}

/// QA 응답 표가 있으면 실제 네트워크 대신 표로 답한다
fn build_http_transport(config: &RuntimeConfig) -> Arc<dyn HttpTransport> {
    match &config.qa_network_responses {
        Some(table) => Arc::new(TableHttpTransport(Arc::clone(table))),
        None => Arc::new(UreqHttpTransport::default()),
    }
}
