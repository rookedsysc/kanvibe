use serde_json::json;

use super::*;
use crate::javascript::stringify;
use crate::usage::context::ProcessOutput;
use crate::usage::test_context::{TestUsage, exited};

fn usage_with(variables: &[(&str, &str)]) -> TestUsage {
    TestUsage::with("/home/tester", UsagePlatform::Linux, variables)
}

fn environment_for(
    usage: &TestUsage,
    provider: AiUsageProvider,
    account_root: &str,
) -> BTreeMap<String, String> {
    create_provider_cli_environment(&usage.context, config_dir_spec(provider), account_root)
}

#[test]
fn adds_only_the_account_location_variable() {
    let usage = usage_with(&[]);
    let environment = environment_for(&usage, AiUsageProvider::Gemini, "/home/tester/.gemini-work");
    assert_eq!(
        environment.get("GEMINI_CLI_HOME").map(String::as_str),
        Some("/home/tester/.gemini-work")
    );
    assert!(!environment.contains_key("CLAUDE_CONFIG_DIR"));
    assert!(!environment.contains_key("CODEX_HOME"));
}

#[test]
fn server_runtime_and_kanvibe_values_never_reach_the_cli() {
    let usage = usage_with(&[
        ("PORT", "3000"),
        ("HOST", "0.0.0.0"),
        ("NODE_ENV", "production"),
        ("KANVIBE_INTERNAL_TOKEN", "secret"),
    ]);
    let environment = environment_for(&usage, AiUsageProvider::Claude, "/home/tester/.claude-work");
    for name in ["PORT", "HOST", "NODE_ENV", "KANVIBE_INTERNAL_TOKEN"] {
        assert!(!environment.contains_key(name), "{name}");
    }
    assert_eq!(
        environment.get("CLAUDE_CONFIG_DIR").map(String::as_str),
        Some("/home/tester/.claude-work")
    );
}

#[test]
fn provider_auth_variables_reach_the_cli_but_other_account_locations_do_not() {
    let usage = usage_with(&[
        ("GEMINI_API_KEY", "gemini-key"),
        ("GOOGLE_CLOUD_PROJECT", "project-1"),
        ("ANTHROPIC_API_KEY", "anthropic-key"),
        ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ("OPENAI_API_KEY", "openai-key"),
        ("CODEX_HOME", "/home/tester/.codex-other"),
        ("GEMINI_CLI_HOME", "/home/tester/.gemini-other"),
        ("SECRET_TOKEN", "secret"),
    ]);
    let environment = environment_for(&usage, AiUsageProvider::Claude, "/home/tester/.claude-work");
    for (name, value) in [
        ("GEMINI_API_KEY", "gemini-key"),
        ("GOOGLE_CLOUD_PROJECT", "project-1"),
        ("ANTHROPIC_API_KEY", "anthropic-key"),
        ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ("OPENAI_API_KEY", "openai-key"),
    ] {
        assert_eq!(
            environment.get(name).map(String::as_str),
            Some(value),
            "{name}"
        );
    }
    for name in ["CODEX_HOME", "GEMINI_CLI_HOME", "SECRET_TOKEN"] {
        assert!(!environment.contains_key(name), "{name}");
    }
}

#[test]
fn default_root_gets_no_location_variable() {
    let usage = usage_with(&[]);
    let environment = environment_for(&usage, AiUsageProvider::Claude, "/home/tester/.claude");
    assert!(!environment.contains_key("CLAUDE_CONFIG_DIR"));
}

#[test]
fn inherited_location_variable_is_dropped_for_default_root() {
    let usage = usage_with(&[("CLAUDE_CONFIG_DIR", "/home/tester/.claude-work")]);
    let environment = environment_for(&usage, AiUsageProvider::Claude, "/home/tester/.claude");
    assert!(!environment.contains_key("CLAUDE_CONFIG_DIR"));
}

#[test]
fn home_as_default_root_gets_no_location_variable() {
    let usage = usage_with(&[]);
    let environment = environment_for(&usage, AiUsageProvider::Gemini, "/home/tester");
    assert!(!environment.contains_key("GEMINI_CLI_HOME"));
}

#[test]
fn claude_login_goes_straight_to_subscription_login() {
    assert_eq!(
        get_provider_login_command(AiUsageProvider::Claude),
        ProviderLoginCommand {
            command: "claude".to_string(),
            args: vec![
                "auth".to_string(),
                "login".to_string(),
                "--claudeai".to_string()
            ],
        }
    );
}

