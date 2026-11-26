//! task 수정·상태 변경·컬럼 이동, 프로젝트 색상, 빠른 검색 목록.

use std::sync::{Arc, Mutex};

use kanvibe_db::repositories::projects::find_project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority, TaskStatus};

use super::fakes::KanbanFixture;
use super::{add_project, add_task, stored_task, tmux_session, with_branch};
use crate::kanban::TaskUpdate;

fn project_color(fixture: &KanbanFixture, project_id: &str) -> Option<String> {
    find_project(&fixture.database().connection(), project_id)
        .unwrap()
        .unwrap()
        .color
}

#[test]
fn editing_task_content_updates_the_board() {
    let fixture = KanbanFixture::new();
    let task = add_task(&fixture, "Old title", TaskStatus::Todo, |_| {});
    let update = TaskUpdate {
        description: Some(Some("Fresh description".into())),
        ..TaskUpdate::default()
    };

    let updated = fixture
        .service
        .update_task(&task.id, &update)
        .unwrap()
        .unwrap();

    assert_eq!(updated.description.as_deref(), Some("Fresh description"));
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn changing_the_project_color_updates_the_board() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);

    fixture
        .service
        .update_project_color(&project.id, "#93C5FD")
        .unwrap();

    assert_eq!(
        project_color(&fixture, &project.id).as_deref(),
        Some("#93C5FD")
    );
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn project_color_is_written_to_the_shared_state_file_before_the_database() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    fixture.responses.returning("/workspace/repo/.git");
    let colors_seen_by_file_write = Arc::new(Mutex::new(Vec::new()));
    let (seen, database, project_id) = (
        Arc::clone(&colors_seen_by_file_write),
        Arc::clone(&fixture.db.database),
        project.id.clone(),
    );
    *fixture.state.project_color_observer.lock().unwrap() = Some(Arc::new(move |_: &str| {
        let stored = find_project(&database.connection(), &project_id)
            .unwrap()
            .unwrap();
        seen.lock().unwrap().push(stored.color);
    }));

    fixture
        .service
        .update_project_color(&project.id, "#0064FF")
        .unwrap();

    assert_eq!(colors_seen_by_file_write.lock().unwrap().clone(), [None]);
    assert_eq!(
        project_color(&fixture, &project.id).as_deref(),
        Some("#0064FF")
    );
}

#[test]
fn color_spreads_only_to_projects_on_the_same_host_and_git_common_directory() {
    let fixture = KanbanFixture::new();
    let root = add_project(&fixture, "/workspace/repo_%", None);
    let linked = add_project(&fixture, "/elsewhere/custom-tree", None);
    let neighbor = add_project(&fixture, "/workspace/repo_%-other", None);
    let remote = add_project(&fixture, "/workspace/repo_%", Some("prod"));
    fixture.responses.set(|call| {
        let is_other = call.command.contains("repo_%-other");
        Ok(if is_other {
            "/workspace/repo_%-other/.git"
        } else {
            "/workspace/repo_%/.git"
        }
        .into())
    });

    fixture
        .service
        .update_project_color(&linked.id, "#0064FF")
        .unwrap();

    let colors: Vec<Option<String>> = [&root, &linked, &neighbor, &remote]
        .iter()
        .map(|project| project_color(&fixture, &project.id))
        .collect();
    let blue = Some("#0064FF".to_string());
    assert_eq!(colors, [blue.clone(), blue, None, None]);
    let written_paths: Vec<String> = fixture
        .state
        .writes_of("project-color")
        .into_iter()
        .map(|write| write.repo_path)
        .collect();
    assert_eq!(
        written_paths,
        ["/workspace/repo_%", "/elsewhere/custom-tree"]
    );
    assert!(
        fixture
            .runner
            .calls()
            .iter()
            .all(|call| call.ssh_host.as_deref() != Some("prod"))
    );
}

#[test]
fn moving_to_another_column_only_changes_the_status() {
    let fixture = KanbanFixture::new();
    let task = add_task(&fixture, "task-a", TaskStatus::Todo, |task| {
        tmux_session(task, "repo-a")
    });

    fixture
        .service
        .move_task_to_column(&task.id, TaskStatus::Review)
        .unwrap();

    let moved = stored_task(&fixture, &task.id).unwrap();
    assert_eq!(moved.status, TaskStatus::Review);
    assert_eq!(
        KanbanTask {
            status: TaskStatus::Todo,
            updated_at: task.updated_at.clone(),
            ..moved
        },
        task
    );
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn status_change_of_a_feature_task_without_worktree_leaves_the_project_root_state_alone() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    let task = add_task(
        &fixture,
        "feature",
        TaskStatus::Todo,
        with_branch(&project, "feature/without-worktree", None),
    );

    let updated = fixture
        .service
        .update_task_status(&task.id, TaskStatus::Review)
        .unwrap()
        .unwrap();

    assert_eq!(updated.status, TaskStatus::Review);
    assert!(
        fixture
            .state
            .writes_of("status")
            .iter()
            .all(|write| write.repo_path != "/workspace/repo")
    );
    assert_eq!(fixture.board_update_count(), 1);
}

