//! AI 계정 화면이 부르는 서비스(`kanvibe_ai`의 계정·사용량 서비스와 로그인 브리지).
//!
//! 이 서비스들은 앱 수명 동안 하나만 있어야 한다(사용량 조회 합류·로그인 PTY 세션 등록부). 앱 셸이 아직 들고 있지 않아서
//! 처음 화면을 열 때 `AiAccountsBackendHandle` 전역을 한 번 만든다. 앱이 같은 서비스를 만들게 되면
//! `install_ai_accounts_backend`로 그 구현을 걸면 된다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use gpui_kit::{App, Global};
use kanvibe_ai::accounts::account_service::{
    AiAccountMutationResult, AiAccountService, AiAccountSummary,
};
use kanvibe_ai::accounts::desktop_login_bridge::{
    AiAccountLoginOpenResult, DesktopLoginBridge, LoginWindow,
};
use kanvibe_ai::accounts::usage_service::AiUsageService;
use kanvibe_ai::usage::context::{
    HttpRequest, HttpResponse, HttpTransport, UsageClock, UsageContext, UsagePlatform,
};
use kanvibe_ai::usage::system_transport::{SystemProcessLauncher, UreqHttpTransport};
use kanvibe_ai::usage::types::{AiUsageProvider, AiUsageSnapshot};
use kanvibe_terminal::login_terminal::PtyLoginTerminalSpawner;
use kanvibe_terminal::pty::NativePtySpawner;

use crate::qa::{AppClock, NetworkResponseTable, QaRuntime};
use crate::services::{AppServices, ServiceResult};
use crate::views::settings::service_call::failure;

/// 화면이 계정 서비스에 바라는 것(TS IPC `aiAccounts.*`, `aiUsage.*`, 로그인 창 통로)
pub trait AiAccountsBackend: Send + Sync {
    fn list_ai_accounts(&self) -> ServiceResult<Vec<AiAccountSummary>>;
    fn add_ai_account(
        &self,
        provider: AiUsageProvider,
        name: &str,
    ) -> ServiceResult<AiAccountMutationResult>;
    fn remove_ai_account(
        &self,
        provider: AiUsageProvider,
        account_root: &str,
    ) -> ServiceResult<AiAccountMutationResult>;
    fn get_cached_ai_usage_snapshot(&self) -> ServiceResult<Option<AiUsageSnapshot>>;
    fn get_ai_usage_snapshot(
        &self,
        can_reuse_recent_snapshot: bool,
    ) -> ServiceResult<AiUsageSnapshot>;
    fn open_ai_account_login(
        &self,
        window: Arc<dyn LoginWindow>,
        provider: AiUsageProvider,
        account_root: &str,
        columns: u16,
        rows: u16,
    ) -> AiAccountLoginOpenResult;
    fn write_ai_account_login(&self, window_id: u64, account_root: &str, data: &str);
    fn close_ai_account_login(&self, window_id: u64, account_root: &str);
}

struct AiAccountsBackendHandle(Arc<dyn AiAccountsBackend>);

impl Global for AiAccountsBackendHandle {}

/// 앱이 계정 서비스를 직접 만들게 되면 부른다
pub fn install_ai_accounts_backend(backend: Arc<dyn AiAccountsBackend>, cx: &mut App) {
    cx.set_global(AiAccountsBackendHandle(backend));
}

/// 처음 부를 때 서비스를 만든다. DB가 없으면 모든 조회가 실패하는 통로를 건다
pub fn ai_accounts_backend(cx: &mut App) -> Arc<dyn AiAccountsBackend> {
    if !cx.has_global::<AiAccountsBackendHandle>() {
        let backend: Arc<dyn AiAccountsBackend> = match build_kanvibe_backend(cx) {
            Some(backend) => Arc::new(backend),
            None => Arc::new(UnavailableBackend),
        };
        cx.set_global(AiAccountsBackendHandle(backend));
    }
    cx.global::<AiAccountsBackendHandle>().0.clone()
}

/// `kanvibe_ai` 서비스로 만든 실제 통로
pub struct KanvibeAiAccountsBackend {
    accounts: AiAccountService,
    usage: AiUsageService,
    login: DesktopLoginBridge,
}

fn build_kanvibe_backend(cx: &App) -> Option<KanvibeAiAccountsBackend> {
    let services = cx.global::<AppServices>();
    let database = services.database()?.clone();
    let network_responses = cx
        .try_global::<QaRuntime>()
        .map(|runtime| runtime.network_responses.clone());
    let context = build_usage_context(services.clock, network_responses);
    let warn = context.warn.clone();
    let spawner = Arc::new(PtyLoginTerminalSpawner::new(Arc::new(NativePtySpawner)));
    Some(KanvibeAiAccountsBackend {
        accounts: AiAccountService::new(
            database.clone(),
            context.clone(),
            Arc::new(context.clone()),
        ),
        usage: AiUsageService::new(
            database,
            Arc::new(context.clone()),
            context.clock.clone(),
            warn.clone(),
        ),
        login: DesktopLoginBridge::new(context, spawner, warn),
    })
}

