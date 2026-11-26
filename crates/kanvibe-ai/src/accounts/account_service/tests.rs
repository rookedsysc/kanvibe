use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::accounts::settings_store::memory::MemorySettings;
use crate::javascript::stringify;
use crate::test_support::{temp_home, write_file};
use crate::usage::test_context::TestUsage;

/// 정해 둔 로그인 상태로 답하고 로그아웃 요청을 기록하는 CLI. 로그아웃을 붙잡아 둘 수 있다
struct ScriptedCli {
    auth_status: Mutex<Option<ProviderAuthStatus>>,
    is_logout_successful: AtomicBool,
    logouts: Mutex<Vec<(AiUsageProvider, String)>>,
    logout_gate: Mutex<Option<mpsc::Receiver<()>>>,
    logout_started: AtomicUsize,
}

impl ScriptedCli {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            auth_status: Mutex::new(None),
            is_logout_successful: AtomicBool::new(true),
            logouts: Mutex::new(Vec::new()),
            logout_gate: Mutex::new(None),
            logout_started: AtomicUsize::new(0),
        })
    }
}

impl ProviderAccountCli for ScriptedCli {
    fn read_auth_status(&self, _: AiUsageProvider, _: &str) -> Option<ProviderAuthStatus> {
        self.auth_status.lock().unwrap().clone()
    }

    fn logout(&self, provider: AiUsageProvider, account_root: &str) -> bool {
        self.logouts
            .lock()
            .unwrap()
            .push((provider, account_root.to_string()));
        self.logout_started.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = self.logout_gate.lock().unwrap().take() {
            gate.recv().unwrap();
        }
        self.is_logout_successful.load(Ordering::SeqCst)
    }
}

struct AccountFixture {
    _directory: tempfile::TempDir,
    home: String,
    service: AiAccountService,
    store: Arc<MemorySettings>,
    cli: Arc<ScriptedCli>,
}

fn fixture() -> AccountFixture {
    let (directory, home) = temp_home();
    let usage = TestUsage::new(&home);
    let store = Arc::new(MemorySettings::default());
    let cli = ScriptedCli::new();
    let service = AiAccountService::new(store.clone(), usage.context.clone(), cli.clone());
    AccountFixture {
        _directory: directory,
        home,
        service,
        store,
        cli,
    }
}

impl AccountFixture {
    fn path(&self, relative: &str) -> String {
        format!("{}/{relative}", self.home)
    }

    fn registration_names(&self) -> Vec<String> {
        self.service
            .get_ai_account_registrations()
            .unwrap()
            .into_iter()
            .map(|registration| registration.account_name)
            .collect()
    }

    fn add(&self, provider: AiUsageProvider, name: &str) -> AiAccountMutationResult {
        self.service.add_ai_account(provider, name).unwrap()
    }

    fn summary_for(&self, account_root: &str) -> Option<AiAccountSummary> {
        self.service
            .list_ai_accounts()
            .unwrap()
            .into_iter()
            .find(|summary| summary.account_root == account_root)
    }

    fn write_codex_auth(&self, directory: &str) {
        write_file(
            &self.path(&format!("{directory}/auth.json")),
            &stringify(&json!({"tokens": {"access_token": "token"}})),
        );
    }
}

fn is_directory(path: &str) -> bool {
    Path::new(path).is_dir()
}

#[test]
fn add_creates_slot_and_registers() {
    let fixture = fixture();
    let result = fixture.add(AiUsageProvider::Claude, "work");
    assert_eq!(result.outcome, AiAccountMutationOutcome::Ok);
    assert_eq!(result.account_root, Some(fixture.path(".claude-work")));
    assert!(is_directory(&fixture.path(".claude-work")));
    assert_eq!(
        fixture.service.get_ai_account_registrations().unwrap(),
        [AiAccountRegistration {
            provider: AiUsageProvider::Claude,
            account_root: fixture.path(".claude-work"),
            account_name: "work".to_string(),
        }]
    );
}

