//! gh를 부르는 셸 명령은 문자열 비교만으로는 검증되지 않는다.
//! 실제 저장소 배치를 만들고 gh·direnv를 흉내 내는 stub을 PATH 앞에 둔 `sh -c`로 돌려, 어느 디렉터리에서 gh가 실행되는지 본다.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kanvibe_domain::task::TaskStatus;
use kanvibe_process::exec::ExecGitError;

use super::fakes::KanbanFixture;
use super::{add_project, add_task, with_branch};
use crate::test_support::LoopbackShellRunner;

const GH_STUB: &str = "#!/bin/sh\nprintf \"gh:%s\" \"$(pwd)\"\n";
const DIRENV_STUB: &str = "#!/bin/sh\nshift 2\nprintf \"direnv:%s\" \"$(pwd)\"\n";
const BLOCKED_DIRENV_STUB: &str = "#!/bin/sh\necho \"direnv: .envrc is blocked\" >&2\nexit 1\n";

fn write_executable(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// gh와 direnv stub 디렉터리
fn create_shell_stubs(workspace: &Path, direnv_stub: &str) -> PathBuf {
    let stub_directory = workspace.join("__bin");
    std::fs::create_dir_all(&stub_directory).unwrap();
    write_executable(&stub_directory.join("gh"), GH_STUB);
    write_executable(&stub_directory.join("direnv"), direnv_stub);
    stub_directory
}

fn run_github_cli_command(command: &str, stub_directory: &Path) -> Result<String, String> {
    let output = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(command)
        .env_clear()
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", stub_directory.display()),
        )
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

fn create_repository(workspace: &Path, relative_path: &str) -> PathBuf {
    let repository = workspace.join(relative_path);
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    repository
}

/// 등록 경로가 `repo_path`인 프로젝트의 task로 PR을 조회해 실행된 명령을 얻는다
fn capture_github_cli_command(repo_path: &Path) -> String {
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, &repo_path.to_string_lossy(), None);
    let task = add_task(
        &fixture,
        "login",
        TaskStatus::Todo,
        with_branch(&project, "feat/login", None),
    );
    fixture
        .service
        .fetch_and_save_pr_url(&task.id, false)
        .unwrap();
    fixture.runner.commands().last().cloned().unwrap()
}

fn gh_directory(workspace: &Path, repo_path: &Path) -> String {
    let command = capture_github_cli_command(repo_path);
    run_github_cli_command(&command, &create_shell_stubs(workspace, DIRENV_STUB)).unwrap()
}

// parity: TS-main-kanbanService-719656
#[test]
fn a_single_nested_repository_under_the_registered_path_runs_gh() {
    let workspace = tempfile::tempdir().unwrap();
    let nested = create_repository(workspace.path(), "web");

    assert_eq!(
        gh_directory(workspace.path(), workspace.path()),
        format!("gh:{}", nested.display())
    );
}

// parity: TS-main-kanbanService-8053fe
#[test]
fn real_shell_recovers_through_the_git_origin_after_gh_cannot_resolve_the_repository() {
    let workspace = tempfile::tempdir().unwrap();
    let repository = workspace.path().join("git-fallback");
    std::fs::create_dir_all(&repository).unwrap();
    let git = LoopbackShellRunner::new(workspace.path());
    git.git(&repository, &["init", "-q"]);
    git.git(
        &repository,
        &["remote", "add", "origin", "git@github.com:example/repo.git"],
    );
    let stub_directory = create_shell_stubs(workspace.path(), DIRENV_STUB);
    let gh_resolving_only_explicit_repository = [
        "#!/bin/sh",
        "case \"$*\" in",
        "  *\"--repo example/repo\"*) printf %s \"https://github.com/example/repo/pull/12\" ;;",
        "  *) echo \"GraphQL: Could not resolve to a Repository\" >&2; exit 1 ;;",
        "esac",
    ]
    .join("\n");
    write_executable(
        &stub_directory.join("gh"),
        &gh_resolving_only_explicit_repository,
    );
    let fixture = KanbanFixture::new();
    let project = add_project(&fixture, &repository.to_string_lossy(), None);
    let task = add_task(
        &fixture,
        "login",
        TaskStatus::Todo,
        with_branch(&project, "feat/login", None),
    );
    let stubs = Arc::new(stub_directory);
    fixture
        .responses
        .set(move |call| run_github_cli_command(&call.command, &stubs).map_err(ExecGitError::new));

    let result = fixture
        .service
        .fetch_and_save_pr_url(&task.id, false)
        .unwrap();

    assert_eq!(
        result.as_deref(),
        Some("https://github.com/example/repo/pull/12")
    );
}

