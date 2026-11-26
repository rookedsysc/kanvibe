//! AI 계정 로그인 CLI를 띄우는 PTY(`kanvibe_ai::accounts::login_terminal::LoginTerminalSpawner` 구현).
//! 로그인 세션은 태스크 터미널이 아니라서 레지스트리에 올리지 않고, 세션 수명은 로그인 브리지가 관리한다.

#[cfg(test)]
mod tests;

use std::sync::Arc;

use kanvibe_ai::accounts::login_terminal::{
    LoginTerminal, LoginTerminalCallbacks, LoginTerminalRequest, LoginTerminalSpawner,
};

use crate::pty::{PtyEventCallbacks, PtyProcess, PtySpawnRequest, PtySpawner};

/// 서버 런타임 값. 로그인 CLI 환경은 허용 목록으로 만들어지지만, 사용자에게 보이는 셸로 새지 않도록 PTY 경계에서 한 번 더 거른다
const SERVER_RUNTIME_KEYS: [&str; 3] = ["PORT", "HOST", "NODE_ENV"];

fn is_server_runtime_key(key: &str) -> bool {
    SERVER_RUNTIME_KEYS.contains(&key) || key.starts_with("KANVIBE_")
}

pub struct PtyLoginTerminalSpawner {
    pty_spawner: Arc<dyn PtySpawner>,
}

impl PtyLoginTerminalSpawner {
    pub fn new(pty_spawner: Arc<dyn PtySpawner>) -> Self {
        Self { pty_spawner }
    }
}

impl LoginTerminalSpawner for PtyLoginTerminalSpawner {
    fn spawn(
        &self,
        request: LoginTerminalRequest,
        callbacks: LoginTerminalCallbacks,
    ) -> Result<Arc<dyn LoginTerminal>, String> {
        let environment = request
            .environment
            .into_iter()
            .filter(|(key, _)| !is_server_runtime_key(key))
            .collect();
        let pty_request = PtySpawnRequest {
            program: request.program,
            args: request.args,
            terminal_name: request.terminal_name,
            columns: request.columns,
            rows: request.rows,
            working_directory: request.working_directory,
            environment,
        };
        let on_exit = callbacks.on_exit;
        let pty_callbacks = PtyEventCallbacks {
            on_output: callbacks.on_output,
            on_exit: Box::new(move |exit| on_exit(exit.exit_code)),
        };
        let pty = self.pty_spawner.spawn(pty_request, pty_callbacks)?;
        Ok(Arc::new(PtyLoginTerminal { pty }))
    }
}

struct PtyLoginTerminal {
    pty: Arc<dyn PtyProcess>,
}

impl LoginTerminal for PtyLoginTerminal {
    fn write(&self, data: &str) -> Result<(), String> {
        self.pty.write(data)
    }

    fn resize(&self, columns: u16, rows: u16) -> Result<(), String> {
        self.pty.resize(columns, rows)
    }

    fn kill(&self) -> Result<(), String> {
        self.pty.kill()
    }
}
