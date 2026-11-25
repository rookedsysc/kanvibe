//! 새 worktree·터미널 연결 전에 KanVibe hook 파일을 설치하고, provider 검증은 뒤에서 이어 가게 예약한다.
//! 설치 실패는 task 생성을 막지 않고 화면에 실패 이벤트로만 알린다.

use std::sync::Arc;
use std::sync::mpsc;

use super::KanbanService;
use super::ports::TaskHookInstallation;
use crate::board_notifier::{BoardNotifier, TaskHookInstallFailedPayload};

/// hook을 설치할 task 정보(TS `Pick<KanbanTask, "id" | "title" | "sshHost">`)
#[derive(Debug, Clone)]
pub(super) struct HookTargetTask {
    pub id: String,
    pub title: String,
    pub ssh_host: Option<String>,
}

/// 설치 실패를 로그로 남기고 화면에 알린다
fn report_task_hook_install_failure(
    board: &BoardNotifier,
    target_path: &str,
    task: &HookTargetTask,
    error_message: &str,
    log_message: &str,
) {
    eprintln!(
        "{log_message}: {{ taskId: {:?}, taskTitle: {:?}, targetPath: {target_path:?}, sshHost: {:?}, error: {error_message:?} }}",
        task.id, task.title, task.ssh_host
    );
    board.broadcast_task_hook_install_failed(TaskHookInstallFailedPayload {
        task_id: task.id.clone(),
        task_title: task.title.clone(),
        error: error_message.to_string(),
    });
}

/// 검증이 성공하면 보드를 다시 그리고, 실패하면 설치 실패로 알린다
fn schedule_task_hook_verification(
    hooks: &dyn TaskHookInstallation,
    board: &Arc<BoardNotifier>,
    target_path: &str,
    task: &HookTargetTask,
) {
    let success_board = Arc::clone(board);
    let failure_board = Arc::clone(board);
    let failed_task = task.clone();
    let failed_path = target_path.to_string();
    hooks.schedule_hooks_verification(
        target_path,
        &task.id,
        task.ssh_host.as_deref(),
        Box::new(move || success_board.broadcast_board_update()),
        Box::new(move |error| {
            report_task_hook_install_failure(
                &failure_board,
                &failed_path,
                &failed_task,
                &error,
                "hooks 백그라운드 검증 실패",
            );
        }),
    );
}

/// hook 파일을 쓰고 성공하면 검증을 예약한다. 실패는 알리기만 하고 호출자에게 넘기지 않는다
pub(super) fn install_task_hook_files_immediately(
    hooks: &dyn TaskHookInstallation,
    board: &Arc<BoardNotifier>,
    target_path: &str,
    task: &HookTargetTask,
    failure_log_message: &str,
) {
    match hooks.install_hook_files(target_path, &task.id, task.ssh_host.as_deref()) {
        Ok(()) => schedule_task_hook_verification(hooks, board, target_path, task),
        Err(error) => {
            report_task_hook_install_failure(board, target_path, task, &error, failure_log_message)
        }
    }
}

impl KanbanService {
    /// 터미널 연결 전 hook 파일 설치를 정해진 시간까지만 기다린다. 넘으면 설치는 뒤에서 마저 끝낸다
    pub(super) fn install_task_hook_files_before_terminal_attach(
        &self,
        target_path: &str,
        task: HookTargetTask,
    ) {
        let (finished_sender, finished_receiver) = mpsc::channel::<()>();
        let hooks = Arc::clone(&self.hooks);
        let board = Arc::clone(&self.board);
        let target_path = target_path.to_string();
        std::thread::spawn(move || {
            install_task_hook_files_immediately(
                hooks.as_ref(),
                &board,
                &target_path,
                &task,
                "터미널 연결 전 hooks 동기 설치 실패",
            );
            let _ = finished_sender.send(());
        });
        let _ = finished_receiver.recv_timeout(self.timings.terminal_hook_install_pre_attach_wait);
    }
}
