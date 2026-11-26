//! TS `projectService.listSubdirectories`, `gitLiteralPaths` 폴더 탐색

use std::sync::Arc;

use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::shell_environment::ShellEnvironment;

use super::fakes::Harness;
use crate::project::adapters::HostProjectRepositoryProbe;
use crate::project::{ProjectService, ProjectServiceParts};

#[test]
fn remote_directories_are_scanned_without_session_tool_checks() {
    let harness = Harness::new();
    harness
        .repositories
        .on_command(|_| Ok("/workspace/api\n/workspace/web\n/workspace/.hidden".into()));

    let directories = harness
        .service
        .list_subdirectories("/workspace", Some("remote-host"));

    let commands = harness.repositories.commands.lock().unwrap().clone();
    assert_eq!(
        commands,
        [(
            "find '/workspace' -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort".to_string(),
            Some("remote-host".to_string())
        )]
    );
    assert_eq!(directories, ["api", "web"]);
}

#[test]
fn tilde_path_is_expanded_to_the_local_home() {
    let harness = Harness::new();
    harness
        .repositories
        .on_command(|_| Ok("/home/tester/projects".into()));

    let directories = harness.service.list_subdirectories("~/", None);

    let commands = harness.repositories.commands.lock().unwrap().clone();
    assert_eq!(
        commands,
        [(
            "find '/home/tester/' -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort".to_string(),
            None
        )]
    );
    assert_eq!(directories, ["projects"]);
}

#[test]
fn remote_tilde_path_is_scanned_from_the_remote_home() {
    let harness = Harness::new();
    harness
        .repositories
        .on_command(|_| Ok("/home/remote/projects".into()));

    let directories = harness
        .service
        .list_subdirectories("~/projects", Some("remote-host"));

    assert_eq!(
        harness.repositories.command_texts(),
        ["find \"$HOME\"'/projects' -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort"]
    );
    assert_eq!(directories, ["projects"]);
}

#[test]
fn remote_project_branches_are_listed_without_a_blocking_fetch() {
    let harness = Harness::new();
    let project = harness.project("repo", "/remote/repo", Some("remote-host"));

    let branches = harness.service.get_project_branches(&project.id).unwrap();

    assert_eq!(branches, ["main", "develop"]);
    let calls = harness
        .repositories
        .branch_list_calls
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        calls,
        [(
            "/remote/repo".to_string(),
            Some("remote-host".to_string()),
            false
        )]
    );
}

/// 실제 로컬 셸로 실행하는 저장소 조회. HOME과 PATH만 넘겨 사용자 환경이 새지 않게 한다
fn local_shell_probe(home: &str) -> HostProjectRepositoryProbe {
    let mut environment = ShellEnvironment::new();
    environment.insert("PATH".into(), "/usr/bin:/bin".into());
    environment.insert("HOME".into(), home.into());
    let runner = SystemShellCommandRunner::new(environment, home, Default::default());
    HostProjectRepositoryProbe::new(Arc::new(HostFileAccess::new(Arc::new(runner), home)))
}

#[test]
fn folder_browsing_keeps_directory_names_with_command_substitution() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().to_str().unwrap().to_string();
    let directory = root
        .path()
        .join("$(printf SUBSTITUTED)$ABSENT`printf tick`\"quote'");
    std::fs::create_dir_all(directory.join("child")).unwrap();
    let harness = Harness::new();
    let service = ProjectService::new(ProjectServiceParts {
        database: Arc::clone(&harness.db.database),
        board: Arc::clone(&harness.board),
        repositories: Arc::new(local_shell_probe(&home)),
        sessions: harness.sessions.clone(),
        hooks: harness.hooks.clone(),
        state_files: harness.state_files.clone(),
        icons: harness.icons.clone(),
        ai_sessions: harness.ai_sessions.clone(),
    });

    let directories = service.list_subdirectories(directory.to_str().unwrap(), None);

    assert_eq!(directories, ["child"]);
}
