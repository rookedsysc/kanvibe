use serde_json::json;
use sha2::{Digest, Sha256};

use super::*;
use crate::javascript::stringify;
use crate::test_support::{temp_home, write_file};
use crate::usage::context::UsagePlatform;
use crate::usage::test_context::{TestUsage, exited};

struct HomeFixture {
    _directory: tempfile::TempDir,
    home: String,
}

impl HomeFixture {
    fn new() -> Self {
        let (directory, home) = temp_home();
        Self {
            _directory: directory,
            home,
        }
    }

    fn path(&self, relative: &str) -> String {
        format!("{}/{relative}", self.home)
    }

    fn usage(&self, variables: &[(&str, &str)]) -> TestUsage {
        TestUsage::with(&self.home, UsagePlatform::Linux, variables)
    }

    /// Keychain이 있는 macOS. 모든 조회가 `keychain`으로 답한다
    fn darwin_usage(&self, variables: &[(&str, &str)], keychain: Result<String, i32>) -> TestUsage {
        let usage = TestUsage::with(&self.home, UsagePlatform::Darwin, variables);
        usage.processes.respond(move |_| match &keychain {
            Ok(credentials) => exited(0, credentials),
            Err(exit_code) => exited(*exit_code, ""),
        });
        usage
    }

    fn make_directory(&self, relative: &str) {
        std::fs::create_dir_all(self.path(relative)).unwrap();
    }

    fn write_json(&self, relative: &str, value: serde_json::Value) {
        write_file(&self.path(relative), &stringify(&value));
    }

    /// TS `writeClaudeConfigDir`. 자격증명과 계정 정보를 홈이나 config dir 안에 둔다
    fn write_claude_config_dir(
        &self,
        directory_name: &str,
        account: serde_json::Value,
        is_colocated: bool,
    ) -> String {
        self.write_json(
            &format!("{directory_name}/.credentials.json"),
            json!({"claudeAiOauth": {"accessToken": "token", "refreshToken": "refresh"}}),
        );
        let config_path = if is_colocated {
            format!("{directory_name}/.claude.json")
        } else {
            ".claude.json".to_string()
        };
        self.write_json(&config_path, json!({"oauthAccount": account}));
        self.path(directory_name)
    }

    fn write_gemini_credentials(&self, config_dir: &str) {
        self.write_json(
            &format!("{config_dir}/oauth_creds.json"),
            json!({"access_token": "token", "refresh_token": "refresh", "expiry_date": 0}),
        );
    }
}

fn discover(usage: &TestUsage, provider: AiUsageProvider) -> Vec<AiUsageAccount> {
    discover_provider_accounts(&usage.context, provider, &[])
}

fn scoped_service(config_dir: &str) -> String {
    let hex: String = Sha256::digest(config_dir.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("Claude Code-credentials-{}", &hex[..8])
}

fn keychain_token() -> String {
    stringify(&json!({"claudeAiOauth": {"accessToken": "keychain-token"}}))
}

#[test]
fn finds_default_claude_account_with_email_label() {
    let fixture = HomeFixture::new();
    fixture.write_claude_config_dir(".claude", json!({"accountUuid": "uuid-default", "emailAddress": "me@example.com", "displayName": "Me"}), false);
    let accounts = discover(
        &fixture.usage(&[("CLAUDE_CONFIG_DIR", "")]),
        AiUsageProvider::Claude,
    );
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].provider, AiUsageProvider::Claude);
    assert_eq!(accounts[0].account_id, "uuid-default");
    assert_eq!(accounts[0].label, "me@example.com");
    assert_eq!(accounts[0].config_dir, fixture.path(".claude"));
}

#[test]
fn label_falls_back_to_display_name_then_default() {
    let fixture = HomeFixture::new();
    let usage = fixture.usage(&[("CLAUDE_CONFIG_DIR", "")]);
    fixture.write_claude_config_dir(
        ".claude",
        json!({"accountUuid": "uuid-name-only", "displayName": "Work Seat"}),
        false,
    );
    assert_eq!(
        discover(&usage, AiUsageProvider::Claude)[0].label,
        "Work Seat"
    );
    fixture.write_json(".claude.json", json!({}));
    assert_eq!(discover(&usage, AiUsageProvider::Claude)[0].label, "Claude");
}

