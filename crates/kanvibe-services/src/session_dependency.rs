//! 세션 멀티플렉서 의존성 확인·설치와 원격 Herdr 서버 교체 묶음(TS `sessionDependencyService.ts`).
//! 확인·설치 로직은 kanvibe-process에 있고, 여기서는 화면이 부르는 네 메서드와 서버 교체(터미널 계층)를 잇는다.

use std::sync::Arc;

use kanvibe_domain::task::SessionType;
use kanvibe_process::remote_session_dependency::{
    HerdrServerHandoffPrompts, SessionDependencies, SessionDependencyStatus,
};

use crate::error::ServiceResult;
use crate::terminal_ports::TaskTerminalSessions;

pub struct SessionDependencyService {
    dependencies: Arc<SessionDependencies>,
    handoff_prompts: HerdrServerHandoffPrompts,
    terminal: Arc<dyn TaskTerminalSessions>,
}

impl SessionDependencyService {
    pub fn new(dependencies: SessionDependencies, terminal: Arc<dyn TaskTerminalSessions>) -> Self {
        Self::with_shared_dependencies(Arc::new(dependencies), terminal)
    }

    /// 터미널 브리지와 같은 확인 캐시를 쓴다. 설정 화면의 설치 결과와 연결 경로의 차단 기록이 한 곳에 남아야
    /// TS 모듈 전역 캐시처럼 두 경로가 같은 판정을 본다
    pub fn with_shared_dependencies(
        dependencies: Arc<SessionDependencies>,
        terminal: Arc<dyn TaskTerminalSessions>,
    ) -> Self {
        Self {
            dependencies,
            handoff_prompts: HerdrServerHandoffPrompts::default(),
            terminal,
        }
    }

    pub fn get_session_dependency_status(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus> {
        Ok(self.dependencies.status(session_type, ssh_host)?)
    }

    /// 설치한 뒤 다시 확인한 상태를 돌려준다
    pub fn install_session_dependency(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> ServiceResult<SessionDependencyStatus> {
        Ok(self
            .dependencies
            .install_and_read_status(session_type, ssh_host)?)
    }

    /// 원격 Herdr 서버 교체 확인은 앱이 실행되는 동안 서버마다 한 번만, 처음 요청한 창 하나만 묻는다
    pub fn claim_herdr_server_handoff_prompt(&self, ssh_host: &str, herdr_session: &str) -> bool {
        self.handoff_prompts.claim(ssh_host, herdr_session)
    }

    pub fn replace_herdr_server(&self, ssh_host: &str, herdr_session: &str) -> ServiceResult<()> {
        self.terminal
            .replace_herdr_remote_server(ssh_host, herdr_session)
            .map_err(crate::error::ServiceError::new)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use kanvibe_process::exec::ShellCommandRunner;
    use kanvibe_process::worktree::HerdrTaskWorkspaces;

    use super::*;
    use crate::test_support::ScriptedCommandRunner;

    #[derive(Default)]
    struct RecordingTerminal {
        replaced: Mutex<Vec<(String, String)>>,
    }

    impl HerdrTaskWorkspaces for RecordingTerminal {
        fn close_task_workspace(
            &self,
            _session_name: &str,
            _ssh_host: Option<&str>,
        ) -> Result<(), String> {
            Ok(())
        }

        fn is_task_alive(&self, _session_name: &str, _ssh_host: Option<&str>) -> bool {
            false
        }
    }

    impl TaskTerminalSessions for RecordingTerminal {
        fn detach_session(&self, _task_id: &str, _trigger_label: &str) {}

        fn replace_herdr_remote_server(
            &self,
            ssh_host: &str,
            herdr_session: &str,
        ) -> Result<(), String> {
            self.replaced
                .lock()
                .unwrap()
                .push((ssh_host.to_string(), herdr_session.to_string()));
            Ok(())
        }
    }

    #[test]
    fn claims_each_handoff_prompt_once_and_hands_replacement_to_the_terminal_layer() {
        let runner: Arc<dyn ShellCommandRunner> = Arc::new(ScriptedCommandRunner::returning(""));
        let terminal = Arc::new(RecordingTerminal::default());
        let service = SessionDependencyService::new(
            SessionDependencies::new(runner),
            Arc::clone(&terminal) as Arc<dyn TaskTerminalSessions>,
        );

        let is_first_claim = service.claim_herdr_server_handoff_prompt("dev", "kanvibe");
        let is_second_claim = service.claim_herdr_server_handoff_prompt("dev", "kanvibe");
        service.replace_herdr_server("dev", "kanvibe").unwrap();

        assert!(is_first_claim);
        assert!(!is_second_claim);
        assert_eq!(
            *terminal.replaced.lock().unwrap(),
            [("dev".to_string(), "kanvibe".to_string())]
        );
    }
}
