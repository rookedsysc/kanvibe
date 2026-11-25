//! TS `projectService sync deleted paths`, `projectService.deleteProject`

use std::sync::Arc;

use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType};

use super::fakes::{Gate, Harness, wait_until, worktree};

const REPO: &str = "/workspace/api";
const REMOVED_PATH: &str = "/workspace/api__worktrees/removed";

struct CleanupFixture {
    harness: Harness,
    project: Project,
    root: KanbanTask,
    removed: KanbanTask,
}

fn cleanup_fixture(ssh_host: Option<&str>) -> CleanupFixture {
    let harness = Harness::new();
    let project = harness.project("api", REPO, ssh_host);
    let root = harness.task(|task| {
        task.title = "main".into();
        task.project_id = Some(project.id.clone());
        task.branch_name = Some("main".into());
        task.base_branch = Some("main".into());
        task.worktree_path = Some(REPO.into());
        task.session_type = Some(SessionType::Tmux);
    });
    let removed = harness.task(|task| {
        task.title = "removed".into();
        task.project_id = Some(project.id.clone());
        task.branch_name = Some("removed".into());
        task.base_branch = Some("main".into());
        task.worktree_path = Some(REMOVED_PATH.into());
        task.session_type = Some(SessionType::Tmux);
    });
    harness
        .repositories
        .returning_worktrees(vec![worktree(REPO, "main")]);
    CleanupFixture {
        harness,
        project,
        root,
        removed,
    }
}

fn task_ids(harness: &Harness) -> Vec<String> {
    let mut ids: Vec<String> = harness.tasks().into_iter().map(|task| task.id).collect();
    ids.sort();
    ids
}

// parity: TS-main-projectService-27113e
#[test]
fn deleted_project_is_removed_with_its_tasks_and_not_recreated() {
    let fixture = cleanup_fixture(None);
    fixture.harness.repositories.on_path_check(|_, _| Ok(true));

    let result = fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(fixture.harness.projects().is_empty());
    assert!(fixture.harness.tasks().is_empty());
    assert!(result.has_changed);
    assert!(
        fixture
            .harness
            .state_files
            .persisted
            .lock()
            .unwrap()
            .is_empty()
    );
}

// parity: TS-main-projectService-5e6d09
#[test]
fn reading_the_board_first_does_not_repair_a_deleted_project_path() {
    let fixture = cleanup_fixture(None);
    fixture.harness.repositories.on_path_check(|_, _| Ok(true));

    fixture.harness.service.get_all_projects().unwrap();
    fixture.harness.service.join_background_jobs();
    assert!(
        fixture
            .harness
            .state_files
            .persisted
            .lock()
            .unwrap()
            .is_empty()
    );
    fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(fixture.harness.projects().is_empty());
    assert!(fixture.harness.tasks().is_empty());
}

/// `/slow/one`의 경로 확인과 worktree 조회를 각각 관문에 묶는다
fn block_slow_host(harness: &Harness) -> (Arc<Gate>, Arc<Gate>) {
    let slow_path_check = Arc::new(Gate::default());
    let slow_worktree_list = Arc::new(Gate::default());
    let path_gate = Arc::clone(&slow_path_check);
    harness.repositories.on_path_check(move |path, _| {
        if path == "/slow/one" {
            path_gate.wait();
        }
        Ok(false)
    });
    let list_gate = Arc::clone(&slow_worktree_list);
    harness.repositories.on_worktree_list(move |repo_path, _| {
        if repo_path == "/slow/one" {
            list_gate.wait();
            return Vec::new();
        }
        vec![worktree(repo_path, "main")]
    });
    (slow_path_check, slow_worktree_list)
}

