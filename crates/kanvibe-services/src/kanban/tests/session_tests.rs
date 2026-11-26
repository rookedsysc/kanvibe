//! task 세션 종류 변경과 일괄 변경.

use std::sync::atomic::Ordering;

use kanvibe_db::repositories::tasks::NewKanbanTask;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskStatus};

use super::fakes::{KanbanFixture, SessionRemoval};
use super::{add_project, add_task, stored_task, tmux_session};
use crate::board_notifier::{BoardEvent, TaskSessionChangedPayload};
use crate::error::ServiceError;
use crate::kanban::{SessionTypeChangeResult, SessionTypeChangeTargets};

fn pay_task_fields(project: &Project, session_name: &str) -> impl FnOnce(&mut NewKanbanTask) {
    let (project_id, session_name) = (project.id.clone(), session_name.to_string());
    move |task| {
        task.project_id = Some(project_id);
        task.branch_name = Some("feat/pay".into());
        task.base_branch = Some("main".into());
        task.worktree_path = Some("/repo__worktrees/feat-pay".into());
        tmux_session(task, &session_name);
    }
}

fn tmux_task_fixture() -> (KanbanFixture, Project, KanbanTask) {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/repo", None);
    let task = add_task(
        &fixture,
        "결제 실패 추적",
        TaskStatus::Done,
        pay_task_fields(&project, "repo-feat-pay"),
    );
    fixture
        .worktrees
        .set_session_name(|_, _| Ok("repo-feat-pay-zellij".into()));
    (fixture, project, task)
}

fn removals(fixture: &KanbanFixture) -> Vec<SessionRemoval> {
    fixture.worktrees.session_removals.lock().unwrap().clone()
}

#[test]
fn closing_the_old_session_removes_tmux_and_records_the_zellij_session() {
    let (fixture, _, task) = tmux_task_fixture();

    let changed = fixture
        .service
        .change_task_session_type(&task.id, SessionType::Zellij, true)
        .unwrap()
        .unwrap();

    assert_eq!(
        fixture.terminal.detached.lock().unwrap().clone(),
        [(task.id.clone(), "change-session-type".to_string())]
    );
    assert_eq!(
        removals(&fixture),
        [SessionRemoval {
            session_type: SessionType::Tmux,
            session_name: "repo-feat-pay".into(),
            ssh_host: None,
            should_throw_on_error: false,
        }]
    );
    assert_eq!(
        (changed.session_type, changed.session_name.as_deref()),
        (Some(SessionType::Zellij), Some("repo-feat-pay-zellij"))
    );
    let stored = stored_task(&fixture, &task.id).unwrap();
    assert_eq!(stored.session_name.as_deref(), Some("repo-feat-pay-zellij"));
    let session_write = fixture.state.writes_of("session")[0].clone();
    assert_eq!(session_write.repo_path, "/repo__worktrees/feat-pay");
    assert_eq!(session_write.content, "zellij:repo-feat-pay-zellij");
    assert!(fixture.events().contains(&BoardEvent::TaskSessionChanged(
        TaskSessionChangedPayload { task_id: task.id }
    )));
}

#[test]
fn keeping_the_old_session_only_detaches_and_changes_the_type() {
    let (fixture, _, task) = tmux_task_fixture();

    let changed = fixture
        .service
        .change_task_session_type(&task.id, SessionType::Zellij, false)
        .unwrap()
        .unwrap();

    assert_eq!(fixture.terminal.detached.lock().unwrap().len(), 1);
    assert!(removals(&fixture).is_empty());
    assert_eq!(changed.session_type, Some(SessionType::Zellij));
}

#[test]
fn the_same_session_type_leaves_the_session_untouched() {
    let (fixture, _, task) = tmux_task_fixture();

    let result = fixture
        .service
        .change_task_session_type(&task.id, SessionType::Tmux, true)
        .unwrap()
        .unwrap();

    assert_eq!(result.session_name.as_deref(), Some("repo-feat-pay"));
    assert!(fixture.terminal.detached.lock().unwrap().is_empty());
    assert!(removals(&fixture).is_empty());
    assert!(fixture.events().is_empty());
}

