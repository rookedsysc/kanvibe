//! TS `hookServerStatus.test.ts`를 옮긴 테스트.

use std::sync::Arc;

use super::installer_test_support::{FakeHookClientRuntime, install_context};
use super::server_status::{HookServerValidation, validate_hook_server_configuration};
use crate::exec::ExecGitError;
use crate::exec::testing::ScriptedCommandRunner;

struct Scenario {
    runner: Arc<ScriptedCommandRunner>,
    runtime: Arc<FakeHookClientRuntime>,
}

impl Scenario {
    fn new(hook_server_url: Result<&str, &str>, runner: ScriptedCommandRunner) -> Self {
        let mut runtime = FakeHookClientRuntime::new();
        runtime.fixed_client_id = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed".into();
        runtime.hook_server_url = hook_server_url.map(str::to_string).map_err(str::to_string);
        Self {
            runner: Arc::new(runner),
            runtime: Arc::new(runtime),
        }
    }

    fn validate(&self, configured_url: &str, ssh_host: Option<&str>) -> HookServerValidation {
        let context = install_context(self.runner.clone(), self.runtime.clone(), "/home/test");
        validate_hook_server_configuration(
            &context,
            &[Some(configured_url.to_string())],
            true,
            ssh_host,
        )
    }

    fn assert_single_strict_command_containing(&self, fragment: &str) {
        let calls = self.runner.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].command.contains(fragment), "{}", calls[0].command);
        assert_eq!(calls[0].ssh_host.as_deref(), Some("remote-host"));
        assert!(calls[0].options.should_check_host_key_strictly);
        assert_eq!(calls[0].options.timeout, None);
    }
}

#[test]
fn marks_remote_hook_urls_with_a_mismatched_port_as_invalid() {
    let scenario = Scenario::new(
        Ok("http://10.0.0.4:6379"),
        ScriptedCommandRunner::returning(""),
    );

    let result = scenario.validate("http://192.168.0.8:9736", Some("remote-host"));

    assert!(!result.has_expected_hook_server_url);
    assert!(!result.has_reachable_hook_server);
    assert!(scenario.runner.calls().is_empty());
}

#[test]
fn accepts_remote_hook_urls_with_a_different_host_when_the_port_matches() {
    let scenario = Scenario::new(
        Ok("http://10.0.0.4:6379"),
        ScriptedCommandRunner::returning(""),
    );

    let result = scenario.validate("http://100.83.96.29:6379", Some("remote-host"));

    assert!(result.has_expected_hook_server_url);
    assert!(result.has_reachable_hook_server);
    scenario.assert_single_strict_command_containing("http://100.83.96.29:6379/api/hooks/health");
}

#[test]
fn accepts_remote_urls_on_the_active_port_when_the_route_cannot_be_recomputed() {
    let scenario = Scenario::new(
        Err("route unavailable"),
        ScriptedCommandRunner::returning(""),
    );

    let result = scenario.validate("http://100.83.96.29:9736", Some("remote-host"));

    assert!(result.has_expected_hook_server_url);
    assert!(result.has_reachable_hook_server);
    assert_eq!(result.expected_hook_server_url, None);
    scenario.assert_single_strict_command_containing("http://100.83.96.29:9736/api/hooks/health");
}

#[test]
fn rejects_remote_urls_with_a_stale_port_when_the_route_cannot_be_recomputed() {
    let scenario = Scenario::new(
        Err("route unavailable"),
        ScriptedCommandRunner::returning(""),
    );

    let result = scenario.validate("http://100.83.96.29:19736", Some("remote-host"));

    assert!(!result.has_expected_hook_server_url);
    assert!(!result.has_reachable_hook_server);
    assert!(scenario.runner.calls().is_empty());
}

#[test]
fn checks_hook_server_reachability_from_the_remote_host() {
    let scenario = Scenario::new(
        Ok("http://10.0.0.4:9736"),
        ScriptedCommandRunner::returning(""),
    );

    let result = scenario.validate("http://10.0.0.4:9736", Some("remote-host"));

    assert!(result.has_expected_hook_server_url);
    assert!(result.has_reachable_hook_server);
    scenario.assert_single_strict_command_containing(
        "curl --unix-socket \"$__kanvibe_socket\" -fsS --max-time 2",
    );
}

#[test]
fn ssh_connection_failure_is_not_reported_as_reachable() {
    let failing_runner = ScriptedCommandRunner::new(|_| {
        Err(ExecGitError::new(
            "remote-host 원격 명령 실패: Connection reset by 100.73.171.123 port 22",
        ))
    });
    let scenario = Scenario::new(Ok("http://10.0.0.4:9736"), failing_runner);

    let result = scenario.validate("http://10.0.0.4:9736", Some("remote-host"));

    assert!(result.has_expected_hook_server_url);
    assert!(!result.has_reachable_hook_server);
}

#[test]
fn treats_a_local_health_check_failure_as_not_installed() {
    let mut runtime = FakeHookClientRuntime::new();
    runtime.is_local_server_healthy = false;
    let scenario = Scenario {
        runner: Arc::new(ScriptedCommandRunner::returning("")),
        runtime: Arc::new(runtime),
    };

    let result = scenario.validate("http://localhost:9736", None);

    assert!(result.has_expected_hook_server_url);
    assert!(!result.has_reachable_hook_server);
    assert_eq!(
        *scenario.runtime.health_check_urls.lock().unwrap(),
        ["http://localhost:9736/api/hooks/health"]
    );
}

#[test]
fn loopback_aliases_match_for_local_urls_but_ports_must_match() {
    let scenario = Scenario::new(
        Ok("http://localhost:9736"),
        ScriptedCommandRunner::returning(""),
    );

    assert!(
        scenario
            .validate("http://127.0.0.1:9736/", None)
            .has_expected_hook_server_url
    );
    assert!(
        !scenario
            .validate("http://127.0.0.1:19736", None)
            .has_expected_hook_server_url
    );
    assert!(
        !scenario
            .validate("http://10.0.0.5:9736", None)
            .has_expected_hook_server_url
    );
    assert!(
        !scenario
            .validate("not a url", None)
            .has_expected_hook_server_url
    );
}
