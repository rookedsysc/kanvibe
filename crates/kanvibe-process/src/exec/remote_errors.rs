//! 원격 명령 실패의 분류와 메시지 정규화.
//! 전송 실패(연결 끊김·거부)는 재시도와 쿨다운 대상이고, 인증·호스트 키 실패는 영구 실패라 다시 보내지 않는다.

use std::sync::LazyLock;
use std::time::Duration;

use regex::{Regex, RegexSet};

use super::ExecGitError;
use super::remote_command::{CommandFailure, is_ssh_command_timeout_error};

static SSH_TRANSPORT_ERROR_PATTERNS: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
        r"(?i)kex_exchange_identification",
        r"(?i)connection reset by peer",
        r"(?i)connection reset by ",
        r"(?i)connection closed by ",
        r"(?i)connection refused",
        r"(?i)connection timed out",
        r"(?i)operation timed out",
        r"(?i)no route to host",
        r"(?i)network is unreachable",
        r"(?i)could not resolve hostname",
        r"(?i)name or service not known",
        r"(?i)temporary failure in name resolution",
        r"(?i)session open refused by peer",
        r"(?i)ssh: connect to host",
    ])
    .expect("유효한 정규식")
});

static SSH_PERMANENT_CONNECTION_ERROR_PATTERNS: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
        r"(?i)host key verification failed",
        r"(?i)remote host identification has changed",
        r"(?i)permission denied",
        r"(?i)too many authentication failures",
        r"(?i)bad owner or permissions",
    ])
    .expect("유효한 정규식")
});

static SSH_INCOMPLETE_COMMAND_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)ssh 명령이 .*완료되지 않았").expect("유효한 정규식"));

static QUIET_FILE_PROBE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^test -[ef] ").expect("유효한 정규식"));

/// 메시지, stderr, stdout을 이어 붙인 판정용 출력
fn collect_error_output(error: &ExecGitError) -> String {
    [
        error.message.as_str(),
        error.stderr.as_str(),
        error.stdout.as_str(),
    ]
    .into_iter()
    .filter(|output| !output.is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

/// 원격 명령 자체의 실패가 아니라 SSH 전송이 끊긴 실패인지
pub fn is_ssh_transport_error(error: &ExecGitError) -> bool {
    let error_output = collect_error_output(error);
    if error.has_remote_command_completed
        || SSH_PERMANENT_CONNECTION_ERROR_PATTERNS.is_match(&error_output)
    {
        return false;
    }
    SSH_TRANSPORT_ERROR_PATTERNS.is_match(&error_output)
}

/// 원격에 닿지 못한 실패(시간 초과, 전송 실패, 인증·호스트 키 실패)인지
pub fn is_ssh_connection_error(error: &ExecGitError) -> bool {
    if error.has_remote_command_completed {
        return false;
    }
    let error_output = collect_error_output(error);
    is_ssh_command_timeout_error(error)
        || SSH_INCOMPLETE_COMMAND_PATTERN.is_match(&error_output)
        || is_ssh_transport_error(error)
        || SSH_PERMANENT_CONNECTION_ERROR_PATTERNS.is_match(&error_output)
}

fn summarize_command_failure(error_output: &str) -> String {
    let last_line = error_output
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).trim())
        .rfind(|line| !line.is_empty());
    last_line
        .unwrap_or("원격 명령 실행에 실패했습니다.")
        .to_string()
}

/// 조용한 probe(`2>/dev/null`, `test -f ... || true`)의 실패는 정상 흐름이므로 로그를 남기지 않는다
pub(crate) fn should_log_remote_command_failure(command: &str) -> bool {
    let trimmed_command = command.trim();
    let is_quiet_probe = command.contains("2>/dev/null")
        || command.contains(">/dev/null 2>&1")
        || (QUIET_FILE_PROBE_PATTERN.is_match(trimmed_command)
            && trimmed_command.contains("|| true"));
    !is_quiet_probe
}

/// 사용자에게 보일 `<host> 원격 명령 실패: ...` 메시지로 바꾼다. 출력이 붙지 않은 오류는 그대로 둔다
pub(crate) fn normalize_ssh_exec_error(
    failure: &CommandFailure,
    ssh_host: &str,
    timeout: Duration,
    is_command_timeout: bool,
) -> ExecGitError {
    if is_command_timeout {
        let seconds = timeout.as_millis().div_ceil(1000);
        return ExecGitError::new(format!(
            "{ssh_host} 원격 명령 실패: SSH 명령이 {seconds}초 안에 완료되지 않았습니다."
        ));
    }
    if !failure.has_output {
        return (*failure.error).clone();
    }
    let stderr = failure.error.stderr.trim();
    let message = if stderr.is_empty() {
        failure.error.message.clone()
    } else {
        summarize_command_failure(stderr)
    };
    ExecGitError {
        message: format!("{ssh_host} 원격 명령 실패: {message}"),
        has_remote_command_completed: failure.error.has_remote_command_completed,
        ..ExecGitError::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_ssh_transport_errors_from_remote_command_failures() {
        let connection_reset = ExecGitError::new(
            "remote-host 원격 명령 실패: Connection reset by 100.73.171.123 port 22",
        );
        let key_exchange = ExecGitError {
            stderr: "kex_exchange_identification: read: Connection reset by peer\n".into(),
            ..ExecGitError::default()
        };
        let command_failure =
            ExecGitError::new("remote-host 원격 명령 실패: tmux 설치에 실패했습니다.");

        assert!(is_ssh_transport_error(&connection_reset));
        assert!(is_ssh_transport_error(&key_exchange));
        assert!(!is_ssh_transport_error(&command_failure));
    }

    #[test]
    fn permanent_failures_are_connection_errors_but_not_transport_errors() {
        for message in [
            "Permission denied (publickey)",
            "Host key verification failed",
        ] {
            let error = ExecGitError::new(message);

            assert!(!is_ssh_transport_error(&error));
            assert!(is_ssh_connection_error(&error));
        }
        assert!(!is_ssh_connection_error(&ExecGitError::new("missing")));
    }
}