#[test]
fn finds_second_account_in_sibling_directory() {
    let fixture = HomeFixture::new();
    fixture.write_claude_config_dir(
        ".claude",
        json!({"accountUuid": "uuid-personal", "emailAddress": "me@example.com"}),
        false,
    );
    fixture.write_claude_config_dir(
        ".claude-work",
        json!({"accountUuid": "uuid-work", "emailAddress": "work@example.com"}),
        true,
    );
    let mut labels: Vec<String> = discover(
        &fixture.usage(&[("CLAUDE_CONFIG_DIR", "")]),
        AiUsageProvider::Claude,
    )
    .into_iter()
    .map(|account| account.label)
    .collect();
    labels.sort();
    assert_eq!(labels, ["me@example.com", "work@example.com"]);
}

#[test]
fn same_account_from_two_paths_is_returned_once() {
    let fixture = HomeFixture::new();
    fixture.write_claude_config_dir(
        ".claude",
        json!({"accountUuid": "uuid-same", "emailAddress": "me@example.com"}),
        false,
    );
    fixture.write_claude_config_dir(
        ".claude-mirror",
        json!({"accountUuid": "uuid-same", "emailAddress": "me@example.com"}),
        true,
    );
    let configured = fixture.path(".claude");
    let accounts = discover(
        &fixture.usage(&[("CLAUDE_CONFIG_DIR", &configured)]),
        AiUsageProvider::Claude,
    );
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].config_dir, configured);
}

#[test]
fn directory_without_credentials_is_not_an_account() {
    let fixture = HomeFixture::new();
    fixture.make_directory(".claude-empty");
    assert!(
        discover(
            &fixture.usage(&[("CLAUDE_CONFIG_DIR", "")]),
            AiUsageProvider::Claude
        )
        .is_empty()
    );
}

#[test]
fn keychain_login_counts_without_credentials_file() {
    let fixture = HomeFixture::new();
    fixture.make_directory(".claude");
    fixture.write_json(
        ".claude.json",
        json!({"oauthAccount": {"accountUuid": "uuid-mac", "emailAddress": "mac@example.com"}}),
    );
    let usage = fixture.darwin_usage(&[("CLAUDE_CONFIG_DIR", "")], Ok(keychain_token()));
    let accounts = discover(&usage, AiUsageProvider::Claude);
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, "uuid-mac");
    assert_eq!(accounts[0].label, "mac@example.com");
    assert_eq!(accounts[0].config_dir, fixture.path(".claude"));
}

#[test]
fn keychain_receives_every_candidate_config_dir() {
    let fixture = HomeFixture::new();
    let work_dir = fixture.path(".claude-work");
    fixture.make_directory(".claude");
    fixture.make_directory(".claude-work");
    let usage = fixture.darwin_usage(&[("CLAUDE_CONFIG_DIR", &work_dir)], Err(44));
    discover(&usage, AiUsageProvider::Claude);
    let services: Vec<String> = usage
        .processes
        .arguments()
        .into_iter()
        .filter(|args| args[0] == "find-generic-password")
        .map(|args| args[args.iter().position(|arg| arg == "-s").unwrap() + 1].clone())
        .collect();
    let work_service = scoped_service(&work_dir);
    let default_service = scoped_service(&fixture.path(".claude"));
    let legacy = "Claude Code-credentials".to_string();
    assert_eq!(
        services,
        [
            work_service.clone(),
            work_service,
            default_service.clone(),
            default_service,
            legacy.clone(),
            legacy
        ]
    );
}

#[test]
fn unreadable_keychain_still_leaves_an_account_slot() {
    let fixture = HomeFixture::new();
    fixture.make_directory(".claude");
    let usage = fixture.darwin_usage(&[("CLAUDE_CONFIG_DIR", "")], Err(36));
    assert_eq!(discover(&usage, AiUsageProvider::Claude).len(), 1);
}

#[test]
fn identity_falls_back_to_home_config_for_default_root() {
    let fixture = HomeFixture::new();
    fixture.write_json(".claude/.claude.json", json!({"numStartups": 1}));
    fixture.write_json(
        ".claude.json",
        json!({"oauthAccount": {"accountUuid": "uuid-mac", "emailAddress": "mac@example.com"}}),
    );
    let usage = fixture.darwin_usage(&[("CLAUDE_CONFIG_DIR", "")], Ok(keychain_token()));
    let accounts = discover(&usage, AiUsageProvider::Claude);
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, "uuid-mac");
    assert_eq!(accounts[0].label, "mac@example.com");
}

#[test]
fn sibling_root_does_not_inherit_home_identity() {
    let fixture = HomeFixture::new();
    fixture.write_claude_config_dir(
        ".claude",
        json!({"accountUuid": "uuid-home", "emailAddress": "home@example.com"}),
        false,
    );
    let work_dir = fixture.path(".claude-work");
    fixture.write_json(
        ".claude-work/.credentials.json",
        json!({"claudeAiOauth": {"accessToken": "token"}}),
    );
    fixture.write_json(".claude-work/.claude.json", json!({"numStartups": 1}));
    let accounts = discover(
        &fixture.usage(&[("CLAUDE_CONFIG_DIR", "")]),
        AiUsageProvider::Claude,
    );
    assert_eq!(accounts.len(), 2);
    let work_account = accounts
        .iter()
        .find(|account| account.account_root == work_dir)
        .unwrap();
    assert_eq!(work_account.account_id, work_dir);
    assert_eq!(work_account.label, "work");
}