#[test]
fn gemini_add_creates_dot_gemini_too() {
    let fixture = fixture();
    let result = fixture.add(AiUsageProvider::Gemini, "work");
    assert_eq!(result.account_root, Some(fixture.path(".gemini-work")));
    assert!(is_directory(&fixture.path(".gemini-work/.gemini")));
}

#[test]
fn escaping_names_are_rejected_without_side_effects() {
    let fixture = fixture();
    assert_eq!(
        fixture.add(AiUsageProvider::Claude, "../escape").outcome,
        AiAccountMutationOutcome::InvalidName
    );
    assert_eq!(
        fixture.add(AiUsageProvider::Claude, "with space").outcome,
        AiAccountMutationOutcome::InvalidName
    );
    assert!(
        fixture
            .service
            .get_ai_account_registrations()
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.store.write_count.load(Ordering::SeqCst), 0);
}

#[test]
fn adding_same_name_again_reports_existing() {
    let fixture = fixture();
    fixture.add(AiUsageProvider::Claude, "work");
    assert_eq!(
        fixture.add(AiUsageProvider::Claude, "work").outcome,
        AiAccountMutationOutcome::AlreadyExists
    );
}

#[test]
fn registered_account_stays_listed_while_signed_out() {
    let fixture = fixture();
    fixture.add(AiUsageProvider::Claude, "work");
    let summary = fixture.summary_for(&fixture.path(".claude-work")).unwrap();
    assert!(!summary.is_logged_in);
    assert!(summary.is_removable);
    assert_eq!(summary.account_name.as_deref(), Some("work"));
}

#[test]
fn listed_ids_link_discovered_accounts_and_cache() {
    let fixture = fixture();
    fixture.add(AiUsageProvider::Claude, "work");
    let usage = TestUsage::new(&fixture.home);
    let registrations = fixture.service.get_ai_account_registrations().unwrap();
    let discovered =
        discover_provider_accounts(&usage.context, AiUsageProvider::Claude, &registrations);
    let summary = fixture
        .service
        .list_ai_accounts()
        .unwrap()
        .into_iter()
        .find(|summary| summary.account_name.as_deref() == Some("work"))
        .unwrap();
    let source = discovered
        .iter()
        .find(|account| account.account_root == summary.account_root)
        .unwrap();
    assert_eq!(summary.account_id, source.account_id);
    assert_eq!(
        summary.cached_account_id,
        to_cached_account_id(&source.account_id)
    );
}

#[test]
fn cli_reported_label_and_plan_are_used() {
    let fixture = fixture();
    fixture.add(AiUsageProvider::Claude, "work");
    *fixture.cli.auth_status.lock().unwrap() = Some(ProviderAuthStatus {
        is_logged_in: true,
        label: Some("work@example.com".to_string()),
        plan_name: Some("max".to_string()),
    });
    let summary = fixture.summary_for(&fixture.path(".claude-work")).unwrap();
    assert_eq!(summary.label, "work@example.com");
    assert_eq!(summary.plan_name.as_deref(), Some("max"));
    assert!(summary.is_logged_in);
}

#[test]
fn unanswered_cli_falls_back_to_credentials_file() {
    let fixture = fixture();
    fixture.write_codex_auth(".codex");
    let codex = fixture
        .service
        .list_ai_accounts()
        .unwrap()
        .into_iter()
        .find(|summary| summary.provider == AiUsageProvider::Codex)
        .unwrap();
    assert!(codex.is_logged_in);
}

#[test]
fn manual_directories_are_not_removable() {
    let fixture = fixture();
    fixture.write_codex_auth(".codex-manual");
    let summary = fixture.summary_for(&fixture.path(".codex-manual")).unwrap();
    assert!(!summary.is_removable);
}