/// 사용량 조회·계정 관리 문맥. TS main처럼 시작 시점 환경 값을 읽기만 하고 바꾸지 않는다.
/// QA 모드에서는 바깥 HTTP를 고정 응답 표로만 답하고 시각은 앱 시계를 따른다
fn build_usage_context(
    clock: AppClock,
    network_responses: Option<NetworkResponseTable>,
) -> UsageContext {
    let process_variables: BTreeMap<String, String> = std::env::vars().collect();
    let variable = |name: &str| process_variables.get(name).cloned().unwrap_or_default();
    let http: Arc<dyn HttpTransport> = match network_responses {
        Some(table) => Arc::new(FixedResponseTransport { table }),
        None => Arc::new(UreqHttpTransport::default()),
    };
    UsageContext {
        home_directory: variable("HOME"),
        user_name: variable("USER"),
        platform: current_usage_platform(),
        system_binary_directories: vec![
            "/usr/local/bin".to_string(),
            "/opt/homebrew/bin".to_string(),
        ],
        process_variables: Arc::new(process_variables),
        http,
        processes: Arc::new(SystemProcessLauncher),
        clock: Arc::new(AppUsageClock(clock)),
        warn: Arc::new(|message: &str| eprintln!("{message}")),
        claude_retry_allowed_at_ms: Arc::new(Mutex::new(0.0)),
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

/// 앱 시계(QA 고정 시각 포함)를 사용량 서비스에 넘긴다
struct AppUsageClock(AppClock);

impl UsageClock for AppUsageClock {
    fn now_ms(&self) -> f64 {
        self.0.now_ms() as f64
    }
}

/// QA 고정 응답 표. 표에 없는 주소는 즉시 실패한다
struct FixedResponseTransport {
    table: NetworkResponseTable,
}

impl HttpTransport for FixedResponseTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let response = self
            .table
            .responses
            .iter()
            .find(|response| request.url.starts_with(&response.url_prefix))
            .ok_or_else(|| format!("QA 고정 응답에 없는 주소입니다: {}", request.url))?;
        let body = match &response.body {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        Ok(HttpResponse {
            status: response.status,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body,
        })
    }
}

impl AiAccountsBackend for KanvibeAiAccountsBackend {
    fn list_ai_accounts(&self) -> ServiceResult<Vec<AiAccountSummary>> {
        self.accounts.list_ai_accounts().map_err(failure)
    }

    fn add_ai_account(
        &self,
        provider: AiUsageProvider,
        name: &str,
    ) -> ServiceResult<AiAccountMutationResult> {
        self.accounts
            .add_ai_account(provider, name)
            .map_err(failure)
    }

    fn remove_ai_account(
        &self,
        provider: AiUsageProvider,
        account_root: &str,
    ) -> ServiceResult<AiAccountMutationResult> {
        self.accounts
            .remove_ai_account(provider, account_root)
            .map_err(failure)
    }

    fn get_cached_ai_usage_snapshot(&self) -> ServiceResult<Option<AiUsageSnapshot>> {
        self.usage.get_cached_ai_usage_snapshot().map_err(failure)
    }

    fn get_ai_usage_snapshot(
        &self,
        can_reuse_recent_snapshot: bool,
    ) -> ServiceResult<AiUsageSnapshot> {
        self.usage
            .get_ai_usage_snapshot(can_reuse_recent_snapshot)
            .map(|snapshot| (*snapshot).clone())
            .map_err(failure)
    }

    fn open_ai_account_login(
        &self,
        window: Arc<dyn LoginWindow>,
        provider: AiUsageProvider,
        account_root: &str,
        columns: u16,
        rows: u16,
    ) -> AiAccountLoginOpenResult {
        self.login
            .open_ai_account_login(window, provider, account_root, columns, rows)
    }

    fn write_ai_account_login(&self, window_id: u64, account_root: &str, data: &str) {
        self.login
            .write_ai_account_login(window_id, account_root, data);
    }

    fn close_ai_account_login(&self, window_id: u64, account_root: &str) {
        self.login.close_ai_account_login(window_id, account_root);
    }
}

/// 데이터 디렉터리 없이 뜬 앱. 모든 조회가 사유와 함께 실패한다
struct UnavailableBackend;

const BACKEND_UNAVAILABLE: &str = "데이터베이스가 열려 있지 않습니다";

impl AiAccountsBackend for UnavailableBackend {
    fn list_ai_accounts(&self) -> ServiceResult<Vec<AiAccountSummary>> {
        Err(failure(BACKEND_UNAVAILABLE))
    }

    fn add_ai_account(
        &self,
        _: AiUsageProvider,
        _: &str,
    ) -> ServiceResult<AiAccountMutationResult> {
        Err(failure(BACKEND_UNAVAILABLE))
    }

    fn remove_ai_account(
        &self,
        _: AiUsageProvider,
        _: &str,
    ) -> ServiceResult<AiAccountMutationResult> {
        Err(failure(BACKEND_UNAVAILABLE))
    }

    fn get_cached_ai_usage_snapshot(&self) -> ServiceResult<Option<AiUsageSnapshot>> {
        Ok(None)
    }

    fn get_ai_usage_snapshot(&self, _: bool) -> ServiceResult<AiUsageSnapshot> {
        Err(failure(BACKEND_UNAVAILABLE))
    }

    fn open_ai_account_login(
        &self,
        _: Arc<dyn LoginWindow>,
        _: AiUsageProvider,
        _: &str,
        _: u16,
        _: u16,
    ) -> AiAccountLoginOpenResult {
        AiAccountLoginOpenResult {
            ok: false,
            error: Some(BACKEND_UNAVAILABLE.to_string()),
        }
    }

    fn write_ai_account_login(&self, _: u64, _: &str, _: &str) {}

    fn close_ai_account_login(&self, _: u64, _: &str) {}
}