#[test]
fn file_account_skips_keychain() {
    let fixture = HomeFixture::new();
    fixture.write_claude_config_dir(
        ".claude",
        json!({"accountUuid": "uuid-file", "emailAddress": "file@example.com"}),
        false,
    );
    let usage = fixture.darwin_usage(&[("CLAUDE_CONFIG_DIR", "")], Ok(keychain_token()));
    let ids: Vec<String> = discover(&usage, AiUsageProvider::Claude)
        .into_iter()
        .map(|account| account.account_id)
        .collect();
    assert_eq!(ids, ["uuid-file"]);
    assert_eq!(usage.processes.request_count(), 0);
}

#[test]
fn codex_account_uses_account_id_and_id_token_email() {
    let fixture = HomeFixture::new();
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(stringify(&json!({"email": "codex@example.com"})));
    fixture.write_json(
        ".codex/auth.json",
        json!({"tokens": {"access_token": "token", "account_id": "acct-1", "id_token": format!("header.{payload}.sig")}}),
    );
    let accounts = discover(
        &fixture.usage(&[("CODEX_HOME", "")]),
        AiUsageProvider::Codex,
    );
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_id, "acct-1");
    assert_eq!(accounts[0].label, "codex@example.com");
}

#[test]
fn codex_auth_without_access_token_is_not_an_account() {
    let fixture = HomeFixture::new();
    fixture.write_json(".codex/auth.json", json!({"tokens": null}));
    assert!(
        discover(
            &fixture.usage(&[("CODEX_HOME", "")]),
            AiUsageProvider::Codex
        )
        .is_empty()
    );
}

#[test]
fn finds_default_gemini_account_under_home() {
    let fixture = HomeFixture::new();
    fixture.write_gemini_credentials(".gemini");
    let accounts = discover(
        &fixture.usage(&[("GEMINI_CLI_HOME", "")]),
        AiUsageProvider::Gemini,
    );
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].config_dir, fixture.path(".gemini"));
    assert_eq!(accounts[0].account_root, fixture.home);
}

#[test]
fn finds_gemini_account_under_sibling_root() {
    let fixture = HomeFixture::new();
    fixture.write_gemini_credentials(".gemini-work/.gemini");
    let accounts = discover(
        &fixture.usage(&[("GEMINI_CLI_HOME", "")]),
        AiUsageProvider::Gemini,
    );
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].account_root, fixture.path(".gemini-work"));
    assert_eq!(accounts[0].config_dir, fixture.path(".gemini-work/.gemini"));
}

#[test]
fn multiple_gemini_accounts_are_labelled_by_name() {
    let fixture = HomeFixture::new();
    fixture.write_gemini_credentials(".gemini");
    fixture.write_gemini_credentials(".gemini-work/.gemini");
    let labels: Vec<String> = discover(
        &fixture.usage(&[("GEMINI_CLI_HOME", "")]),
        AiUsageProvider::Gemini,
    )
    .into_iter()
    .map(|account| account.label)
    .collect();
    assert_eq!(labels, ["Gemini", "work"]);
}

#[test]
fn registered_account_keeps_its_slot_while_signed_out() {
    let fixture = HomeFixture::new();
    let work_root = fixture.path(".gemini-work");
    let registration = AiAccountRegistration {
        provider: AiUsageProvider::Gemini,
        account_root: work_root.clone(),
        account_name: "work".to_string(),
    };
    let usage = fixture.usage(&[("GEMINI_CLI_HOME", "")]);
    let accounts =
        discover_provider_accounts(&usage.context, AiUsageProvider::Gemini, &[registration]);
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].label, "work");
    assert_eq!(accounts[0].config_dir, format!("{work_root}/.gemini"));
}

#[test]
fn other_provider_registrations_do_not_leak_in() {
    let fixture = HomeFixture::new();
    let registration = AiAccountRegistration {
        provider: AiUsageProvider::Claude,
        account_root: fixture.path(".claude-work"),
        account_name: "work".to_string(),
    };
    let usage = fixture.usage(&[("GEMINI_CLI_HOME", "")]);
    assert!(
        discover_provider_accounts(&usage.context, AiUsageProvider::Gemini, &[registration])
            .is_empty()
    );
}
