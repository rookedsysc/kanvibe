use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use kanvibe_ai::accounts::account_service::{
    AiAccountMutationOutcome, AiAccountMutationResult, AiAccountSummary,
};
use kanvibe_ai::accounts::mobile_login_bridge::{
    MobileAiLoginError, MobileAiLoginEvent, MobileLoginOpenRequest, MobileLoginSocket,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::RunningAgentPane;
use kanvibe_ai::usage::types::{
    AiUsageAccountResult, AiUsageProvider, AiUsageSnapshot, AiUsageStatus, AiUsageWindow,
    AiUsageWindowKind,
};
use serde_json::{Value, json};

use super::*;
use crate::test_support::{
    CallLog, FakeProjectCatalog, FakeTaskBoard, lock, task, wait_until, with_project,
};

const ACCOUNT_ROOT: &str = "/fixture/.claude-work";

fn account(is_removable: bool) -> AiAccountSummary {
    AiAccountSummary {
        provider: AiUsageProvider::Claude,
        account_id: "provider-uuid".to_string(),
        cached_account_id: "cached".to_string(),
        account_root: ACCOUNT_ROOT.to_string(),
        account_name: Some("work".to_string()),
        label: "work@example.test".to_string(),
        is_logged_in: true,
        plan_name: Some("pro".to_string()),
        is_removable,
    }
}

fn outcome(
    outcome: AiAccountMutationOutcome,
    account_root: Option<&str>,
) -> AiAccountMutationResult {
    serde_json::from_value(json!({ "outcome": outcome, "accountRoot": account_root })).unwrap()
}

/// 계정 목록과 삭제 결과를 정해 두는 가짜. 삭제에 문이 있으면 열릴 때까지 멈춘다
struct FakeAccounts {
    calls: CallLog,
    accounts: Mutex<Vec<AiAccountSummary>>,
    add_result: Mutex<AiAccountMutationResult>,
    remove_gate: Mutex<Option<Receiver<()>>>,
}

impl Default for FakeAccounts {
    fn default() -> Self {
        Self {
            calls: CallLog::default(),
            accounts: Mutex::new(vec![account(true)]),
            add_result: Mutex::new(outcome(AiAccountMutationOutcome::Ok, Some(ACCOUNT_ROOT))),
            remove_gate: Mutex::new(None),
        }
    }
}

impl AiAccountDirectory for FakeAccounts {
    fn list_ai_accounts(&self) -> Result<Vec<AiAccountSummary>, String> {
        Ok(lock(&self.accounts).clone())
    }

    fn add_ai_account(
        &self,
        provider: AiUsageProvider,
        account_name: &str,
    ) -> Result<AiAccountMutationResult, String> {
        self.calls
            .record(format!("add {} {account_name}", provider.id()));
        Ok(lock(&self.add_result).clone())
    }

    fn remove_ai_account(
        &self,
        provider: AiUsageProvider,
        account_root: &str,
    ) -> Result<AiAccountMutationResult, String> {
        self.calls
            .record(format!("remove {} {account_root}", provider.id()));
        if let Some(gate) = lock(&self.remove_gate).take() {
            let _ = gate.recv();
            lock(&self.accounts).clear();
        }
        Ok(outcome(AiAccountMutationOutcome::Ok, None))
    }
}

struct FakeUsage {
    snapshot: Mutex<Option<AiUsageSnapshot>>,
}

impl AiUsageSource for FakeUsage {
    fn get_ai_usage_snapshot(&self) -> Result<AiUsageSnapshot, String> {
        lock(&self.snapshot)
            .clone()
            .ok_or_else(|| "없음".to_string())
    }
}

#[derive(Default)]
struct FakeLogins {
    calls: CallLog,
}

impl AiLoginSessions for FakeLogins {
    fn open(&self, request: MobileLoginOpenRequest) -> Result<(), MobileAiLoginError> {
        self.calls.record(format!(
            "open {} {} {} {} {}x{}",
            request.device_id,
            request.socket_id,
            request.provider.id(),
            request.account_root,
            request.columns,
            request.rows
        ));
        Ok(())
    }

    fn write(&self, _: &str, _: &str, _: &str) -> Result<(), MobileAiLoginError> {
        Ok(())
    }

    fn resize(&self, _: &str, _: &str, _: f64, _: f64) -> Result<(), MobileAiLoginError> {
        Ok(())
    }

    fn close(&self, _: &str, _: &str) {}

    fn close_for_device(&self, _: &str) {}

    fn close_for_account(&self, account_root: &str) {
        self.calls
            .record(format!("close_for_account {account_root}"));
    }

    fn close_all(&self) {}
}

struct FakeSocket {
    is_open: bool,
}

impl MobileLoginSocket for FakeSocket {
    fn is_open(&self) -> bool {
        self.is_open
    }

    fn send_event(&self, _: MobileAiLoginEvent) {}
}

struct AiFixture {
    ai: Arc<MobileAi>,
    tasks: Arc<FakeTaskBoard>,
    projects: Arc<FakeProjectCatalog>,
    accounts: Arc<FakeAccounts>,
    usage: Arc<FakeUsage>,
    logins: Arc<FakeLogins>,
}

fn fixture() -> AiFixture {
    let tasks = Arc::new(FakeTaskBoard::default());
    tasks.put_task(with_project(task("task-1"), None));
    let projects = Arc::new(FakeProjectCatalog::default());
    let accounts = Arc::new(FakeAccounts::default());
    let usage = Arc::new(FakeUsage {
        snapshot: Mutex::new(None),
    });
    let logins = Arc::new(FakeLogins::default());
    let ai = Arc::new(MobileAi::new(MobileAiParts {
        tasks: tasks.clone(),
        projects: projects.clone(),
        accounts: accounts.clone(),
        usage: usage.clone(),
        logins: logins.clone(),
    }));
    AiFixture {
        ai,
        tasks,
        projects,
        accounts,
        usage,
        logins,
    }
}

fn first_account_id(fixture: &AiFixture) -> String {
    fixture.ai.list_mobile_ai_accounts().unwrap()[0]
        .account_id
        .clone()
}

fn is_hex_handle(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn open_socket(is_open: bool) -> Arc<dyn MobileLoginSocket> {
    Arc::new(FakeSocket { is_open })
}

#[test]
fn account_list_carries_opaque_handles_only() {
    let fixture = fixture();

    let accounts = serde_json::to_value(fixture.ai.list_mobile_ai_accounts().unwrap()).unwrap();

    let account_id = accounts[0]["accountId"].as_str().unwrap().to_string();
    assert!(is_hex_handle(&account_id));
    assert_eq!(
        accounts,
        json!([{ "accountId": account_id, "provider": "claude", "accountName": "work", "label": "work@example.test", "isLoggedIn": true, "planName": "pro", "isRemovable": true }])
    );
    assert!(!accounts.to_string().contains(ACCOUNT_ROOT));
    assert!(!accounts.to_string().contains("provider-uuid"));
    assert_eq!(
        serde_json::to_value(fixture.ai.list_mobile_ai_accounts().unwrap()).unwrap(),
        accounts
    );
}

#[test]
fn creation_returns_the_handle_of_the_new_account() {
    let fixture = fixture();

    let created = fixture
        .ai
        .add_mobile_ai_account(&json!({ "provider": "claude", "accountName": "work" }))
        .unwrap();

    assert_eq!(fixture.accounts.calls.all(), vec!["add claude work"]);
    assert_eq!(created.account_id, first_account_id(&fixture));
}

#[test]
fn untrusted_account_creation_is_rejected_before_writes() {
    let fixture = fixture();
    let inputs = [
        Value::Null,
        json!([]),
        json!({ "provider": "unknown", "accountName": "work" }),
        json!({ "provider": "claude", "accountName": "../escape" }),
        json!({ "provider": "claude", "accountName": "work", "accountRoot": "/etc" }),
        json!({ "provider": "claude", "accountName": 5 }),
    ];
    for input in inputs {
        let error = fixture.ai.add_mobile_ai_account(&input).unwrap_err();
        assert_eq!(error.status_code(), Some(400), "{input}");
    }

    assert!(fixture.accounts.calls.all().is_empty());
}

#[test]
fn duplicate_account_is_a_conflict() {
    let fixture = fixture();
    *lock(&fixture.accounts.add_result) = outcome(AiAccountMutationOutcome::AlreadyExists, None);

    let error = fixture
        .ai
        .add_mobile_ai_account(&json!({ "provider": "claude", "accountName": "work" }))
        .unwrap_err();

    assert_eq!(error.status_code(), Some(409));
}

#[test]
fn deletion_handle_resolves_to_the_current_account_root() {
    let fixture = fixture();
    let account_id = first_account_id(&fixture);

    let removal = fixture.ai.remove_mobile_ai_account(&account_id).unwrap();

    assert_eq!(
        serde_json::to_value(removal).unwrap(),
        json!({ "outcome": "ok" })
    );
    assert_eq!(
        fixture.logins.calls.all(),
        vec![format!("close_for_account {ACCOUNT_ROOT}")]
    );
    assert_eq!(
        fixture.accounts.calls.all(),
        vec![format!("remove claude {ACCOUNT_ROOT}")]
    );
}

#[test]
fn path_injection_and_stale_handles_delete_nothing() {
    let fixture = fixture();

    let injected = fixture
        .ai
        .remove_mobile_ai_account(ACCOUNT_ROOT)
        .unwrap_err();
    let stale = fixture
        .ai
        .remove_mobile_ai_account(&"a".repeat(64))
        .unwrap_err();

    assert_eq!(
        (injected.status_code(), stale.status_code()),
        (Some(400), Some(404))
    );
    assert!(fixture.accounts.calls.all().is_empty());
}

#[test]
fn discovered_accounts_kanvibe_does_not_own_are_protected() {
    let fixture = fixture();
    *lock(&fixture.accounts.accounts) = vec![account(false)];
    let account_id = first_account_id(&fixture);

    let error = fixture
        .ai
        .remove_mobile_ai_account(&account_id)
        .unwrap_err();

    assert_eq!(error.status_code(), Some(403));
    assert!(fixture.accounts.calls.all().is_empty());
}

#[test]
fn login_uses_the_validated_server_account() {
    let fixture = fixture();
    let account_id = first_account_id(&fixture);

    fixture
        .ai
        .open_mobile_ai_login(
            "device-1",
            "socket-1",
            &account_id,
            (80, 24),
            open_socket(true),
        )
        .unwrap();

    assert_eq!(
        fixture.logins.calls.all(),
        vec![format!(
            "open device-1 socket-1 claude {ACCOUNT_ROOT} 80x24"
        )]
    );
}

#[test]
fn login_is_not_launched_after_the_socket_closed() {
    let fixture = fixture();
    let account_id = first_account_id(&fixture);

    fixture
        .ai
        .open_mobile_ai_login(
            "device-1",
            "socket-1",
            &account_id,
            (80, 24),
            open_socket(false),
        )
        .unwrap();

    assert!(fixture.logins.calls.all().is_empty());
}

#[test]
fn live_and_graph_readers_run_only_for_known_tasks_and_valid_providers() {
    let fixture = fixture();

    let live =
        serde_json::to_value(fixture.ai.get_mobile_live_ai_sessions("task-1").unwrap()).unwrap();
    assert_eq!(
        live,
        json!({ "taskId": "task-1", "isRemote": false, "sessions": [] })
    );
    assert_eq!(
        fixture
            .ai
            .get_mobile_agent_call_graph("task-1", "claude", "session-1")
            .unwrap(),
        None
    );
    assert_eq!(
        fixture
            .projects
            .calls
            .count_starting_with("get_task_agent_call_graph task-1 claude session-1"),
        1
    );
    let shell = fixture
        .ai
        .get_mobile_agent_call_graph("task-1", "shell", "session-1")
        .unwrap_err();
    assert_eq!(shell.status_code(), Some(400));
    lock(&fixture.tasks.tasks).clear();

    let missing = fixture
        .ai
        .get_mobile_live_ai_sessions("missing")
        .unwrap_err();

    assert_eq!(missing.status_code(), Some(404));
}

#[test]
fn graph_session_path_injection_is_rejected() {
    let fixture = fixture();
    for session_id in ["../other", "/etc/account", "folder\\session", ".", ".."] {
        let error = fixture
            .ai
            .get_mobile_agent_call_graph("task-1", "claude", session_id)
            .unwrap_err();
        assert_eq!(error.status_code(), Some(400), "{session_id}");
    }

    assert_eq!(
        fixture
            .projects
            .calls
            .count_starting_with("get_task_agent_call_graph"),
        0
    );
}

#[test]
fn deletion_and_login_startup_are_serialized() {
    let fixture = fixture();
    let account_id = first_account_id(&fixture);
    let (finish_removal, removal_gate) = channel::<()>();
    *lock(&fixture.accounts.remove_gate) = Some(removal_gate);

    let removing = {
        let ai = Arc::clone(&fixture.ai);
        let account_id = account_id.clone();
        std::thread::spawn(move || ai.remove_mobile_ai_account(&account_id))
    };
    wait_until("삭제 시작", || {
        fixture.accounts.calls.count_starting_with("remove") == 1
    });
    let opening = {
        let ai = Arc::clone(&fixture.ai);
        std::thread::spawn(move || {
            ai.open_mobile_ai_login(
                "device-1",
                "socket-1",
                &account_id,
                (80, 24),
                open_socket(true),
            )
        })
    };
    finish_removal.send(()).unwrap();

    removing.join().unwrap().unwrap();
    assert_eq!(
        opening.join().unwrap().unwrap_err().status_code(),
        Some(404)
    );
    assert!(
        fixture
            .logins
            .calls
            .all()
            .iter()
            .all(|call| !call.starts_with("open"))
    );
}

#[test]
fn usage_carries_hashed_identity_and_public_fields_only() {
    let fixture = fixture();
    *lock(&fixture.usage.snapshot) = Some(AiUsageSnapshot {
        fetched_at: "2026-11-13T00:00:00Z".to_string(),
        accounts: vec![AiUsageAccountResult {
            provider: AiUsageProvider::Claude,
            account_id: "/fixture/.claude-work".to_string(),
            label: "work".to_string(),
            status: AiUsageStatus::Ok,
            plan_name: Some("pro".to_string()),
            windows: vec![AiUsageWindow {
                kind: AiUsageWindowKind::Weekly,
                model_name: None,
                used_percent: 42.0,
                resets_at: None,
            }],
            reason: None,
            fetched_at: "2026-11-13T00:00:00Z".to_string(),
        }],
    });

    let usage = serde_json::to_value(fixture.ai.get_mobile_ai_usage_snapshot().unwrap()).unwrap();

    assert_eq!(
        usage["accounts"][0]["windows"],
        json!([{ "kind": "weekly", "modelName": null, "usedPercent": 42.0, "resetsAt": null }])
    );
    assert!(is_hex_handle(
        usage["accounts"][0]["accountId"].as_str().unwrap()
    ));
    assert!(!usage.to_string().contains("/fixture"));
}

#[test]
fn running_panes_are_the_shared_snapshot() {
    let fixture = fixture();
    let pane = RunningAgentPane {
        provider: AiSessionProvider::Codex,
        worktree_path: "/work".to_string(),
        session_name: "session".to_string(),
        window_id: "@1".to_string(),
        window_name: "codex".to_string(),
    };
    *lock(&fixture.projects.running_panes) = vec![pane.clone()];

    assert_eq!(fixture.ai.get_mobile_running_agent_panes(), vec![pane]);
}
