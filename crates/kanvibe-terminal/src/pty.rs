//! PTY 경계(TS `node-pty`). 실제 구현은 portable-pty이고, 레지스트리와 로그인 터미널은 이 trait만 본다.
//! 출력·종료 콜백은 `spawn`이 돌아온 뒤 PTY 읽기 스레드에서만 부른다. 호출자가 잠금을 쥔 채 `spawn`해도 교착하지 않는다.

mod native;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use kanvibe_process::shell_environment::ShellEnvironment;

pub use native::NativePtySpawner;

/// PTY 하나를 띄우는 요청. TS `pty.spawn(file, args, { name, cols, rows, cwd, env })`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySpawnRequest {
    pub program: String,
    pub args: Vec<String>,
    /// `TERM`으로 넘길 터미널 이름
    pub terminal_name: String,
    pub columns: u16,
    pub rows: u16,
    pub working_directory: String,
    /// 이 값만 자식에게 넘긴다(env_clear 후 설정)
    pub environment: ShellEnvironment,
}

/// PTY가 끝난 사유. `is_detached`는 원격 daemon PTY에서 연결만 끊기고 세션은 남은 경우다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyExit {
    pub exit_code: i32,
    pub signal: Option<i32>,
    pub is_detached: bool,
}

/// PTY 출력과 종료를 받는 콜백. 출력은 UTF-8 조각 경계를 맞춘 문자열이다(node-pty `onData`)
pub struct PtyEventCallbacks {
    pub on_output: Box<dyn Fn(&str) + Send + Sync>,
    pub on_exit: Box<dyn Fn(PtyExit) + Send + Sync>,
}

/// 떠 있는 PTY. 네이티브 fd가 먼저 닫혀 쓰기·크기 바꾸기·종료가 실패할 수 있으므로 오류를 돌려준다
pub trait PtyProcess: Send + Sync {
    fn pid(&self) -> u32;
    /// 지금 앞에서 도는 프로세스 이름(node-pty `process`). 모르면 `None`이다
    fn foreground_process_name(&self) -> Option<String>;
    fn columns(&self) -> u16;
    fn rows(&self) -> u16;
    fn write(&self, data: &str) -> Result<(), String>;
    fn resize(&self, columns: u16, rows: u16) -> Result<(), String>;
    fn kill(&self) -> Result<(), String>;
}

pub trait PtySpawner: Send + Sync {
    fn spawn(
        &self,
        request: PtySpawnRequest,
        callbacks: PtyEventCallbacks,
    ) -> Result<Arc<dyn PtyProcess>, String>;
}
