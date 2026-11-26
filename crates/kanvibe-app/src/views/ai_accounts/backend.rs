//! AI 계정 화면이 부르는 서비스(`kanvibe_ai`의 계정·사용량 서비스와 로그인 브리지).
//!
//! 이 서비스들은 앱 수명 동안 하나만 있어야 한다(사용량 조회 합류·로그인 PTY 세션 등록부). 그래서 화면은 만들지 않고
//! 런타임(`DesktopRuntime`)이 가진 인스턴스를 `AiAccountsBackend` 뒤에서 부른다. 테스트는 같은 자리에 가짜 통로를 넘긴다.

use std::sync::Arc;

use gpui_kit::App;
use kanvibe_ai::accounts::account_service::{AiAccountMutationResult, AiAccountSummary};
use kanvibe_ai::accounts::desktop_login_bridge::{AiAccountLoginOpenResult, LoginWindow};
use kanvibe_ai::usage::types::{AiUsageProvider, AiUsageSnapshot};
use kanvibe_runtime::DesktopRuntime;

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
    fn resize_ai_account_login(&self, window_id: u64, account_root: &str, columns: u16, rows: u16);
    fn close_ai_account_login(&self, window_id: u64, account_root: &str);
}

/// 런타임의 계정·사용량 서비스와 로그인 브리지. 데이터 디렉터리 없이 뜬 실행이면 모든 조회가 실패하는 통로다
pub fn runtime_backend(cx: &App) -> Arc<dyn AiAccountsBackend> {
    match cx.global::<AppServices>().runtime() {
        Some(runtime) => Arc::new(RuntimeAiAccountsBackend(runtime.clone())),
        None => Arc::new(UnavailableBackend),
    }
}

/// 앱 프로세스에 하나인 런타임 서비스로 답하는 통로. QA 고정 응답과 고정 시각은 런타임이 이미 쓴다
struct RuntimeAiAccountsBackend(Arc<DesktopRuntime>);

impl AiAccountsBackend for RuntimeAiAccountsBackend {
    fn list_ai_accounts(&self) -> ServiceResult<Vec<AiAccountSummary>> {
        self.0
            .ai_account_service()
            .list_ai_accounts()
            .map_err(failure)
    }

    fn add_ai_account(
        &self,
        provider: AiUsageProvider,
        name: &str,
    ) -> ServiceResult<AiAccountMutationResult> {
        self.0
            .ai_account_service()
            .add_ai_account(provider, name)
            .map_err(failure)
    }

    fn remove_ai_account(
        &self,
        provider: AiUsageProvider,
        account_root: &str,
    ) -> ServiceResult<AiAccountMutationResult> {
        self.0
            .ai_account_service()
            .remove_ai_account(provider, account_root)
            .map_err(failure)
    }

    fn get_cached_ai_usage_snapshot(&self) -> ServiceResult<Option<AiUsageSnapshot>> {
        self.0
            .ai_usage_service()
            .get_cached_ai_usage_snapshot()
            .map_err(failure)
    }

    fn get_ai_usage_snapshot(
        &self,
        can_reuse_recent_snapshot: bool,
    ) -> ServiceResult<AiUsageSnapshot> {
        self.0
            .ai_usage_service()
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
        self.0.ai_desktop_login_bridge().open_ai_account_login(
            window,
            provider,
            account_root,
            columns,
            rows,
        )
    }

    fn write_ai_account_login(&self, window_id: u64, account_root: &str, data: &str) {
        self.0
            .ai_desktop_login_bridge()
            .write_ai_account_login(window_id, account_root, data);
    }

    fn resize_ai_account_login(&self, window_id: u64, account_root: &str, columns: u16, rows: u16) {
        self.0.ai_desktop_login_bridge().resize_ai_account_login(
            window_id,
            account_root,
            columns,
            rows,
        );
    }

    fn close_ai_account_login(&self, window_id: u64, account_root: &str) {
        self.0
            .ai_desktop_login_bridge()
            .close_ai_account_login(window_id, account_root);
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

    fn resize_ai_account_login(&self, _: u64, _: &str, _: u16, _: u16) {}

    fn close_ai_account_login(&self, _: u64, _: &str) {}
}
