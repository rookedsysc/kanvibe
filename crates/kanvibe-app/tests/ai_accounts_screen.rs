//! AI 계정 화면(TS `AiAccountsRoute`)과 내장 로그인 터미널(TS `AiAccountLoginTerminal`).
//! 계정 서비스 대신 가짜 통로를 걸고, 화면 상태·보낸 요청·클립보드를 본다.

use std::sync::{Arc, Mutex};

use gpui_kit::gpui;
use gpui_kit::{Entity, TestAppContext, VisualTestContext, WeakEntity};
use kanvibe_ai::accounts::account_service::{
    AiAccountMutationOutcome, AiAccountMutationResult, AiAccountSummary,
};
use kanvibe_ai::accounts::desktop_login_bridge::{AiAccountLoginOpenResult, LoginWindow};
use kanvibe_ai::usage::types::{
    AiUsageAccountResult, AiUsageFailureReason, AiUsageProvider, AiUsageSnapshot, AiUsageStatus,
};
use kanvibe_app::qa::faults::ServiceFault;
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::routes::{RouteResolution, resolve_route_path};
use kanvibe_app::services::{AppServices, ServiceFailure, ServiceResult};
use kanvibe_app::views::ai_accounts::AiAccountsView;
use kanvibe_app::views::ai_accounts::backend::{AiAccountsBackend, install_ai_accounts_backend};
use kanvibe_app::views::{ScreenContext, WindowNavigator};
use serde_json::json;

#[derive(Default)]
struct BackendState {
    accounts: Vec<AiAccountSummary>,
    usage: Option<AiUsageSnapshot>,
    is_list_failing: bool,
    is_usage_failing: bool,
    is_remove_failing: bool,
    is_login_refused: bool,
    list_calls: usize,
    usage_calls: usize,
    open_calls: usize,
    close_calls: usize,
    writes: Vec<String>,
    login_window: Option<Arc<dyn LoginWindow>>,
}

#[derive(Default)]
struct FakeBackend {
    state: Mutex<BackendState>,
}

fn failed(reason: &str) -> ServiceFailure {
    ServiceFailure {
        reason: reason.into(),
    }
}

impl AiAccountsBackend for FakeBackend {
    fn list_ai_accounts(&self) -> ServiceResult<Vec<AiAccountSummary>> {
        let mut state = self.state.lock().unwrap();
        state.list_calls += 1;
        if state.is_list_failing {
            return Err(failed("database is locked"));
        }
        Ok(state.accounts.clone())
    }

    fn add_ai_account(
        &self,
        provider: AiUsageProvider,
        name: &str,
    ) -> ServiceResult<AiAccountMutationResult> {
        let root = format!("/home/user/.{}-{name}", provider.id());
        self.state
            .lock()
            .unwrap()
            .accounts
            .push(account(provider, name, &root, &root, false));
        Ok(AiAccountMutationResult {
            outcome: AiAccountMutationOutcome::Ok,
            account_root: Some(root),
        })
    }

    fn remove_ai_account(
        &self,
        _: AiUsageProvider,
        account_root: &str,
    ) -> ServiceResult<AiAccountMutationResult> {
        let mut state = self.state.lock().unwrap();
        if state.is_remove_failing {
            return Err(failed("EBUSY"));
        }
        state
            .accounts
            .retain(|account| account.account_root != account_root);
        Ok(AiAccountMutationResult {
            outcome: AiAccountMutationOutcome::Ok,
            account_root: None,
        })
    }

    fn get_cached_ai_usage_snapshot(&self) -> ServiceResult<Option<AiUsageSnapshot>> {
        Ok(None)
    }

    fn get_ai_usage_snapshot(&self, _: bool) -> ServiceResult<AiUsageSnapshot> {
        let mut state = self.state.lock().unwrap();
        state.usage_calls += 1;
        if state.is_usage_failing {
            return Err(failed("network down"));
        }
        Ok(state.usage.clone().unwrap_or(AiUsageSnapshot {
            accounts: vec![],
            fetched_at: "2026-01-15T03:00:00.000Z".into(),
        }))
    }