#[test]
fn a_task_without_a_session_has_nothing_to_change() {
    let fixture = KanbanFixture::new();
    let task = add_task(&fixture, "세션 없음", TaskStatus::Done, |_| {});

    assert_eq!(
        fixture
            .service
            .change_task_session_type(&task.id, SessionType::Zellij, true),
        Ok(None)
    );
    assert!(
        fixture
            .worktrees
            .session_creations
            .lock()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn bulk_targets_are_tasks_with_another_session_type_including_done_with_live_count_and_hosts() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/repo", None);
    add_task(
        &fixture,
        "done-tmux",
        TaskStatus::Done,
        pay_task_fields(&project, "repo-feat-pay"),
    );
    add_task(&fixture, "remote-herdr", TaskStatus::Progress, |task| {
        pay_task_fields(&project, "remote")(task);
        task.session_type = Some(SessionType::Herdr);
        task.ssh_host = Some("build-host".into());
    });
    add_task(&fixture, "already-zellij", TaskStatus::Progress, |task| {
        pay_task_fields(&project, "repo-feat-pay")(task);
        task.session_type = Some(SessionType::Zellij);
    });
    add_task(&fixture, "no-project", TaskStatus::Progress, |task| {
        pay_task_fields(&project, "repo-feat-pay")(task);
        task.project_id = None;
    });
    add_task(&fixture, "no-session-type", TaskStatus::Progress, |task| {
        pay_task_fields(&project, "repo-feat-pay")(task);
        task.session_type = None;
    });
    *fixture.worktrees.alive_session_names.lock().unwrap() = vec!["repo-feat-pay".into()];

    let targets = fixture
        .service
        .get_session_type_change_targets(SessionType::Zellij)
        .unwrap();

    assert_eq!(
        targets,
        SessionTypeChangeTargets {
            task_count: 2,
            live_session_count: 1,
            ssh_hosts: vec![None, Some("build-host".into())]
        }
    );
    assert_eq!(fixture.worktrees.alive_checks.load(Ordering::SeqCst), 2);
}

#[test]
fn bulk_change_closes_only_sessions_of_tasks_that_switched_and_counts_failures() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/repo", None);
    let task_a = add_task(
        &fixture,
        "task-a",
        TaskStatus::Done,
        pay_task_fields(&project, "repo-a"),
    );
    let task_b = add_task(
        &fixture,
        "task-b",
        TaskStatus::Done,
        pay_task_fields(&project, "repo-b"),
    );
    add_task(&fixture, "already-zellij", TaskStatus::Done, |task| {
        pay_task_fields(&project, "repo-z")(task);
        task.session_type = Some(SessionType::Zellij);
    });
    fixture
        .worktrees
        .set_session_name(|creation, call_number| match call_number {
            2 => Err(ServiceError::new("zellij 레이아웃 준비 실패")),
            _ => Ok(format!("{}-zellij", creation.branch_name)),
        });

    let result = fixture
        .service
        .change_all_task_session_types(SessionType::Zellij, true)
        .unwrap();

    assert_eq!(
        result,
        SessionTypeChangeResult {
            changed_count: 1,
            failed_count: 1
        }
    );
    // 두 task가 동시에 전환되므로 어느 쪽이 두 번째 세션 생성을 맞을지는 정해지지 않는다. 성공한 쪽의 옛 세션만 정리돼야 한다
    let switched = [&task_a, &task_b]
        .into_iter()
        .find(|task| {
            stored_task(&fixture, &task.id).unwrap().session_type == Some(SessionType::Zellij)
        })
        .unwrap();
    let removed = removals(&fixture)
        .into_iter()
        .map(|removal| removal.session_name);
    assert!(removed.eq(switched.session_name.clone()));
}