#[test]
fn registered_account_is_logged_out_and_removed() {
    let fixture = fixture();
    let account_root = fixture
        .add(AiUsageProvider::Claude, "work")
        .account_root
        .unwrap();
    let result = fixture
        .service
        .remove_ai_account(AiUsageProvider::Claude, &account_root)
        .unwrap();
    assert_eq!(result.outcome, AiAccountMutationOutcome::Ok);
    assert_eq!(
        fixture.cli.logouts.lock().unwrap()[0],
        (AiUsageProvider::Claude, account_root.clone())
    );
    assert!(!is_directory(&account_root));
    assert!(
        fixture
            .service
            .get_ai_account_registrations()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn manual_root_is_only_logged_out() {
    let fixture = fixture();
    let manual_root = fixture.path(".claude-manual");
    std::fs::create_dir_all(&manual_root).unwrap();
    let result = fixture
        .service
        .remove_ai_account(AiUsageProvider::Claude, &manual_root)
        .unwrap();
    assert_eq!(result.outcome, AiAccountMutationOutcome::Ok);
    assert!(is_directory(&manual_root));
}

#[test]
fn providers_without_logout_report_manual_logout() {
    let fixture = fixture();
    fixture
        .cli
        .is_logout_successful
        .store(false, Ordering::SeqCst);
    let result = fixture
        .service
        .remove_ai_account(AiUsageProvider::Gemini, &fixture.path(".gemini-manual"))
        .unwrap();
    assert_eq!(
        result.outcome,
        AiAccountMutationOutcome::ManualLogoutRequired
    );
}

#[test]
fn concurrent_adds_of_different_accounts_are_both_kept() {
    let fixture = fixture();
    let outcomes = std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.add(AiUsageProvider::Claude, "first").outcome);
        let second = scope.spawn(|| fixture.add(AiUsageProvider::Codex, "second").outcome);
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(
        outcomes,
        [AiAccountMutationOutcome::Ok, AiAccountMutationOutcome::Ok]
    );
    let mut names = fixture.registration_names();
    names.sort();
    assert_eq!(names, ["first", "second"]);
}

#[test]
fn concurrent_adds_of_same_account_register_once() {
    let fixture = fixture();
    let mut outcomes = std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.add(AiUsageProvider::Claude, "same").outcome);
        let second = scope.spawn(|| fixture.add(AiUsageProvider::Claude, "same").outcome);
        vec![first.join().unwrap(), second.join().unwrap()]
    });
    outcomes.sort_by_key(|outcome| *outcome != AiAccountMutationOutcome::Ok);
    assert_eq!(
        outcomes,
        [
            AiAccountMutationOutcome::Ok,
            AiAccountMutationOutcome::AlreadyExists
        ]
    );
    assert_eq!(
        fixture
            .service
            .get_ai_account_registrations()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn add_during_removal_logout_is_preserved() {
    let fixture = fixture();
    let account_root = fixture
        .add(AiUsageProvider::Claude, "old")
        .account_root
        .unwrap();
    let (finish_logout, gate) = mpsc::channel();
    *fixture.cli.logout_gate.lock().unwrap() = Some(gate);
    std::thread::scope(|scope| {
        let removal = scope.spawn(|| {
            fixture
                .service
                .remove_ai_account(AiUsageProvider::Claude, &account_root)
                .unwrap()
        });
        while fixture.cli.logout_started.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let addition = scope.spawn(|| fixture.add(AiUsageProvider::Codex, "new"));
        finish_logout.send(()).unwrap();
        removal.join().unwrap();
        addition.join().unwrap();
    });
    assert_eq!(fixture.registration_names(), ["new"]);
}

#[test]
fn failed_save_does_not_block_next_change() {
    let fixture = fixture();
    fixture.store.failing_writes.store(1, Ordering::SeqCst);
    let first = fixture
        .service
        .add_ai_account(AiUsageProvider::Claude, "failed");
    assert_eq!(first.unwrap_err().to_string(), "save failed");
    assert_eq!(
        fixture.add(AiUsageProvider::Codex, "survived").outcome,
        AiAccountMutationOutcome::Ok
    );
    assert_eq!(fixture.registration_names(), ["survived"]);
}