    fn open_ai_account_login(
        &self,
        window: Arc<dyn LoginWindow>,
        _: AiUsageProvider,
        _: &str,
        _: u16,
        _: u16,
    ) -> AiAccountLoginOpenResult {
        let mut state = self.state.lock().unwrap();
        state.open_calls += 1;
        if state.is_login_refused {
            return AiAccountLoginOpenResult {
                ok: false,
                error: Some("pty unavailable".into()),
            };
        }
        state.login_window = Some(window);
        AiAccountLoginOpenResult {
            ok: true,
            error: None,
        }
    }

    fn write_ai_account_login(&self, _: u64, _: &str, data: &str) {
        self.state.lock().unwrap().writes.push(data.to_string());
    }

    fn close_ai_account_login(&self, _: u64, _: &str) {
        self.state.lock().unwrap().close_calls += 1;
    }
}

impl FakeBackend {
    fn login_window(&self) -> Arc<dyn LoginWindow> {
        self.state
            .lock()
            .unwrap()
            .login_window
            .clone()
            .expect("login opened")
    }
}

fn account(
    provider: AiUsageProvider,
    label: &str,
    id: &str,
    root: &str,
    is_logged_in: bool,
) -> AiAccountSummary {
    AiAccountSummary {
        provider,
        account_id: id.into(),
        cached_account_id: format!("cached-{id}"),
        account_root: root.into(),
        account_name: Some(label.into()),
        label: label.into(),
        is_logged_in,
        plan_name: None,
        is_removable: true,
    }
}

fn usage_result(account_id: &str, reason: Option<AiUsageFailureReason>) -> AiUsageAccountResult {
    AiUsageAccountResult {
        provider: AiUsageProvider::Claude,
        account_id: account_id.into(),
        label: "work".into(),
        status: AiUsageStatus::Ok,
        plan_name: Some("max".into()),
        windows: vec![],
        reason,
        fetched_at: "2026-01-15T03:00:00.000Z".into(),
    }
}

fn init_app(cx: &mut TestAppContext, backend: Arc<FakeBackend>) {
    let launch = LaunchOptions {
        data_dir: None,
        initial_route: "/ko/ai-accounts".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: Some(QaOptions {
            fixture_root: None,
            fixed_now_ms: 1_768_446_000_000,
            network_responses_path: None,
            faults_path: None,
            control_socket_path: None,
            platform_override: None,
        }),
    };
    cx.update(|cx| {
        kanvibe_app::init_app(&launch, cx).expect("app init");
        install_ai_accounts_backend(backend, cx);
    });
}

fn open_accounts(cx: &mut TestAppContext) -> (Entity<AiAccountsView>, &mut VisualTestContext) {
    let RouteResolution::Render(route) = resolve_route_path("/ko/ai-accounts") else {
        panic!("route");
    };
    let screen = ScreenContext {
        route,
        navigator: WindowNavigator::new(WeakEntity::new_invalid()),
    };
    let (view, cx) = cx.add_window_view(|_, cx| AiAccountsView::new(screen, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx)
}

fn add_fault(cx: &mut VisualTestContext, fault: serde_json::Value) {
    let fault: ServiceFault = serde_json::from_value(fault).expect("fault");
    cx.update(|_, cx| cx.global::<AppServices>().faults().expect("QA").add(fault));
}

// parity: TS-renderer-AiAccountsRoute-ac5df1
#[gpui::test]
fn expired_account_logs_in_from_its_card_and_refreshes_afterwards(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    {
        let mut state = backend.state.lock().unwrap();
        state.accounts = vec![account(
            AiUsageProvider::Claude,
            "work",
            "acc-1",
            "/root/work",
            true,
        )];
        state.usage = Some(AiUsageSnapshot {
            accounts: vec![usage_result(
                "acc-1",
                Some(AiUsageFailureReason::ExpiredCredentials),
            )],
            fetched_at: "2026-01-15T03:00:00.000Z".into(),
        });
    }
    init_app(cx, backend.clone());
    let (view, cx) = open_accounts(cx);
    let expired = backend.state.lock().unwrap().accounts[0].clone();
    assert!(view.read_with(cx, |view, _| view.model.requires_login(&expired)));
    view.update_in(cx, |view, window, cx| {
        view.open_login(AiUsageProvider::Claude, "/root/work".into(), window, cx)
    });
    cx.run_until_parked();
    let (lists_before, usage_before) = {
        let state = backend.state.lock().unwrap();
        (state.list_calls, state.usage_calls)
    };
    backend.login_window().send_exit("/root/work", 0);
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.model.active_login.is_none()));
    let state = backend.state.lock().unwrap();
    assert_eq!(state.list_calls, lists_before + 1);
    assert_eq!(state.usage_calls, usage_before + 1);
}

