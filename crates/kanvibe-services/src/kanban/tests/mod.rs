//! 보드 서비스 테스트(TS `kanbanService.test.ts`).

mod creation_tests;
mod deletion_tests;
mod editing_tests;
mod fakes;
mod github_command_tests;
mod pull_request_tests;
mod pull_sync_tests;
mod session_tests;

use kanvibe_db::repositories::tasks::NewKanbanTask;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};

use self::fakes::KanbanFixture;
use crate::test_support::{new_project, new_task};

/// 테스트 프로젝트. 기본 브랜치는 `main`이다
fn add_project(fixture: &KanbanFixture, repo_path: &str, ssh_host: Option<&str>) -> Project {
    let mut project = new_project(repo_path.rsplit('/').next().unwrap_or("repo"), repo_path);
    project.ssh_host = ssh_host.map(str::to_string);
    fixture.db.insert_project(project)
}

/// 기본값 task에 필요한 필드만 바꿔 저장한다. TS 테스트처럼 지워진 프로젝트를 가리키는 task도 만들 수 있도록
/// 저장하는 동안만 외래 키 검사를 끈다
fn add_task(
    fixture: &KanbanFixture,
    title: &str,
    status: TaskStatus,
    customize: impl FnOnce(&mut NewKanbanTask),
) -> KanbanTask {
    let mut task = new_task(title, status);
    customize(&mut task);
    let set_foreign_keys = |sql: &str| fixture.database().connection().execute_batch(sql).unwrap();
    set_foreign_keys("PRAGMA foreign_keys = OFF");
    let inserted = fixture.db.insert_task(task);
    set_foreign_keys("PRAGMA foreign_keys = ON");
    inserted
}

/// 브랜치·worktree·세션을 가진 task 필드
fn with_branch(
    project: &Project,
    branch_name: &str,
    worktree_path: Option<&str>,
) -> impl FnOnce(&mut NewKanbanTask) {
    let project_id = project.id.clone();
    let branch_name = branch_name.to_string();
    let worktree_path = worktree_path.map(str::to_string);
    move |task| {
        task.project_id = Some(project_id);
        task.branch_name = Some(branch_name);
        task.worktree_path = worktree_path;
    }
}

fn stored_task(fixture: &KanbanFixture, task_id: &str) -> Option<KanbanTask> {
    kanvibe_db::repositories::tasks::find_task(&fixture.database().connection(), task_id).unwrap()
}

fn tmux_session(task: &mut NewKanbanTask, session_name: &str) {
    task.session_type = Some(SessionType::Tmux);
    task.session_name = Some(session_name.to_string());
}
