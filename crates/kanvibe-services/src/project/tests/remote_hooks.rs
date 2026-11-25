//! TS `projectService remote hook and AI session support`

use kanvibe_domain::project::Project;
use kanvibe_domain::task::KanbanTask;
use kanvibe_process::hooks::providers::KanvibeHookProvider;

use super::fakes::{Harness, StatusCall, hook_status};
use crate::project::ports::{AiSessionListRequest, AiSessionScope};

const REMOTE_REPO: &str = "/remote/repo";
const REMOTE_WORKTREE: &str = "/remote/worktree";
const HOST: &str = "remote-host";

fn remote_task(
    harness: &Harness,
    worktree_path: &str,
    branch_name: Option<&str>,
) -> (Project, KanbanTask) {
    let project = harness.project("repo", REMOTE_REPO, Some(HOST));
    let task = harness.task(|task| {
        task.project_id = Some(project.id.clone());
        task.worktree_path = Some(worktree_path.into());
        task.branch_name = branch_name.map(str::to_string);
        task.base_branch = Some("main".into());
        task.ssh_host = Some(HOST.into());
    });
    (project, task)
}

fn status_targets(harness: &Harness) -> Vec<StatusCall> {
    harness.hooks.status_calls.lock().unwrap().clone()
}

fn assert_nothing_repaired(harness: &Harness) {
    assert!(harness.hooks.ensure_calls.lock().unwrap().is_empty());
    assert!(
        harness
            .hooks
            .provider_install_calls
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(harness.state_files.persisted.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-43712e
#[test]
fn detailed_hook_status_failure_is_not_hidden_as_success() {
    let harness = Harness::new();
    let (_, task) = remote_task(&harness, REMOTE_WORKTREE, None);
    *harness.hooks.status_error.lock().unwrap() = Some("inspection unavailable".into());

    let error = harness
        .service
        .get_task_hooks_status(&task.id, KanvibeHookProvider::Claude)
        .unwrap_err();

    assert_eq!(error.message, "inspection unavailable");
    assert_eq!(status_targets(&harness).len(), 1);
    assert!(harness.hooks.ensure_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-80e8c2
#[test]
fn project_hook_status_reads_the_existing_root_task_without_repairing() {
    for provider in [
        KanvibeHookProvider::Claude,
        KanvibeHookProvider::Gemini,
        KanvibeHookProvider::Codex,
    ] {
        let harness = Harness::new();
        let project = harness.project("repo", REMOTE_REPO, Some(HOST));
        let root = harness.root_task(&project, Some(REMOTE_REPO));

        let status = harness
            .service
            .get_project_hooks_status(&project.id, provider)
            .unwrap();

        assert_eq!(status, Some(hook_status(provider, false)));
        assert_eq!(harness.find_task(&root.id), Some(root));
        assert_nothing_repaired(&harness);
    }
}

// parity: TS-main-projectService-34c257
#[test]
fn status_reads_do_not_repair_hooks_even_when_all_providers_are_missing() {
    let harness = Harness::new();
    let (_, task) = remote_task(&harness, REMOTE_WORKTREE, None);

    let statuses: Vec<_> = KanvibeHookProvider::ALL
        .into_iter()
        .map(|provider| {
            harness
                .service
                .get_task_hooks_status(&task.id, provider)
                .unwrap()
        })
        .collect();

    let expected: Vec<_> = KanvibeHookProvider::ALL
        .into_iter()
        .map(|provider| Some(hook_status(provider, false)))
        .collect();
    assert_eq!(statuses, expected);
    assert_nothing_repaired(&harness);
}

// parity: TS-main-projectService-7c2922
#[test]
fn batch_hook_status_resolves_the_task_target_once() {
    let harness = Harness::new();
    let (_, task) = remote_task(&harness, REMOTE_WORKTREE, None);

    let statuses = harness
        .service
        .get_task_hooks_statuses(&task.id)
        .unwrap()
        .unwrap();

    assert_eq!(
        statuses.gemini,
        hook_status(KanvibeHookProvider::Gemini, true)
    );
    let calls = harness.hooks.statuses_calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        [(
            REMOTE_WORKTREE.to_string(),
            task.id,
            None,
            Some(HOST.to_string())
        )]
    );
    assert!(harness.hooks.ensure_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-682778
#[test]
fn default_branch_status_reads_the_root_task_without_writing_state() {
    let harness = Harness::new();
    let project = harness.project("repo", REMOTE_REPO, Some(HOST));
    let root = harness.root_task(&project, Some(REMOTE_REPO));

    let status = harness
        .service
        .get_task_hooks_status(&root.id, KanvibeHookProvider::Claude)
        .unwrap();

    assert_eq!(
        status,
        Some(hook_status(KanvibeHookProvider::Claude, false))
    );
    assert_eq!(harness.find_task(&root.id), Some(root));
    assert_nothing_repaired(&harness);
}

// parity: TS-main-projectService-b28979
#[test]
fn remote_task_claude_status_is_read_on_its_ssh_host() {
    let harness = Harness::new();
    let (_, task) = remote_task(&harness, REMOTE_WORKTREE, None);

    harness
        .service
        .get_task_hooks_status(&task.id, KanvibeHookProvider::Claude)
        .unwrap();

    assert_eq!(
        status_targets(&harness),
        [(
            KanvibeHookProvider::Claude,
            REMOTE_WORKTREE.to_string(),
            Some(task.id),
            Some(HOST.to_string())
        )]
    );
}

// parity: TS-main-projectService-266f32
#[test]
fn non_default_branch_task_on_the_remote_repo_root_reads_status_with_its_own_id() {
    let harness = Harness::new();
    let (project, task) = remote_task(&harness, REMOTE_REPO, Some("feature/login"));
    let root = harness.root_task(&project, Some(REMOTE_REPO));

    for provider in KanvibeHookProvider::ALL {
        harness
            .service
            .get_task_hooks_status(&task.id, provider)
            .unwrap();
    }

    let expected: Vec<_> = KanvibeHookProvider::ALL
        .into_iter()
        .map(|provider| {
            (
                provider,
                REMOTE_REPO.to_string(),
                Some(task.id.clone()),
                Some(HOST.to_string()),
            )
        })
        .collect();
    assert_eq!(status_targets(&harness), expected);
    assert_eq!(harness.find_task(&root.id), Some(root));
    assert!(harness.hooks.ensure_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-e0f056
#[test]
fn remote_reinstall_runs_only_the_chosen_provider() {
    let harness = Harness::new();
    let (_, task) = remote_task(&harness, REMOTE_WORKTREE, None);

    harness
        .service
        .install_task_hooks(&task.id, KanvibeHookProvider::Claude)
        .unwrap();

    let calls = harness.hooks.provider_install_calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        [(
            REMOTE_WORKTREE.to_string(),
            task.id,
            Some(KanvibeHookProvider::Claude),
            Some(HOST.to_string())
        )]
    );
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-aaf85b
#[test]
fn non_default_branch_task_on_the_remote_repo_root_reinstalls_with_its_own_id() {
    let harness = Harness::new();
    let (_, task) = remote_task(&harness, REMOTE_REPO, Some("feature/login"));

    for provider in KanvibeHookProvider::ALL {
        harness
            .service
            .install_task_hooks(&task.id, provider)
            .unwrap();
    }

    let expected: Vec<_> = KanvibeHookProvider::ALL
        .into_iter()
        .map(|provider| {
            (
                REMOTE_REPO.to_string(),
                task.id.clone(),
                Some(provider),
                Some(HOST.to_string()),
            )
        })
        .collect();
    let calls = harness.hooks.provider_install_calls.lock().unwrap().clone();
    assert_eq!(calls, expected);
    assert!(harness.hooks.install_calls.lock().unwrap().is_empty());
}

// parity: TS-main-projectService-daed09
#[test]
fn remote_ai_session_listing_passes_the_task_scope_and_ssh_host() {
    let harness = Harness::new();
    let (_, task) = remote_task(&harness, REMOTE_WORKTREE, None);
    let request = AiSessionListRequest {
        query: Some("claude".into()),
        ..AiSessionListRequest::default()
    };

    harness
        .service
        .get_task_ai_sessions(&task.id, &request)
        .unwrap();

    let calls = harness.ai_sessions.aggregate_calls.lock().unwrap().clone();
    let scope = AiSessionScope {
        worktree_path: REMOTE_WORKTREE.into(),
        repo_path: REMOTE_REPO.into(),
        ssh_host: Some(HOST.into()),
    };
    assert_eq!(calls, [(scope, request)]);
}