// parity: TS-renderer-AiAccountsRoute-7b1fc0
#[gpui::test]
fn account_waiting_for_a_claude_run_counts_as_logged_in(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    {
        let mut state = backend.state.lock().unwrap();
        state.accounts = vec![account(
            AiUsageProvider::Claude,
            "work",
            "acc-1",
            "/root/work",
            true,
        )];
        state.usage = Some(AiUsageSnapshot {
            accounts: vec![usage_result(
                "acc-1",
                Some(AiUsageFailureReason::AwaitingCliRefresh),
            )],
            fetched_at: "2026-01-15T03:00:00.000Z".into(),
        });
    }
    init_app(cx, backend.clone());
    let (view, cx) = open_accounts(cx);
    let waiting = backend.state.lock().unwrap().accounts[0].clone();
    assert!(!view.read_with(cx, |view, _| view.model.requires_login(&waiting)));
}

// parity: TS-renderer-AiAccountsRoute-e96cb5
#[gpui::test]
fn failed_login_keeps_the_output_and_can_be_closed_and_retried(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    backend.state.lock().unwrap().accounts = vec![account(
        AiUsageProvider::Codex,
        "work",
        "acc-1",
        "/root/work",
        false,
    )];
    init_app(cx, backend.clone());
    let (view, cx) = open_accounts(cx);
    view.update_in(cx, |view, window, cx| {
        view.open_login(AiUsageProvider::Codex, "/root/work".into(), window, cx)
    });
    cx.run_until_parked();
    let login_window = backend.login_window();
    login_window.send_output("/root/work", "Error: failed to start the login server\r\n");
    login_window.send_exit("/root/work", 1);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.model.login_exit_code),
        Some(1)
    );
    let terminal = view
        .read_with(cx, |view, _| view.login_terminal())
        .expect("터미널을 남긴다");
    assert!(terminal.read_with(cx, |terminal, _| {
        terminal.visible_lines()[0].starts_with("Error: failed")
    }));
    view.update(cx, |view, cx| view.close_login(cx));
    view.update_in(cx, |view, window, cx| {
        view.open_login(AiUsageProvider::Codex, "/root/work".into(), window, cx)
    });
    cx.run_until_parked();
    assert_eq!(backend.state.lock().unwrap().open_calls, 2);
    assert_eq!(
        view.read_with(cx, |view, _| view.model.login_exit_code),
        None
    );
}