/// 엉뚱한 저장소의 PR을 저장하면 머지 알림까지 잘못 뜬다. 실패가 낫다
// parity: TS-main-kanbanService-7508c2
#[test]
fn several_nested_repositories_keep_gh_in_the_registered_path() {
    let workspace = tempfile::tempdir().unwrap();
    create_repository(workspace.path(), "api");
    create_repository(workspace.path(), "web");

    assert_eq!(
        gh_directory(workspace.path(), workspace.path()),
        format!("gh:{}", workspace.path().display())
    );
}

// parity: TS-main-kanbanService-c59bd8
#[test]
fn a_registered_repository_does_not_descend_into_nested_ones() {
    let workspace = tempfile::tempdir().unwrap();
    let repository = create_repository(workspace.path(), "solo");
    create_repository(workspace.path(), "solo/nested");

    assert_eq!(
        gh_directory(workspace.path(), &repository),
        format!("gh:{}", repository.display())
    );
}

// parity: TS-main-kanbanService-62b872
#[test]
fn without_envrc_plain_gh_runs_even_when_direnv_is_installed() {
    let workspace = tempfile::tempdir().unwrap();
    let repository = create_repository(workspace.path(), "plain");

    assert_eq!(
        gh_directory(workspace.path(), &repository),
        format!("gh:{}", repository.display())
    );
}

// parity: TS-main-kanbanService-bc278d
#[test]
fn envrc_in_the_repository_runs_gh_through_direnv() {
    let workspace = tempfile::tempdir().unwrap();
    let repository = create_repository(workspace.path(), "direnv-repo");
    std::fs::write(repository.join(".envrc"), "export GH_TOKEN=token\n").unwrap();

    assert_eq!(
        gh_directory(workspace.path(), &repository),
        format!("direnv:{}", repository.display())
    );
}

/// 상위 폴더에 경로별 GitHub 인증을 둔 경우도 direnv는 그 설정을 읽는다
// parity: TS-main-kanbanService-e51a46
#[test]
fn envrc_in_a_parent_directory_also_runs_gh_through_direnv() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join(".envrc"), "export GH_TOKEN=token\n").unwrap();
    let repository = create_repository(workspace.path(), "work/direnv-parent-repo");

    assert_eq!(
        gh_directory(workspace.path(), &repository),
        format!("direnv:{}", repository.display())
    );
}

/// direnv allow가 안 된 .envrc면 direnv 경로가 늘 실패하므로 평범한 gh로 되돌아가야 조회할 수 있다
// parity: TS-main-kanbanService-fee9c7
#[test]
fn a_failing_direnv_falls_back_to_plain_gh() {
    let workspace = tempfile::tempdir().unwrap();
    let repository = create_repository(workspace.path(), "blocked-envrc");
    std::fs::write(repository.join(".envrc"), "export GH_TOKEN=token\n").unwrap();
    let command = capture_github_cli_command(&repository);

    let output = run_github_cli_command(
        &command,
        &create_shell_stubs(workspace.path(), BLOCKED_DIRENV_STUB),
    );

    assert_eq!(output, Ok(format!("gh:{}", repository.display())));
}

/// 경로 없음은 재시도해도 회복되지 않는다. 오류로 올리면 매 background sync마다 실패 창이 뜬다
// parity: TS-main-kanbanService-fddc93
#[test]
fn a_vanished_repository_prints_the_marker_without_calling_gh() {
    let workspace = tempfile::tempdir().unwrap();

    let output = gh_directory(workspace.path(), &workspace.path().join("gone"));

    assert_eq!(output, "kanvibe-missing-repository");
}

/// 원격 실행기는 이 패턴이 든 명령을 조용한 probe로 보고 실패 로그를 지우므로, 원격 PR 조회 실패가 로그에 남지 않게 된다
// parity: TS-main-kanbanService-4041f1
#[test]
fn the_command_has_no_quiet_probe_redirections() {
    let workspace = tempfile::tempdir().unwrap();
    let repository = create_repository(workspace.path(), "logging");

    let command = capture_github_cli_command(&repository);

    assert!(!command.contains(">/dev/null 2>&1"));
    assert!(!command.contains("2>/dev/null"));
}
