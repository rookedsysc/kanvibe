//! `cargo nextest run --message-format libtest-json`으로 workspace 테스트를 돌려 테스트별 결과를 모은다.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    Failed,
    Ignored,
}

/// `kanvibe-db::kanvibe_db$migrations::tests::case` 같은 nextest 이름을 나눈 것
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TestName {
    pub package: String,
    pub binary: String,
    pub test_path: String,
}

impl TestName {
    pub fn parse(full_name: &str) -> Option<Self> {
        let (binary_id, test_path) = full_name.split_once('$')?;
        let (package, binary) = binary_id.split_once("::").unwrap_or((binary_id, binary_id));
        Some(Self {
            package: package.to_string(),
            binary: binary.to_string(),
            test_path: test_path.to_string(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct LibtestEvent {
    #[serde(rename = "type")]
    event_type: String,
    event: String,
    name: Option<String>,
}

pub fn parse_libtest_events(output: &str) -> BTreeMap<TestName, TestOutcome> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<LibtestEvent>(line).ok())
        .filter(|event| event.event_type == "test")
        .filter_map(|event| {
            let outcome = match event.event.as_str() {
                "ok" => TestOutcome::Passed,
                "failed" | "timeout" => TestOutcome::Failed,
                "ignored" => TestOutcome::Ignored,
                _ => return None,
            };
            Some((TestName::parse(event.name.as_deref()?)?, outcome))
        })
        .collect()
}

/// 실패한 테스트가 있어도 끝까지 돌려 모든 결과를 받는다. libtest-json 출력은 아직 실험 기능이라
/// nextest 자식 프로세스에만 켜는 변수를 넘긴다
pub fn run_workspace_tests(
    workspace_root: &Path,
) -> Result<BTreeMap<TestName, TestOutcome>, String> {
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args([
            "nextest",
            "run",
            "--workspace",
            "--no-fail-fast",
            "--message-format",
            "libtest-json",
        ])
        .env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1")
        .current_dir(workspace_root)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("cargo nextest를 실행하지 못했습니다: {error}"))?;

    let outcomes = parse_libtest_events(&String::from_utf8_lossy(&output.stdout));
    if outcomes.is_empty() {
        return Err(
            "nextest 결과가 비어 있습니다. `cargo install cargo-nextest --locked`로 설치했는지, 빌드가 성공했는지 확인하세요"
                .to_string(),
        );
    }
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_finished_test_events_only() {
        let output = concat!(
            r#"{"type":"suite","event":"started","test_count":2}"#,
            "\n",
            r#"{"type":"test","event":"started","name":"kanvibe-db::kanvibe_db$a::tests::one"}"#,
            "\n",
            r#"{"type":"test","event":"ok","name":"kanvibe-db::kanvibe_db$a::tests::one","exec_time":0.1}"#,
            "\n",
            r#"{"type":"test","event":"failed","name":"kanvibe-db::ts_reference_compat$two","stdout":"x"}"#,
            "\n",
            "warning: not json\n",
        );

        let outcomes = parse_libtest_events(output);

        assert_eq!(outcomes.len(), 2);
        let failed_name = TestName::parse("kanvibe-db::ts_reference_compat$two").unwrap();
        assert_eq!(failed_name.binary, "ts_reference_compat");
        assert_eq!(outcomes[&failed_name], TestOutcome::Failed);
    }
}
