//! 계정 로그인 PTY를 node-pty처럼 띄운다.
//!
//! node-pty는 PTY를 먼저 만든 뒤 자식에서 `execvp`가 실패하면 그 이유를 출력하고 종료 코드 1로 끝났다. 그래서 CLI가
//! 없는 PC에서도 기기는 `ready` → 출력 → `exit` 순서를 받았다. Rust PTY는 실행 파일을 먼저 찾아 spawn 자체가 실패하므로,
//! 찾지 못한 경우에만 같은 출력과 종료 코드를 내는 셸로 바꿔 띄운다.

use std::path::Path;
use std::sync::Arc;

use kanvibe_ai::accounts::login_terminal::{
    LoginTerminal, LoginTerminalCallbacks, LoginTerminalRequest, LoginTerminalSpawner,
};

const MISSING_PROGRAM_SCRIPT: &str =
    "printf 'execvp(3) failed.: No such file or directory\\r\\n'; exit 1";

pub struct NodePtyCompatibleLoginSpawner {
    inner: Arc<dyn LoginTerminalSpawner>,
}

impl NodePtyCompatibleLoginSpawner {
    pub fn new(inner: Arc<dyn LoginTerminalSpawner>) -> Self {
        Self { inner }
    }
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// `execvp`가 찾는 방식: `/`가 있으면 그 경로, 없으면 자식 PATH의 디렉터리 순서
fn can_exec(program: &str, path_variable: Option<&String>) -> bool {
    if program.contains('/') {
        return is_executable(Path::new(program));
    }
    path_variable
        .map(|value| {
            value
                .split(':')
                .any(|directory| is_executable(&Path::new(directory).join(program)))
        })
        .unwrap_or(false)
}

impl LoginTerminalSpawner for NodePtyCompatibleLoginSpawner {
    fn spawn(
        &self,
        mut request: LoginTerminalRequest,
        callbacks: LoginTerminalCallbacks,
    ) -> Result<Arc<dyn LoginTerminal>, String> {
        if !can_exec(&request.program, request.environment.get("PATH")) {
            request.program = "/bin/sh".to_string();
            request.args = vec!["-c".to_string(), MISSING_PROGRAM_SCRIPT.to_string()];
        }
        self.inner.spawn(request, callbacks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_are_resolved_like_execvp() {
        let path = "/nonexistent:/bin".to_string();
        assert!(can_exec("sh", Some(&path)));
        assert!(!can_exec("kanvibe-missing-cli", Some(&path)));
        assert!(can_exec("/bin/sh", None));
        assert!(!can_exec("sh", None));
    }
}