// parity: TS-renderer-AiAccountsRoute-c9de3b
#[gpui::test]
fn same_named_accounts_get_only_their_own_usage(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    {
        let mut state = backend.state.lock().unwrap();
        state.accounts = vec![
            account(AiUsageProvider::Claude, "work", "acc-1", "/root/a", true),
            account(AiUsageProvider::Claude, "work", "acc-2", "/root/b", true),
        ];
        state.usage = Some(AiUsageSnapshot {
            accounts: vec![usage_result("cached-acc-2", None)],
            fetched_at: "2026-01-15T03:00:00.000Z".into(),
        });
    }
    init_app(cx, backend.clone());
    let (view, cx) = open_accounts(cx);
    let accounts = backend.state.lock().unwrap().accounts.clone();
    view.read_with(cx, |view, _| {
        assert!(view.model.usage.usage_for(&accounts[0]).is_none());
        assert_eq!(
            view.model
                .usage
                .usage_for(&accounts[1])
                .map(|usage| usage.account_id.as_str()),
            Some("cached-acc-2")
        );
    });
}

// parity: TS-renderer-AiAccountsRoute-de6f29
#[gpui::test]
fn usage_failure_still_allows_adding_and_logging_in(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    backend.state.lock().unwrap().is_usage_failing = true;
    init_app(cx, backend.clone());
    let (view, cx) = open_accounts(cx);
    assert!(view.read_with(cx, |view, _| view.model.usage.has_failed));
    view.update_in(cx, |view, window, cx| {
        view.submit_account_name(AiUsageProvider::Gemini, "work".into(), None, window, cx)
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.model.active_login.is_some()));
    assert_eq!(backend.state.lock().unwrap().open_calls, 1);
}

// parity: TS-renderer-AiAccountsRoute-65a19b
#[gpui::test]
fn first_list_failure_shows_the_reason_and_retry_rereads(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    backend.state.lock().unwrap().is_list_failing = true;
    init_app(cx, backend.clone());
    let (view, cx) = open_accounts(cx);
    view.read_with(cx, |view, _| {
        assert!(view.model.accounts.is_none(), "계정 없음으로 보이지 않는다");
        assert_eq!(
            view.model.load_failure.as_deref(),
            Some("database is locked")
        );
    });
    backend.state.lock().unwrap().is_list_failing = false;
    view.update(cx, |view, cx| view.load_accounts(cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.model.accounts.is_some()));
}

// parity: TS-renderer-AiAccountsRoute-c793c0
#[gpui::test]
fn removal_shows_progress_blocks_others_and_leaves_failure_on_the_row(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    {
        let mut state = backend.state.lock().unwrap();
        state.accounts = vec![
            account(AiUsageProvider::Claude, "a", "acc-1", "/root/a", true),
            account(AiUsageProvider::Claude, "b", "acc-2", "/root/b", true),
        ];
        state.is_remove_failing = true;
    }
    init_app(cx, backend.clone());
    let (view, cx) = open_accounts(cx);
    let accounts = backend.state.lock().unwrap().accounts.clone();
    add_fault(
        cx,
        json!({"target": "aiAccounts.removeAiAccount", "mode": "delay", "ms": 100}),
    );
    view.update(cx, |view, cx| {
        view.remove_account(accounts[0].clone(), cx);
        view.remove_account(accounts[1].clone(), cx);
    });
    assert_eq!(
        view.read_with(cx, |view, _| view.model.removing_root.clone())
            .as_deref(),
        Some("/root/a")
    );
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.model.removal_failure_for(&accounts[0]), Some("EBUSY"));
        assert_eq!(view.model.removal_failure_for(&accounts[1]), None);
        assert!(view.model.removing_root.is_none());
    });
}