fn add_done_candidate(fixture: &KanbanFixture, with_project: bool) -> KanbanTask {
    let project = add_project(fixture, "/remote/repo", Some("remote-host"));
    add_task(fixture, "Remote cleanup", TaskStatus::Review, |task| {
        if with_project {
            with_branch(&project, "feature/preserve-resources", None)(task);
        }
        tmux_session(task, "repo-fix-done");
        task.worktree_path = Some("/remote/repo__worktrees/fix-done".into());
        task.ssh_host = Some("remote-host".into());
    })
}

fn assert_resources_preserved(fixture: &KanbanFixture, task: &KanbanTask) {
    let stored = stored_task(fixture, &task.id).unwrap();
    assert_eq!(stored.status, TaskStatus::Done);
    assert_eq!(stored.session_type, Some(SessionType::Tmux));
    assert_eq!(stored.session_name.as_deref(), Some("repo-fix-done"));
    assert_eq!(
        stored.worktree_path.as_deref(),
        Some("/remote/repo__worktrees/fix-done")
    );
    assert!(fixture.terminal.detached.lock().unwrap().is_empty());
    assert!(
        fixture
            .worktrees
            .session_removals
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(
        fixture
            .worktrees
            .worktree_removals
            .lock()
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn done_status_saves_first_and_keeps_resource_fields() {
    let fixture = KanbanFixture::new();
    let task = add_done_candidate(&fixture, false);

    fixture
        .service
        .update_task_status(&task.id, TaskStatus::Done)
        .unwrap();

    assert_resources_preserved(&fixture, &task);
}

#[test]
fn done_status_never_cleans_up_session_worktree_or_branch() {
    let fixture = KanbanFixture::new();
    let task = add_done_candidate(&fixture, true);

    fixture
        .service
        .update_task_status(&task.id, TaskStatus::Done)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    assert_resources_preserved(&fixture, &task);
}

#[test]
fn moving_to_done_saves_status_first_and_keeps_resource_fields() {
    let fixture = KanbanFixture::new();
    let task = add_done_candidate(&fixture, false);

    fixture
        .service
        .move_task_to_column(&task.id, TaskStatus::Done)
        .unwrap();

    assert_resources_preserved(&fixture, &task);
}

#[test]
fn moving_to_done_never_cleans_up_session_worktree_or_branch() {
    let fixture = KanbanFixture::new();
    let task = add_done_candidate(&fixture, true);

    fixture
        .service
        .move_task_to_column(&task.id, TaskStatus::Done)
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    assert_resources_preserved(&fixture, &task);
}

#[test]
fn failed_done_move_rolls_back_to_the_previous_status_and_resources() {
    let fixture = KanbanFixture::new();
    let task = add_done_candidate(&fixture, false);
    // TS 테스트의 `update` 실패 주입 자리. Done으로 바꾸는 저장만 실패시킨다
    fixture
        .database()
        .connection()
        .execute_batch(
            "CREATE TRIGGER reject_done BEFORE UPDATE ON kanban_tasks WHEN NEW.status = 'done' \
             BEGIN SELECT RAISE(ABORT, 'done move failed'); END;",
        )
        .unwrap();

    let error = fixture
        .service
        .move_task_to_column(&task.id, TaskStatus::Done)
        .unwrap_err();

    assert!(error.message.contains("done move failed"));
    let stored = stored_task(&fixture, &task.id).unwrap();
    assert_eq!(stored.status, TaskStatus::Review);
    assert_eq!(stored.session_name.as_deref(), Some("repo-fix-done"));
    assert_eq!(
        stored.worktree_path.as_deref(),
        Some("/remote/repo__worktrees/fix-done")
    );
    assert_eq!(stored.ssh_host.as_deref(), Some("remote-host"));
    assert!(
        fixture
            .worktrees
            .session_removals
            .lock()
            .unwrap()
            .is_empty()
    );
    assert_eq!(fixture.board_update_count(), 1);
}

#[test]
fn quick_search_lists_done_tasks_with_their_project_name() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/kanvibe", None);
    let active = add_task(
        &fixture,
        "Active task",
        TaskStatus::Progress,
        with_branch(&project, "dev", None),
    );
    let done = add_task(
        &fixture,
        "Done task",
        TaskStatus::Done,
        with_branch(&project, "feat/done-search", None),
    );
    // TS 테스트처럼 두 task의 시각을 같게 둔다. 저장 시각은 초 단위라, 두 insert 사이에 초가 바뀌면 순서가 달라진다
    fixture
        .database()
        .connection()
        .execute_batch(
            "UPDATE kanban_tasks SET created_at = '2026-05-02 00:00:00', updated_at = '2026-05-02 00:00:00'",
        )
        .unwrap();

    let searchable = fixture.service.get_searchable_tasks().unwrap();

    let summary: Vec<(String, Option<String>, Option<String>, TaskStatus)> = searchable
        .iter()
        .map(|task| {
            (
                task.id.clone(),
                task.branch_name.clone(),
                task.project_name.clone(),
                task.status,
            )
        })
        .collect();
    let kanvibe = Some("kanvibe".to_string());
    assert_eq!(
        summary,
        [
            (
                active.id,
                Some("dev".into()),
                kanvibe.clone(),
                TaskStatus::Progress
            ),
            (
                done.id,
                Some("feat/done-search".into()),
                kanvibe,
                TaskStatus::Done
            ),
        ]
    );
    assert!(searchable.iter().all(|task| task.ssh_host.is_none()));
}

fn add_feature_task_with_worktree(
    fixture: &KanbanFixture,
    description: Option<&str>,
) -> KanbanTask {
    let project = add_project(fixture, "/workspace/repo", None);
    add_task(fixture, "알림 회귀 수정", TaskStatus::Todo, |task| {
        with_branch(
            &project,
            "fix/notifications",
            Some("/workspace/repo-worktrees/task-1"),
        )(task);
        task.description = description.map(str::to_string);
    })
}

fn single_metadata_write(fixture: &KanbanFixture) -> (String, String) {
    let writes = fixture.state.writes_of("metadata");
    assert_eq!(writes.len(), 1);
    (writes[0].repo_path.clone(), writes[0].content.clone())
}

#[test]
fn changing_the_description_writes_the_shared_task_file() {
    let fixture = KanbanFixture::new();
    let task = add_feature_task_with_worktree(&fixture, None);
    let update = TaskUpdate {
        description: Some(Some("결제 실패 로그 원인 추적".into())),
        ..TaskUpdate::default()
    };

    fixture.service.update_task(&task.id, &update).unwrap();

    let (path, content) = single_metadata_write(&fixture);
    assert_eq!(path, "/workspace/repo-worktrees/task-1");
    assert!(content.contains("description=Some(Some(\"결제 실패 로그 원인 추적\"))"));
}

#[test]
fn clearing_the_description_writes_null_so_other_devices_clear_it() {
    let fixture = KanbanFixture::new();
    let task = add_feature_task_with_worktree(&fixture, Some("결제 실패 로그 원인 추적"));
    let update = TaskUpdate {
        description: Some(None),
        ..TaskUpdate::default()
    };

    fixture.service.update_task(&task.id, &update).unwrap();

    let (_, content) = single_metadata_write(&fixture);
    assert!(content.contains("description=Some(None)"));
}

#[test]
fn feature_task_priority_is_written_to_the_task_file() {
    let fixture = KanbanFixture::new();
    let task = add_feature_task_with_worktree(&fixture, Some("설명"));
    let update = TaskUpdate {
        priority: Some(Some(TaskPriority::High)),
        ..TaskUpdate::default()
    };

    fixture.service.update_task(&task.id, &update).unwrap();

    let (path, content) = single_metadata_write(&fixture);
    assert_eq!(path, "/workspace/repo-worktrees/task-1");
    assert!(content.contains("priority=Some(Some(\"high\"))"));
}

#[test]
fn project_root_task_priority_is_written_to_the_project_file() {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, "/workspace/repo", None);
    let task = add_task(
        &fixture,
        "main",
        TaskStatus::Todo,
        with_branch(&project, "main", Some("/workspace/repo")),
    );
    let update = TaskUpdate {
        priority: Some(Some(TaskPriority::Medium)),
        ..TaskUpdate::default()
    };

    fixture.service.update_task(&task.id, &update).unwrap();

    let priority_writes = fixture.state.writes_of("project-priority");
    assert_eq!(priority_writes[0].repo_path, "/workspace/repo");
    assert_eq!(priority_writes[0].content, "Some(\"medium\")");
}

#[test]
fn title_only_edits_leave_the_shared_files_alone() {
    let fixture = KanbanFixture::new();
    let task = add_feature_task_with_worktree(&fixture, Some("결제 실패 로그 원인 추적"));
    let update = TaskUpdate {
        title: Some("알림 회귀 재수정".into()),
        ..TaskUpdate::default()
    };

    fixture.service.update_task(&task.id, &update).unwrap();

    assert!(fixture.state.writes.lock().unwrap().is_empty());
}
