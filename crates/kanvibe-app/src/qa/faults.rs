//! QA 모드 서비스 고장 주입. 대상 이름은 TS IPC `namespace.method`(예: `kanban.getTasksByStatus`)를 식별자로 쓴다.
//! 판정 규칙은 TS 드라이버 shim(`parity/tools/screens/ts-driver/main-shim.cjs`의 `findFault`)과 같다.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FaultMode {
    /// 영원히 응답하지 않는다
    Hang,
    /// `ms` 뒤 `message`로 실패한다
    Fail,
    /// `ms` 뒤 정상 처리한다
    Delay,
    /// 정상 처리 대신 `value`를 돌려준다
    Result,
}

/// states.yaml `faults` 항목 하나. `value_ref`는 드라이버가 `service-results.json`에서 풀어 `value`로 넘긴다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ServiceFault {
    pub target: String,
    pub mode: FaultMode,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub ms: Option<u64>,
    #[serde(default)]
    pub value: Option<Value>,
    /// 이 횟수를 넘긴 호출부터 적용한다
    #[serde(default)]
    pub after_calls: Option<u32>,
    /// 호출 인자(JSON 문자열)에 이 글자가 있을 때만 적용한다
    #[serde(default, rename = "match")]
    pub argument_match: Option<String>,
}

/// 서비스 호출이 고장 판정 뒤 해야 할 일
#[derive(Debug, Clone, PartialEq)]
pub enum FaultOutcome {
    Proceed,
    Hang,
    Fail { message: String, after: Duration },
    Delay(Duration),
    Result(Value),
}

const DEFAULT_FAIL_MESSAGE: &str = "parity fixture: injected failure";
const DEFAULT_DELAY: Duration = Duration::from_millis(1000);

#[derive(Debug, Default)]
pub struct FaultRegistry {
    faults: Mutex<Vec<ServiceFault>>,
    call_counts: Mutex<HashMap<String, u32>>,
}

#[derive(Debug, thiserror::Error)]
pub enum FaultFileError {
    #[error("고장 주입 파일을 읽지 못했습니다({path}): {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("고장 주입 파일 형식이 잘못되었습니다({path}): {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
}

impl FaultRegistry {
    /// `[ServiceFault, ...]` JSON 파일로 첫 화면 전에 걸 고장을 읽는다
    pub fn load_file(path: &Path) -> Result<Self, FaultFileError> {
        let path_text = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|source| FaultFileError::Read {
            path: path_text.clone(),
            source,
        })?;
        let faults: Vec<ServiceFault> =
            serde_json::from_str(&text).map_err(|source| FaultFileError::Parse {
                path: path_text,
                source,
            })?;
        let registry = Self::default();
        faults.into_iter().for_each(|fault| registry.add(fault));
        Ok(registry)
    }

    /// 같은 대상에 이미 고장이 있으면 뒤에 덧붙인다(TS 드라이버 `setFault`와 같다)
    pub fn add(&self, fault: ServiceFault) {
        self.lock_faults().push(fault);
    }

    pub fn clear(&self, target: &str) {
        self.lock_faults().retain(|fault| fault.target != target);
    }

    /// 호출 한 번을 세고, 걸린 고장이 있으면 그 결과를 돌려준다
    pub fn check(&self, target: &str, serialized_arguments: &str) -> FaultOutcome {
        let call_count = self.count_call(target);
        let namespace_wildcard = target
            .split_once('.')
            .map(|(namespace, _)| format!("{namespace}.*"));
        let faults = self.lock_faults();
        let exact = faults.iter().filter(|fault| fault.target == target);
        let wildcard = faults
            .iter()
            .filter(|fault| Some(&fault.target) == namespace_wildcard.as_ref());
        exact
            .chain(wildcard)
            .find(|fault| fault_applies(fault, call_count, serialized_arguments))
            .map(fault_outcome)
            .unwrap_or(FaultOutcome::Proceed)
    }

    fn count_call(&self, target: &str) -> u32 {
        let mut call_counts = self
            .call_counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = call_counts.entry(target.to_string()).or_default();
        *count += 1;
        *count
    }

    fn lock_faults(&self) -> std::sync::MutexGuard<'_, Vec<ServiceFault>> {
        self.faults
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn fault_applies(fault: &ServiceFault, call_count: u32, serialized_arguments: &str) -> bool {
    fault.after_calls.unwrap_or(0) < call_count
        && fault
            .argument_match
            .as_deref()
            .is_none_or(|needle| serialized_arguments.contains(needle))
}

fn fault_outcome(fault: &ServiceFault) -> FaultOutcome {
    let configured_wait = fault.ms.map(Duration::from_millis);
    match fault.mode {
        FaultMode::Hang => FaultOutcome::Hang,
        FaultMode::Delay => FaultOutcome::Delay(configured_wait.unwrap_or(DEFAULT_DELAY)),
        FaultMode::Fail => FaultOutcome::Fail {
            message: fault
                .message
                .clone()
                .unwrap_or_else(|| DEFAULT_FAIL_MESSAGE.to_string()),
            after: configured_wait.unwrap_or_default(),
        },
        FaultMode::Result => FaultOutcome::Result(fault.value.clone().unwrap_or(Value::Null)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fault(json: &str) -> ServiceFault {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn hang_fault_matches_its_exact_target_only() {
        let registry = FaultRegistry::default();
        registry.add(fault(
            r#"{"target":"kanban.getTasksByStatus","mode":"hang"}"#,
        ));

        assert_eq!(
            registry.check("kanban.getTasksByStatus", "[]"),
            FaultOutcome::Hang
        );
        assert_eq!(
            registry.check("kanban.getTask", "[]"),
            FaultOutcome::Proceed
        );
    }

    #[test]
    fn namespace_wildcard_and_after_calls_follow_the_ts_shim() {
        let registry = FaultRegistry::default();
        registry.add(fault(
            r#"{"target":"project.*","mode":"fail","message":"boom","after_calls":1}"#,
        ));

        assert_eq!(
            registry.check("project.getAll", "[]"),
            FaultOutcome::Proceed
        );
        assert_eq!(
            registry.check("project.getAll", "[]"),
            FaultOutcome::Fail {
                message: "boom".into(),
                after: Duration::ZERO
            }
        );
    }

    #[test]
    fn match_limits_the_fault_to_calls_whose_arguments_contain_the_text() {
        let registry = FaultRegistry::default();
        registry.add(fault(
            r#"{"target":"kanban.getTask","mode":"delay","ms":5,"match":"task-2"}"#,
        ));

        assert_eq!(
            registry.check("kanban.getTask", r#"["task-1"]"#),
            FaultOutcome::Proceed
        );
        assert_eq!(
            registry.check("kanban.getTask", r#"["task-2"]"#),
            FaultOutcome::Delay(Duration::from_millis(5))
        );
        registry.clear("kanban.getTask");
        assert_eq!(
            registry.check("kanban.getTask", r#"["task-2"]"#),
            FaultOutcome::Proceed
        );
    }
}