#[test]
fn gemini_login_opens_the_cli_itself() {
    assert_eq!(
        get_provider_login_command(AiUsageProvider::Gemini),
        ProviderLoginCommand {
            command: "gemini".to_string(),
            args: Vec::new(),
        }
    );
}

fn status_with(
    provider: AiUsageProvider,
    account_root: &str,
    output: ProcessOutput,
) -> Option<ProviderAuthStatus> {
    let usage = usage_with(&[]);
    usage.processes.respond(move |_| output.clone());
    read_provider_auth_status(&usage.context, provider, account_root)
}

#[test]
fn claude_reports_login_email_and_plan() {
    let stdout =
        stringify(&json!({"loggedIn": true, "email": "me@example.com", "subscriptionType": "max"}));
    assert_eq!(
        status_with(
            AiUsageProvider::Claude,
            "/home/tester/.claude",
            exited(0, &stdout)
        ),
        Some(ProviderAuthStatus {
            is_logged_in: true,
            label: Some("me@example.com".to_string()),
            plan_name: Some("max".to_string()),
        })
    );
}

#[test]
fn codex_reports_login_state_only() {
    assert_eq!(
        status_with(
            AiUsageProvider::Codex,
            "/home/tester/.codex",
            exited(0, "Logged in using ChatGPT\n")
        ),
        Some(ProviderAuthStatus {
            is_logged_in: true,
            label: None,
            plan_name: None,
        })
    );
}

#[test]
fn codex_logged_out_text_is_not_logged_in() {
    assert_eq!(
        status_with(
            AiUsageProvider::Codex,
            "/home/tester/.codex",
            exited(0, "Not logged in\n")
        ),
        Some(ProviderAuthStatus {
            is_logged_in: false,
            label: None,
            plan_name: None,
        })
    );
}

#[test]
fn providers_without_status_command_are_unknown() {
    let usage = usage_with(&[]);
    assert_eq!(
        read_provider_auth_status(
            &usage.context,
            AiUsageProvider::Gemini,
            "/home/tester/.gemini-work"
        ),
        None
    );
    assert_eq!(usage.processes.request_count(), 0);
}

#[test]
fn unrunnable_cli_is_unknown() {
    assert_eq!(
        status_with(
            AiUsageProvider::Claude,
            "/home/tester/.claude",
            ProcessOutput::default()
        ),
        None
    );
}

#[test]
fn unreadable_output_is_unknown() {
    assert_eq!(
        status_with(
            AiUsageProvider::Claude,
            "/home/tester/.claude",
            exited(0, "not json at all")
        ),
        None
    );
}

#[test]
fn cli_runs_with_narrow_environment_and_merged_path() {
    let usage = TestUsage::with(
        "/Users/tester",
        UsagePlatform::Darwin,
        &[
            ("PATH", "/usr/bin:/usr/local/bin"),
            ("DISPLAY", ":0"),
            ("SECRET_TOKEN", "x"),
            ("LANG", ""),
        ],
    );
    usage.processes.respond(|_| exited(0, "Logged in"));
    read_provider_auth_status(
        &usage.context,
        AiUsageProvider::Codex,
        "/Users/tester/.codex-work",
    );
    let request = usage.processes.requests.lock().unwrap()[0].clone();
    assert_eq!(request.program, "codex");
    assert_eq!(request.args, ["login", "status"]);
    assert_eq!(request.timeout_ms, 30_000);
    let environment = request.environment;
    assert!(!environment.contains_key("SECRET_TOKEN"));
    assert_eq!(environment["DISPLAY"], ":0");
    assert_eq!(environment["LANG"], "en_US.UTF-8");
    assert_eq!(environment["CODEX_HOME"], "/Users/tester/.codex-work");
    assert!(environment["PATH"].starts_with("/usr/bin:/usr/local/bin:/opt/homebrew/bin"));
    assert!(environment["PATH"].ends_with("/Users/tester/.bun/bin"));
}

#[test]
fn logout_succeeds_only_on_zero_exit() {
    let usage = usage_with(&[]);
    usage.processes.respond(|_| exited(0, ""));
    assert!(logout_through_cli(
        &usage.context,
        AiUsageProvider::Claude,
        "/home/tester/.claude"
    ));
    assert_eq!(usage.processes.arguments()[0], ["auth", "logout"]);
    usage.processes.respond(|_| exited(1, ""));
    assert!(!logout_through_cli(
        &usage.context,
        AiUsageProvider::Codex,
        "/home/tester/.codex"
    ));
    assert!(!logout_through_cli(
        &usage.context,
        AiUsageProvider::Gemini,
        "/home/tester"
    ));
}