// parity: TS-main-projectService-707478
#[test]
fn slow_host_does_not_hold_other_hosts_and_same_host_waits_its_turn() {
    let harness = Harness::new();
    harness.project("slow-1", "/slow/one", Some("slow-host"));
    harness.project("slow-2", "/slow/two", Some("slow-host"));
    harness.project("other-1", "/other/one", Some("other-host"));
    harness.project("api", REPO, None);
    let (slow_path_check, slow_worktree_list) = block_slow_host(&harness);

    std::thread::scope(|scope| {
        let syncing = scope.spawn(|| harness.service.sync_registered_project_worktrees(None));
        let checked = || harness.repositories.checked_paths();
        wait_until(|| {
            ["/slow/one", "/other/one", REPO]
                .iter()
                .all(|path| checked().contains(&path.to_string()))
        });
        assert!(!checked().contains(&"/slow/two".to_string()));
        slow_path_check.open();

        let listed = || harness.repositories.listed_repo_paths();
        wait_until(|| {
            ["/slow/one", "/other/one", REPO]
                .iter()
                .all(|path| listed().contains(&path.to_string()))
        });
        assert!(!listed().contains(&"/slow/two".to_string()));
        slow_worktree_list.open();
        syncing.join().unwrap().unwrap();
    });
    assert!(
        harness
            .repositories
            .listed_repo_paths()
            .contains(&"/slow/two".to_string())
    );
}

// parity: TS-main-projectService-609b0c
#[test]
fn only_deleted_worktrees_are_cleaned_even_if_git_still_lists_them() {
    let fixture = cleanup_fixture(None);
    fixture
        .harness
        .repositories
        .on_path_check(|path, _| Ok(path == REMOVED_PATH));
    fixture.harness.repositories.returning_worktrees(vec![
        worktree(REPO, "main"),
        worktree(REMOVED_PATH, "removed"),
    ]);

    let result = fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert_eq!(
        task_ids(&fixture.harness),
        std::slice::from_ref(&fixture.root.id)
    );
    assert_eq!(fixture.harness.projects().len(), 1);
    assert!(result.has_changed);
    assert!(result.registered_worktrees.is_empty());
    assert!(fixture.harness.sessions.detached.lock().unwrap().is_empty());
    assert!(
        !fixture
            .harness
            .state_files
            .persisted_paths()
            .contains(&REMOVED_PATH.to_string())
    );
}

// parity: TS-main-projectService-7264a8
#[test]
fn failed_worktree_listing_and_pathless_tasks_are_not_deletion_evidence() {
    let fixture = cleanup_fixture(None);
    let pathless = fixture.harness.task(|task| {
        task.project_id = Some(fixture.project.id.clone());
        task.branch_name = Some("main".into());
    });
    fixture.harness.repositories.returning_worktrees(Vec::new());

    fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    let mut expected = vec![
        fixture.root.id.clone(),
        fixture.removed.id.clone(),
        pathless.id,
    ];
    expected.sort();
    assert_eq!(task_ids(&fixture.harness), expected);
    assert_eq!(fixture.harness.projects().len(), 1);
}

// parity: TS-main-projectService-762865
#[test]
fn worktree_moved_by_git_keeps_its_task_id_and_metadata() {
    let fixture = cleanup_fixture(None);
    let moved_path = "/workspace/api__worktrees/moved";
    let mut removed = fixture.removed.clone();
    removed.pr_url = Some("https://github.com/example/api/pull/1".into());
    removed.agent_type = Some("codex".into());
    kanvibe_db::repositories::tasks::update_task(
        &fixture.harness.db.database.connection(),
        &removed,
    )
    .unwrap();
    fixture
        .harness
        .repositories
        .on_path_check(|path, _| Ok(path == REMOVED_PATH));
    fixture.harness.repositories.returning_worktrees(vec![
        worktree(REPO, "main"),
        worktree(moved_path, "removed"),
    ]);

    fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    let moved = fixture.harness.find_task(&removed.id).unwrap();
    assert_eq!(moved.worktree_path.as_deref(), Some(moved_path));
    assert_eq!(
        moved.pr_url.as_deref(),
        Some("https://github.com/example/api/pull/1")
    );
    assert_eq!(moved.agent_type.as_deref(), Some("codex"));
}

