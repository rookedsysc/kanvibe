//! 로그인 CLI를 띄우는 PTY의 경계. PTY와 VT 처리는 `kanvibe-terminal`(M3)이 맡고, 이 crate는 아래 trait만 부른다.
//! 로그인 세션은 태스크 터미널이 아니라서 세션 수명은 로그인 브리지가 따로 관리한다.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::usage::context::UsageContext;
use crate::usage::provider_cli::{create_provider_cli_environment, get_provider_login_command};
use crate::usage::provider_config_dir::config_dir_spec;
use crate::usage::types::AiUsageProvider;

/// PTY 하나를 띄우는 요청. TS `pty.spawn(command, args, { name, cols, rows, cwd, env })`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginTerminalRequest {
    pub program: String,
    pub args: Vec<String>,
    /// 터미널 종류(`TERM`에 해당하는 이름)
    pub terminal_name: String,
    pub columns: u16,
    pub rows: u16,
    pub working_directory: String,
    /// 이 값만 자식에게 넘긴다
    pub environment: BTreeMap<String, String>,
}

/// PTY가 내보내는 출력과 종료. PTY 구현은 `spawn`이 돌아온 뒤 자기 읽기 스레드에서 부른다
pub struct LoginTerminalCallbacks {
    pub on_output: Box<dyn Fn(&str) + Send + Sync>,
    pub on_exit: Box<dyn Fn(i32) + Send + Sync>,
}

/// 떠 있는 PTY. 네이티브 fd가 먼저 닫혀 쓰기·크기 바꾸기·종료가 실패할 수 있으므로 오류를 돌려준다
pub trait LoginTerminal: Send + Sync {
    fn write(&self, data: &str) -> Result<(), String>;
    fn resize(&self, columns: u16, rows: u16) -> Result<(), String>;
    fn kill(&self) -> Result<(), String>;
}

/// `kanvibe-terminal`이 구현한다. 콜백은 `spawn` 안에서 동기로 부르지 않는다(브리지가 등록 잠금을 쥔 채 부른다)
pub trait LoginTerminalSpawner: Send + Sync {
    fn spawn(
        &self,
        request: LoginTerminalRequest,
        callbacks: LoginTerminalCallbacks,
    ) -> Result<Arc<dyn LoginTerminal>, String>;
}

const LOGIN_TERMINAL_NAME: &str = "xterm-color";

/// 계정 하나의 로그인 PTY 요청. 작업 디렉터리는 CLI 환경의 HOME이다
pub fn build_login_terminal_request(
    context: &UsageContext,
    provider: AiUsageProvider,
    account_root: &str,
    extra_args: &[&str],
    columns: u16,
    rows: u16,
) -> LoginTerminalRequest {
    let command = get_provider_login_command(provider);
    let environment =
        create_provider_cli_environment(context, config_dir_spec(provider), account_root);
    LoginTerminalRequest {
        program: command.command,
        args: command
            .args
            .into_iter()
            .chain(extra_args.iter().map(|arg| arg.to_string()))
            .collect(),
        terminal_name: LOGIN_TERMINAL_NAME.to_string(),
        columns,
        rows,
        working_directory: environment.get("HOME").cloned().unwrap_or_default(),
        environment,
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    /// 호출을 기록하는 가짜 PTY. 지정하면 모든 조작이 EBADF처럼 실패한다
    #[derive(Default)]
    pub struct FakeTerminal {
        pub writes: Mutex<Vec<String>>,
        pub resizes: Mutex<Vec<(u16, u16)>>,
        pub kill_count: Mutex<usize>,
        pub is_broken: AtomicBool,
    }

    impl FakeTerminal {
        fn check(&self) -> Result<(), String> {
            if self.is_broken.load(Ordering::SeqCst) {
                return Err("ioctl(2) failed, EBADF".to_string());
            }
            Ok(())
        }

        pub fn kills(&self) -> usize {
            *self.kill_count.lock().unwrap()
        }
    }

    impl LoginTerminal for FakeTerminal {
        fn write(&self, data: &str) -> Result<(), String> {
            self.check()?;
            self.writes.lock().unwrap().push(data.to_string());
            Ok(())
        }

        fn resize(&self, columns: u16, rows: u16) -> Result<(), String> {
            self.check()?;
            self.resizes.lock().unwrap().push((columns, rows));
            Ok(())
        }

        fn kill(&self) -> Result<(), String> {
            *self.kill_count.lock().unwrap() += 1;
            self.check()
        }
    }

    /// 띄운 PTY 하나의 요청, 가짜 PTY, 콜백
    pub type SpawnedTerminal = (
        LoginTerminalRequest,
        Arc<FakeTerminal>,
        Arc<LoginTerminalCallbacks>,
    );

    /// 띄운 PTY와 그 콜백을 기록한다. 실패할 차례를 정해 둘 수 있다
    #[derive(Default)]
    pub struct FakeSpawner {
        pub spawned: Mutex<Vec<SpawnedTerminal>>,
        pub should_fail_next: AtomicBool,
    }

    impl FakeSpawner {
        pub fn count(&self) -> usize {
            self.spawned.lock().unwrap().len()
        }

        pub fn terminal(&self, index: usize) -> Arc<FakeTerminal> {
            self.spawned.lock().unwrap()[index].1.clone()
        }

        pub fn request(&self, index: usize) -> LoginTerminalRequest {
            self.spawned.lock().unwrap()[index].0.clone()
        }

        pub fn emit_output(&self, index: usize, data: &str) {
            let callbacks = self.spawned.lock().unwrap()[index].2.clone();
            (callbacks.on_output)(data);
        }

        pub fn emit_exit(&self, index: usize, exit_code: i32) {
            let callbacks = self.spawned.lock().unwrap()[index].2.clone();
            (callbacks.on_exit)(exit_code);
        }
    }

    impl LoginTerminalSpawner for FakeSpawner {
        fn spawn(
            &self,
            request: LoginTerminalRequest,
            callbacks: LoginTerminalCallbacks,
        ) -> Result<Arc<dyn LoginTerminal>, String> {
            if self.should_fail_next.swap(false, Ordering::SeqCst) {
                return Err("secret /home/user/token".to_string());
            }
            let terminal = Arc::new(FakeTerminal::default());
            self.spawned
                .lock()
                .unwrap()
                .push((request, terminal.clone(), Arc::new(callbacks)));
            Ok(terminal)
        }
    }
}