// parity: TS-renderer-AiAccountsRoute-3faf8d
#[gpui::test]
fn adding_blocks_repeat_submits_and_keeps_a_failure_reason(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    init_app(cx, backend.clone());
    let (view, cx) = open_accounts(cx);
    add_fault(
        cx,
        json!({"target": "aiAccounts.addAiAccount", "mode": "fail", "ms": 100, "message": "EACCES: permission denied"}),
    );
    view.update_in(cx, |view, window, cx| {
        view.submit_account_name(AiUsageProvider::Claude, "work".into(), None, window, cx);
        view.submit_account_name(AiUsageProvider::Claude, "work".into(), None, window, cx);
    });
    assert!(view.read_with(cx, |view, _| view.model.add_states[0].is_pending));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let state = &view.model.add_states[0];
        assert!(!state.is_pending);
        assert_eq!(
            state.failure.as_ref().map(|(_, reason)| reason.as_str()),
            Some("EACCES: permission denied")
        );
    });
    assert!(
        backend.state.lock().unwrap().accounts.is_empty(),
        "두 번 만들지 않는다"
    );
}

fn open_login_terminal(
    cx: &mut TestAppContext,
    backend: Arc<FakeBackend>,
) -> (Entity<AiAccountsView>, &mut VisualTestContext) {
    init_app(cx, backend);
    let (view, cx) = open_accounts(cx);
    view.update_in(cx, |view, window, cx| {
        view.open_login(AiUsageProvider::Claude, "/root/work".into(), window, cx)
    });
    cx.run_until_parked();
    (view, cx)
}

// parity: TS-renderer-AiAccountLoginTerminal-258162
#[gpui::test]
fn exited_login_no_longer_takes_input(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    let (view, cx) = open_login_terminal(cx, backend.clone());
    backend.login_window().send_exit("/root/work", 1);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        view.read_with(cx, |view, _| view.login_terminal())
            .is_some()
    );
    cx.simulate_keystrokes("a enter");
    assert!(backend.state.lock().unwrap().writes.is_empty());
}

// parity: TS-renderer-AiAccountLoginTerminal-71163a
#[gpui::test]
fn osc52_copy_in_the_login_terminal_reaches_the_clipboard(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    let (_view, cx) = open_login_terminal(cx, backend.clone());
    backend
        .login_window()
        .send_output("/root/work", "\u{1b}]52;c;Q09ERS0xMjM0\u{7}");
    cx.run_until_parked();
    let copied = cx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("CODE-1234"));
}

// parity: TS-renderer-AiAccountLoginTerminal-abc982
#[gpui::test]
fn closed_login_terminal_stops_handling_osc52(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    let (view, cx) = open_login_terminal(cx, backend.clone());
    let login_window = backend.login_window();
    view.update(cx, |view, cx| view.close_login(cx));
    cx.run_until_parked();
    login_window.send_output("/root/work", "\u{1b}]52;c;Q09ERS0xMjM0\u{7}");
    cx.run_until_parked();
    assert!(login_window.is_destroyed());
    let copied = cx
        .update(|_, cx| cx.read_from_clipboard())
        .and_then(|item| item.text());
    assert_eq!(copied, None);
}

// parity: TS-renderer-AiAccountLoginTerminal-d76947
#[gpui::test]
fn unmounting_closes_the_pending_login(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    let (view, cx) = open_login_terminal(cx, backend.clone());
    view.update(cx, |view, cx| view.close_login(cx));
    cx.run_until_parked();
    assert_eq!(backend.state.lock().unwrap().close_calls, 1);
}

// parity: TS-renderer-AiAccountLoginTerminal-1f51ab
#[gpui::test]
fn refused_login_shows_the_reason_and_leaves_no_session(cx: &mut TestAppContext) {
    let backend = Arc::new(FakeBackend::default());
    backend.state.lock().unwrap().is_login_refused = true;
    let (view, cx) = open_login_terminal(cx, backend.clone());
    let terminal = view.read_with(cx, |view, _| view.login_terminal()).unwrap();
    assert!(terminal.read_with(cx, |terminal, _| {
        terminal.visible_lines()[0].starts_with("pty unavailable")
    }));
    assert!(backend.state.lock().unwrap().login_window.is_none());
    drop(terminal);
    view.update(cx, |view, cx| view.close_login(cx));
    cx.run_until_parked();
    assert_eq!(backend.state.lock().unwrap().close_calls, 1);
}