// parity: TS-main-projectService-a77adb
#[test]
fn deleted_default_branch_worktree_is_not_recreated_and_root_task_stays() {
    let fixture = cleanup_fixture(None);
    let missing_root = "/workspace/api__worktrees/main";
    let mut root = fixture.root.clone();
    root.worktree_path = Some(missing_root.into());
    kanvibe_db::repositories::tasks::update_task(&fixture.harness.db.database.connection(), &root)
        .unwrap();
    fixture
        .harness
        .repositories
        .on_path_check(move |path, _| Ok(path == missing_root));
    fixture.harness.repositories.returning_worktrees(vec![
        kanvibe_process::git_operations::WorktreeEntry {
            path: REPO.into(),
            branch: String::new(),
            is_bare: true,
        },
        worktree(missing_root, "main"),
    ]);

    fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(task_ids(&fixture.harness).contains(&root.id));
    assert!(
        !fixture
            .harness
            .state_files
            .persisted_paths()
            .contains(&missing_root.to_string())
    );
}

// parity: TS-main-projectService-93390d
#[test]
fn task_without_ssh_host_is_checked_on_the_project_host() {
    let fixture = cleanup_fixture(Some("devbox"));
    fixture
        .harness
        .repositories
        .on_path_check(|path, host| Ok(path == REMOVED_PATH && host.is_none()));

    fixture
        .harness
        .service
        .sync_registered_project_worktrees(None)
        .unwrap();

    assert!(task_ids(&fixture.harness).contains(&fixture.removed.id));
    let checks = fixture
        .harness
        .repositories
        .path_checks
        .lock()
        .unwrap()
        .clone();
    assert!(checks.contains(&(REMOVED_PATH.to_string(), Some("devbox".to_string()))));
}

// parity: TS-main-projectService-7fba64
#[test]
fn records_are_kept_when_the_path_check_fails() {
    for reason in ["SSH connection refused", "EACCES"] {
        let fixture = cleanup_fixture(None);
        fixture
            .harness
            .repositories
            .on_path_check(move |_, _| Err(reason.to_string()));

        let result = fixture
            .harness
            .service
            .sync_registered_project_worktrees(None)
            .unwrap();

        assert_eq!(fixture.harness.tasks().len(), 2);
        assert_eq!(fixture.harness.projects().len(), 1);
        assert!(result.errors.join(" ").contains(reason));
        assert!(
            fixture
                .harness
                .state_files
                .persisted
                .lock()
                .unwrap()
                .is_empty()
        );
    }
}

// parity: TS-main-projectService-542fd6
#[test]
fn deleting_a_project_removes_its_task_records() {
    let fixture = cleanup_fixture(None);

    let is_deleted = fixture
        .harness
        .service
        .delete_project(&fixture.project.id)
        .unwrap();

    assert!(is_deleted);
    assert!(fixture.harness.tasks().is_empty());
    assert!(fixture.harness.projects().is_empty());
    assert_eq!(fixture.harness.events.board_update_count(), 1);
}

// parity: TS-main-projectService-0f8fab
#[test]
fn deleting_a_missing_project_deletes_nothing_and_does_not_refresh() {
    let fixture = cleanup_fixture(None);

    let is_deleted = fixture
        .harness
        .service
        .delete_project("missing-project")
        .unwrap();

    assert!(!is_deleted);
    assert_eq!(fixture.harness.tasks().len(), 2);
    assert_eq!(fixture.harness.events.board_update_count(), 0);
}

// parity: TS-main-projectService-b755c5
#[test]
fn explicit_sync_during_deletion_does_not_revive_the_root_task() {
    let harness = Harness::new();
    let project = harness.project("api", REPO, None);
    // 삭제가 시작된 뒤 끝나기 전에 sync가 들어온 상태를 만든다. Rust 삭제는 트랜잭션 하나라
    // 그 사이를 직접 잡을 수 없으므로, 삭제 중 표시를 남기는 것과 같은 상태에서 sync를 돌린다
    harness
        .service
        .root_repair_blocks
        .lock()
        .unwrap()
        .deleting
        .insert(project.id.clone());
    harness.service.get_all_projects().unwrap();

    harness
        .service
        .sync_registered_project_worktrees(Some(std::slice::from_ref(&project)))
        .unwrap();
    harness
        .service
        .root_repair_blocks
        .lock()
        .unwrap()
        .deleting
        .remove(&project.id);
    let is_deleted = harness.service.delete_project(&project.id).unwrap();

    assert!(is_deleted);
    assert!(harness.tasks().is_empty());
    assert!(harness.repositories.listed_repo_paths().is_empty());
}
