//! 서비스가 터미널 계층(M3 `kanvibe-terminal`)에 맡기는 일. 이 crate는 터미널 crate에 의존하지 않고 trait만 정의하며,
//! 터미널 crate가 구현해 앱 조립 시점에 넘긴다.

use kanvibe_process::worktree::HerdrTaskWorkspaces;

/// task가 쓰는 PTY·멀티플렉서 세션 조작(TS `lib/terminal.ts`의 `detachSession`, `replaceHerdrRemoteServer`).
/// Herdr 공유 서버의 task workspace 정리·생존 확인(`HerdrTaskWorkspaces`)도 같은 구현이 맡는다.
pub trait TaskTerminalSessions: HerdrTaskWorkspaces + Send + Sync {
    /// task에 붙은 KanVibe 소유 PTY를 모두 끊고 탭 목록을 잊는다. 멀티플렉서 세션 자체는 남는다
    fn detach_session(&self, task_id: &str, trigger_label: &str);

    /// 원격 Herdr 서버를 새 방식으로 띄운 서버로 바꾼다
    fn replace_herdr_remote_server(
        &self,
        ssh_host: &str,
        herdr_session: &str,
    ) -> Result<(), String>;
}
