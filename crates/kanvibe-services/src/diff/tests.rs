use std::cell::Cell;
use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc;

use kanvibe_db::repositories::tasks::NewKanbanTask;
use kanvibe_domain::task::TaskStatus;
use kanvibe_domain::task_diff_stats::DiffFileStatus;
use kanvibe_process::exec::{ExecGitError, ShellCommandRunner};

use super::*;
use crate::test_support::{
    LoopbackShellRunner, RecordedCommand, ScriptedCommandRunner, TemporaryDatabase, new_task,
    wait_until,
};

const MAX_FILE_BYTES: usize = 1024 * 1024;

fn diff_file(path: &str, status: DiffFileStatus, additions: i64, deletions: i64) -> DiffFile {
    DiffFile {
        path: path.into(),
        original_path: None,
        status,
        additions,
        deletions,
    }
}

fn stats(file_count: i64, additions: i64, deletions: i64) -> TaskDiffStats {
    TaskDiffStats {
        file_count,
        additions,
        deletions,
    }
}

struct Fixture {
    database: TemporaryDatabase,
    service: DiffService,
}

fn fixture_with(runner: Arc<dyn ShellCommandRunner>) -> Fixture {
    let database = TemporaryDatabase::open();
    let home = database.directory.path().join("home");
    let files = Arc::new(HostFileAccess::new(runner, home.to_string_lossy()));
    let service = DiffService::new(Arc::clone(&database.database), files);
    Fixture { database, service }
}

impl Fixture {
    fn insert_task(
        &self,
        worktree_path: &str,
        branch_name: Option<&str>,
        ssh_host: Option<&str>,
    ) -> String {
        self.database
            .insert_task(NewKanbanTask {
                worktree_path: Some(worktree_path.into()),
                branch_name: branch_name.map(str::to_string),
                base_branch: Some("main".into()),
                ssh_host: ssh_host.map(str::to_string),
                ..new_task("diff task", TaskStatus::Progress)
            })
            .id
    }

    fn saved_stats(&self, task_id: &str) -> Option<TaskDiffStats> {
        kanvibe_db::repositories::task_diff_stats::find_task_diff_stats(
            &self.database.database.connection(),
            task_id,
        )
        .unwrap()
        .map(|record| record.stats)
    }

    /// 집계는 task를 참조하므로 없는 task면 자리만 채운 행을 먼저 만든다
    fn seed_stats(&self, task_id: &str, cached: TaskDiffStats) {
        self.database
            .database
            .connection()
            .execute(
                "INSERT OR IGNORE INTO kanban_tasks (id, title, status) VALUES (?1, ?1, 'review')",
                [task_id],
            )
            .unwrap();
        save_task_diff_stats(&self.database.database.connection(), task_id, cached).unwrap();
    }
}

fn diff_output(additions: i64, deletions: i64) -> String {
    [
        "__KANVIBE_DIFF_NAME_STATUS__",
        "M\0src/app.ts",
        "__KANVIBE_DIFF_NUMSTAT__",
        &format!("{additions}\t{deletions}\tsrc/app.ts"),
        "__KANVIBE_DIFF_WORKING_TREE__",
    ]
    .join("\0")
        + "\0"
}

#[test]
fn lists_remote_diff_files_through_the_ssh_host() {
    let output = [
        "__KANVIBE_DIFF_NAME_STATUS__",
        "M\0src/app.ts",
        "__KANVIBE_DIFF_NUMSTAT__",
        "3\t1\tsrc/app.ts",
        "__KANVIBE_DIFF_WORKING_TREE__",
        "?? docs/new.md",
    ]
    .join("\0")
        + "\0";
    let runner = Arc::new(ScriptedCommandRunner::returning(&output));
    let fixture = fixture_with(Arc::clone(&runner) as Arc<dyn ShellCommandRunner>);
    let task_id = fixture.insert_task(
        "/remote/worktrees/fix-qa",
        Some("fix/qa"),
        Some("remote-host"),
    );

    let files = fixture.service.get_git_diff_files(&task_id);

    assert_eq!(
        files,
        [
            diff_file("src/app.ts", DiffFileStatus::Modified, 3, 1),
            diff_file("docs/new.md", DiffFileStatus::Added, 0, 0),
        ]
    );
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].ssh_host.as_deref(), Some("remote-host"));
    for expected in [
        "git -C '/remote/worktrees/fix-qa' diff HEAD --name-status",
        "git -C '/remote/worktrees/fix-qa' diff HEAD --numstat",
        "git -C '/remote/worktrees/fix-qa' status --porcelain --untracked-files=all",
    ] {
        assert!(calls[0].command.contains(expected), "{expected}");
    }
}

#[test]
fn reads_and_saves_remote_files_through_the_ssh_host() {
    let runner = Arc::new(ScriptedCommandRunner::new(|call: &RecordedCommand| {
        Ok(if call.command.contains(" show ") {
            "base content"
        } else {
            "current content"
        }
        .into())
    }));
    let fixture = fixture_with(Arc::clone(&runner) as Arc<dyn ShellCommandRunner>);
    let task_id = fixture.insert_task(
        "/remote/worktrees/fix-qa",
        Some("fix/qa"),
        Some("remote-host"),
    );

    assert_eq!(
        fixture
            .service
            .get_original_file_content(&task_id, "src/app.ts"),
        "base content"
    );
    assert_eq!(
        fixture.service.get_file_content(&task_id, "src/app.ts"),
        "current content"
    );
    assert_eq!(
        fixture
            .service
            .save_file_content(&task_id, "src/app.ts", "updated"),
        FileSaveResult {
            success: true,
            error: None
        }
    );

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("remote-host"))
    );
    assert!(
        calls[0]
            .command
            .contains("git -C '/remote/worktrees/fix-qa' show 'HEAD:src/app.ts'")
    );
    assert!(
        calls[1]
            .command
            .contains("cat '/remote/worktrees/fix-qa/src/app.ts'")
    );
    assert!(
        calls[2]
            .command
            .contains("> '/remote/worktrees/fix-qa/src/app.ts'")
    );
}

#[test]
fn refreshes_only_the_requested_tasks_and_saves_their_stats() {
    let runner = Arc::new(ScriptedCommandRunner::returning(&diff_output(12, 3)));
    let fixture = fixture_with(Arc::clone(&runner) as Arc<dyn ShellCommandRunner>);
    let task_id = fixture.insert_task(
        "/repo__worktrees/task-progress",
        Some("feat/progress"),
        None,
    );
    fixture.insert_task("/repo__worktrees/task-other", Some("feat/other"), None);

    let cached = fixture
        .service
        .get_task_diff_stats(std::slice::from_ref(&task_id));

    assert!(cached.is_empty());
    wait_until(|| fixture.saved_stats(&task_id).is_some());
    assert_eq!(fixture.saved_stats(&task_id), Some(stats(1, 12, 3)));
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn fills_tasks_not_refreshed_from_the_saved_stats() {
    let runner = Arc::new(ScriptedCommandRunner::returning(&diff_output(12, 3)));
    let fixture = fixture_with(Arc::clone(&runner) as Arc<dyn ShellCommandRunner>);
    let progress = fixture.insert_task(
        "/repo__worktrees/task-progress",
        Some("feat/progress"),
        None,
    );
    fixture.seed_stats("task-review", stats(4, 40, 8));
    fixture.seed_stats(&progress, stats(1, 1, 1));

    let cached = fixture
        .service
        .get_task_diff_stats(std::slice::from_ref(&progress));

    assert_eq!(
        cached,
        BTreeMap::from([
            ("task-review".to_string(), stats(4, 40, 8)),
            (progress.clone(), stats(1, 1, 1))
        ])
    );
    wait_until(|| fixture.saved_stats(&progress) == Some(stats(1, 12, 3)));
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn returns_saved_stats_without_git_when_nothing_needs_refreshing() {
    let runner = Arc::new(ScriptedCommandRunner::returning(""));
    let fixture = fixture_with(Arc::clone(&runner) as Arc<dyn ShellCommandRunner>);
    fixture.seed_stats("task-review", stats(4, 40, 8));

    let cached = fixture.service.get_task_diff_stats(&[]);

    assert_eq!(
        cached,
        BTreeMap::from([("task-review".to_string(), stats(4, 40, 8))])
    );
    assert!(runner.calls().is_empty());
}

#[test]
fn a_failed_refresh_keeps_the_saved_stats() {
    let runner = Arc::new(ScriptedCommandRunner::new(|_| {
        Err(ExecGitError::new("git 실행 실패"))
    }));
    let fixture = fixture_with(Arc::clone(&runner) as Arc<dyn ShellCommandRunner>);
    let task_id = fixture.insert_task(
        "/repo__worktrees/task-progress",
        Some("feat/progress"),
        None,
    );
    fixture.seed_stats(&task_id, stats(4, 40, 8));

    let cached = fixture
        .service
        .get_task_diff_stats(std::slice::from_ref(&task_id));
    wait_until(|| !runner.calls().is_empty());
    wait_until(|| lock(&fixture.service.shared.pending_refreshes).is_empty());

    assert_eq!(cached, BTreeMap::from([(task_id.clone(), stats(4, 40, 8))]));
    assert_eq!(fixture.saved_stats(&task_id), Some(stats(4, 40, 8)));
}

#[test]
fn reading_the_file_list_in_detail_also_refreshes_the_stats() {
    let runner = Arc::new(ScriptedCommandRunner::returning(&diff_output(7, 2)));
    let fixture = fixture_with(runner);
    let task_id = fixture.insert_task("/repo__worktrees/task-review", Some("feat/review"), None);

    let files = fixture.service.get_git_diff_files(&task_id);

    assert_eq!(
        files,
        [diff_file("src/app.ts", DiffFileStatus::Modified, 7, 2)]
    );
    assert_eq!(fixture.saved_stats(&task_id), Some(stats(1, 7, 2)));
}

/// 실제 git 저장소 fixture. 원격 호스트를 넘겨도 같은 머신에서 실행한다
struct GitFixture {
    fixture: Fixture,
    runner: Arc<LoopbackShellRunner>,
    repository: tempfile::TempDir,
}

impl GitFixture {
    fn new() -> Self {
        let repository = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runner = Arc::new(LoopbackShellRunner::new(home.path()));
        let fixture = fixture_with(Arc::clone(&runner) as Arc<dyn ShellCommandRunner>);
        let git_fixture = Self {
            fixture,
            runner,
            repository,
        };
        git_fixture.git(&["init", "-b", "main"]);
        git_fixture.git(&["config", "user.email", "fixture@example.invalid"]);
        git_fixture.git(&["config", "user.name", "fixture"]);
        git_fixture
    }

    fn path(&self) -> &Path {
        self.repository.path()
    }

    fn git(&self, arguments: &[&str]) {
        self.runner.git(self.path(), arguments);
    }

    fn write(&self, name: &str, content: &[u8]) {
        std::fs::write(self.path().join(name), content).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path().join(name)).unwrap()
    }

    fn insert_task(&self, branch_name: &str, ssh_host: Option<&str>) -> String {
        self.fixture
            .insert_task(&self.path().to_string_lossy(), Some(branch_name), ssh_host)
    }
}

fn sorted_by_path(mut files: Vec<DiffFile>) -> Vec<DiffFile> {
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files
}

fn build_uncommitted_fixture(repository: &GitFixture) {
    for (name, content) in [
        ("tracked.txt", "base\n"),
        ("deleted.txt", "remove\nthis\n"),
        ("reverted.txt", "original\n"),
        ("old.txt", "rename\n"),
        (".gitignore", "ignored.txt\n"),
    ] {
        repository.write(name, content.as_bytes());
    }
    repository.git(&["add", "."]);
    repository.git(&["commit", "-m", "base"]);
    repository.git(&["switch", "-c", "feature"]);
    repository.write("tracked.txt", b"committed\n");
    repository.write("committed-only.txt", b"already committed\n");
    repository.git(&["add", "."]);
    repository.git(&["commit", "-m", "feature changes"]);
    for (name, content) in [
        ("tracked.txt", "staged\n"),
        ("staged.txt", "new staged\n"),
        ("reverted.txt", "staged change\n"),
        ("removed-add.txt", "temporary\n"),
    ] {
        repository.write(name, content.as_bytes());
    }
    repository.git(&["add", "."]);
    repository.write("tracked.txt", b"working\nextra\n");
    repository.write("reverted.txt", b"original\n");
    std::fs::remove_file(repository.path().join("removed-add.txt")).unwrap();
    std::fs::remove_file(repository.path().join("deleted.txt")).unwrap();
    repository.git(&["mv", "old.txt", "new.txt"]);
    repository.write("untracked ' 한글.txt", b"one\ntwo");
    repository.write("binary.dat", &[0, 1, 2]);
    repository.write("empty.txt", b"");
    repository.write("ignored.txt", b"ignored\n");
}

#[test]
fn lists_only_uncommitted_files_and_clears_stats_after_commit() {
    for ssh_host in [None, Some("remote-host")] {
        let repository = GitFixture::new();
        build_uncommitted_fixture(&repository);
        let task_id = repository.insert_task("feature", ssh_host);
        let service = &repository.fixture.service;

        let files = sorted_by_path(service.get_git_diff_files(&task_id));

        let renamed = DiffFile {
            original_path: Some("old.txt".into()),
            ..diff_file("new.txt", DiffFileStatus::Renamed, 0, 0)
        };
        assert_eq!(
            files,
            [
                diff_file("binary.dat", DiffFileStatus::Added, 0, 0),
                diff_file("deleted.txt", DiffFileStatus::Deleted, 0, 2),
                diff_file("empty.txt", DiffFileStatus::Added, 0, 0),
                renamed,
                diff_file("staged.txt", DiffFileStatus::Added, 1, 0),
                diff_file("tracked.txt", DiffFileStatus::Modified, 2, 1),
                diff_file("untracked ' 한글.txt", DiffFileStatus::Added, 2, 0),
            ],
            "{ssh_host:?}"
        );
        assert!(
            repository
                .runner
                .calls()
                .iter()
                .all(|call| call.ssh_host.as_deref() == ssh_host)
        );
        assert_eq!(
            repository.fixture.saved_stats(&task_id),
            Some(stats(7, 5, 3))
        );
        // exec_git은 출력의 앞뒤 공백을 지우므로 끝 줄바꿈이 남지 않는다
        assert_eq!(
            service.get_original_file_content(&task_id, "tracked.txt"),
            "committed"
        );
        assert_eq!(
            service.get_original_file_content(&task_id, "old.txt"),
            "rename"
        );
        repository.git(&["add", "-A"]);
        repository.git(&["commit", "-m", "finish changes"]);
        assert_eq!(service.get_git_diff_files(&task_id), []);
        assert_eq!(
            repository.fixture.saved_stats(&task_id),
            Some(stats(0, 0, 0))
        );
    }
}

#[test]
fn reads_and_saves_paths_with_spaces_korean_tabs_newlines_and_renames() {
    let repository = GitFixture::new();
    repository.write("committed old.txt", b"committed rename\n");
    repository.write("working old.txt", b"working rename\n");
    repository.git(&["add", "."]);
    repository.git(&["commit", "-m", "initial"]);
    repository.git(&["checkout", "-b", "feature"]);
    repository.git(&["mv", "committed old.txt", "committed 한글\tnew.txt"]);
    repository.write("stats\n\t한글.txt", b"first\nsecond\n");
    repository.git(&["add", "."]);
    repository.git(&["commit", "-m", "rename"]);
    repository.write("stats\n\t한글.txt", b"first\nsecond\nthird\n");
    repository.git(&["mv", "working old.txt", "working 새 파일.txt"]);
    let untracked = [
        "hello world.txt",
        "한글.txt",
        "tab\tfile.txt",
        "line\nfile.txt",
    ];
    for name in untracked {
        repository.write(name, b"original\n");
    }
    let task_id = repository.insert_task("feature", None);
    let service = &repository.fixture.service;

    let files = service.get_git_diff_files(&task_id);

    let mut expected: Vec<&str> = untracked.to_vec();
    expected.extend(["working 새 파일.txt", "stats\n\t한글.txt"]);
    expected.sort();
    let mut paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, expected);
    assert_eq!(
        files.iter().find(|file| file.path == "stats\n\t한글.txt"),
        Some(&diff_file(
            "stats\n\t한글.txt",
            DiffFileStatus::Modified,
            1,
            0
        ))
    );
    let renamed: Vec<&str> = files
        .iter()
        .filter(|file| file.status == DiffFileStatus::Renamed)
        .map(|file| file.path.as_str())
        .collect();
    assert_eq!(renamed, ["working 새 파일.txt"]);
    for name in untracked {
        assert_eq!(
            files
                .iter()
                .find(|file| file.path == name)
                .unwrap()
                .additions,
            1
        );
    }
    for name in expected {
        assert_ne!(service.get_file_content(&task_id, name), "");
        assert_eq!(
            service.save_file_content(&task_id, name, "updated\n"),
            FileSaveResult {
                success: true,
                error: None
            }
        );
        assert_eq!(repository.read(name), "updated\n");
    }
}

#[test]
fn lists_changes_without_a_base_branch_and_keeps_stats_when_git_fails() {
    let repository = GitFixture::new();
    repository.write("tracked.txt", b"initial\n");
    repository.git(&["add", "."]);
    repository.git(&["commit", "-m", "initial"]);
    let task_id = repository.insert_task("main", None);
    let fixture = &repository.fixture;
    fixture.seed_stats(&task_id, stats(8, 21, 5));

    let cached = fixture
        .service
        .get_task_diff_stats(std::slice::from_ref(&task_id));
    assert_eq!(cached, BTreeMap::from([(task_id.clone(), stats(8, 21, 5))]));
    fixture.service.get_git_diff_files(&task_id);
    assert_eq!(fixture.service.get_git_diff_files(&task_id), []);
    assert_eq!(fixture.saved_stats(&task_id), Some(stats(0, 0, 0)));

    fixture.seed_stats(&task_id, stats(3, 3, 3));
    let missing = repository.path().join("missing-worktree");
    fixture
        .database
        .database
        .connection()
        .execute(
            "UPDATE kanban_tasks SET worktree_path = ?1 WHERE id = ?2",
            (missing.to_string_lossy(), &task_id),
        )
        .unwrap();
    assert_eq!(fixture.service.get_git_diff_files(&task_id), []);
    assert_eq!(fixture.saved_stats(&task_id), Some(stats(3, 3, 3)));
}

#[test]
fn reads_dot_dot_named_inner_files_and_blocks_parent_paths() {
    for ssh_host in [None, Some("remote-host")] {
        let repository = GitFixture::new();
        std::fs::create_dir(repository.path().join("..directory")).unwrap();
        repository.write("..notes", b"notes\n");
        repository.write("..directory/file", b"nested\n");
        let task_id = repository.insert_task("main", ssh_host);
        let service = &repository.fixture.service;
        let saved = FileSaveResult {
            success: true,
            error: None,
        };

        // 원격 읽기는 exec_git을 거치므로 끝 줄바꿈이 지워진다
        let expected_notes = if ssh_host.is_some() {
            "notes"
        } else {
            "notes\n"
        };
        assert_eq!(
            service.get_file_content(&task_id, "..notes"),
            expected_notes
        );
        assert!(
            service
                .get_file_content(&task_id, "..directory/file")
                .starts_with("nested")
        );
        assert_eq!(
            service.save_file_content(&task_id, "..notes", "updated\n"),
            saved
        );
        assert_eq!(repository.read("..notes"), "updated\n");
        assert_eq!(
            service.save_file_content(&task_id, "..directory/file", "nested updated\n"),
            saved
        );
        assert_eq!(repository.read("..directory/file"), "nested updated\n");
        assert!(
            !service
                .save_file_content(&task_id, "../outside", "blocked")
                .success
        );
        assert!(
            !service
                .save_file_content(&task_id, "/outside", "blocked")
                .success
        );
        assert!(!repository.path().parent().unwrap().join("outside").exists());
    }
}

/// 미리보기 fixture. worktree 밖 디렉터리와 함께 만든다
struct WorkspaceFixture {
    repository: GitFixture,
    outside: tempfile::TempDir,
}

fn workspace_fixture() -> WorkspaceFixture {
    let repository = GitFixture::new();
    std::fs::create_dir(repository.path().join("docs")).unwrap();
    repository.write("docs/plan.md", "# plan\n본문\n".as_bytes());
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    WorkspaceFixture {
        repository,
        outside,
    }
}

impl WorkspaceFixture {
    fn read(
        &self,
        task_id: &str,
        file_path: &str,
    ) -> Result<TaskWorkspaceFile, TaskWorkspaceFileFailure> {
        self.repository
            .fixture
            .service
            .read_task_workspace_file(task_id, file_path)
    }

    fn path_of(&self, relative: &str) -> String {
        self.repository
            .path()
            .join(relative)
            .to_string_lossy()
            .into_owned()
    }
}

#[test]
fn reads_a_relative_path_inside_the_worktree() {
    let workspace = workspace_fixture();
    let task_id = workspace.repository.insert_task("main", None);

    let file = workspace.read(&task_id, "docs/plan.md").unwrap();

    assert_eq!(
        file,
        TaskWorkspaceFile {
            path: "docs/plan.md".into(),
            content: "# plan\n본문\n".into()
        }
    );
}

#[test]
fn turns_an_absolute_path_inside_the_worktree_into_a_relative_one() {
    let workspace = workspace_fixture();
    let task_id = workspace.repository.insert_task("main", None);

    let file = workspace
        .read(&task_id, &workspace.path_of("docs/plan.md"))
        .unwrap();

    assert_eq!(
        file,
        TaskWorkspaceFile {
            path: "docs/plan.md".into(),
            content: "# plan\n본문\n".into()
        }
    );
}

#[test]
fn refuses_parent_and_outside_absolute_paths_without_running_commands() {
    for ssh_host in [None, Some("remote-host")] {
        let workspace = workspace_fixture();
        let task_id = workspace.repository.insert_task("main", ssh_host);
        let outside_name = workspace
            .outside
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let sibling = format!(
            "{}-sibling/secret.txt",
            workspace.repository.path().display()
        );
        let outside_file = workspace
            .outside
            .path()
            .join("secret.txt")
            .to_string_lossy()
            .into_owned();
        let calls_before = workspace.repository.runner.calls().len();

        for request in [
            format!("../{outside_name}/secret.txt"),
            outside_file,
            sibling,
        ] {
            assert_eq!(
                workspace.read(&task_id, &request),
                Err(TaskWorkspaceFileFailure::OutsideWorktree)
            );
        }
        assert_eq!(
            TaskWorkspaceFileFailure::OutsideWorktree.to_string(),
            "outside-worktree"
        );
        assert_eq!(workspace.repository.runner.calls().len(), calls_before);
    }
}

#[test]
fn refuses_files_over_one_mebibyte_and_reads_exactly_one() {
    for ssh_host in [None, Some("remote-host")] {
        let workspace = workspace_fixture();
        workspace
            .repository
            .write("limit.txt", "a".repeat(MAX_FILE_BYTES).as_bytes());
        workspace
            .repository
            .write("over.txt", "a".repeat(MAX_FILE_BYTES + 1).as_bytes());
        let task_id = workspace.repository.insert_task("main", ssh_host);

        let at_limit = workspace.read(&task_id, "limit.txt").unwrap();

        assert_eq!(at_limit.path, "limit.txt");
        assert_eq!(at_limit.content.len(), MAX_FILE_BYTES);
        assert_eq!(
            workspace.read(&task_id, "over.txt"),
            Err(TaskWorkspaceFileFailure::TooLarge)
        );
    }
}

#[test]
fn reports_missing_files_and_directories_as_not_found() {
    for ssh_host in [None, Some("remote-host")] {
        let workspace = workspace_fixture();
        let task_id = workspace.repository.insert_task("main", ssh_host);

        assert_eq!(
            workspace.read(&task_id, "docs/missing.md"),
            Err(TaskWorkspaceFileFailure::NotFound)
        );
        assert_eq!(
            workspace.read(&task_id, "docs"),
            Err(TaskWorkspaceFileFailure::NotFound)
        );
    }
}

#[test]
fn reports_a_missing_task_as_not_found() {
    let workspace = workspace_fixture();

    assert_eq!(
        workspace.read("missing-task", "docs/plan.md"),
        Err(TaskWorkspaceFileFailure::NotFound)
    );
}

#[test]
fn measures_remote_files_before_reading_them() {
    let workspace = workspace_fixture();
    let task_id = workspace
        .repository
        .insert_task("main", Some("remote-host"));

    let file = workspace
        .read(&task_id, &workspace.path_of("docs/plan.md"))
        .unwrap();

    assert_eq!(
        file,
        TaskWorkspaceFile {
            path: "docs/plan.md".into(),
            content: "# plan\n본문".into()
        }
    );
    let call = workspace.repository.runner.calls().pop().unwrap();
    assert!(
        call.command
            .contains(&format!("wc -c < '{}'", workspace.path_of("docs/plan.md")))
    );
    assert_eq!(call.ssh_host.as_deref(), Some("remote-host"));
    assert_eq!(call.options.max_buffer_bytes, Some(MAX_FILE_BYTES + 4096));
}

#[test]
fn a_failed_remote_command_is_reported_as_unreadable() {
    let runner = Arc::new(ScriptedCommandRunner::new(|_| {
        Err(ExecGitError::new(
            "ssh: connect to host remote-host port 22: Connection refused",
        ))
    }));
    let fixture = fixture_with(runner);
    let task_id = fixture.insert_task("/remote/worktree", Some("main"), Some("remote-host"));

    let failure = fixture
        .service
        .read_task_workspace_file(&task_id, "docs/plan.md")
        .unwrap_err();

    assert_eq!(failure, TaskWorkspaceFileFailure::Unreadable);
    assert_eq!(failure.to_string(), "unreadable");
}

fn rename_case(kind: &str, is_edited: bool) {
    let repository = GitFixture::new();
    let old_path = "old 한글\tfile.txt";
    let new_path = "new 한글\nfile.txt";
    let original: String = (0..12)
        .map(|index| format!("original line {index}\n"))
        .collect();
    let current = if is_edited {
        original.replace("original line 5", "changed line 5")
    } else {
        original.clone()
    };
    repository.write(old_path, original.as_bytes());
    repository.git(&["add", "."]);
    repository.git(&["commit", "-m", "initial"]);
    repository.git(&["checkout", "-b", "feature"]);
    repository.git(&["mv", old_path, new_path]);
    repository.write(new_path, current.as_bytes());
    repository.git(&["add", "."]);
    if kind != "staged" {
        repository.git(&["commit", "-m", "rename"]);
    }
    let final_path = if kind == "chained" {
        "final.txt"
    } else {
        new_path
    };
    if kind == "chained" {
        repository.git(&["mv", new_path, final_path]);
    }
    let task_id = repository.insert_task("feature", None);
    let service = &repository.fixture.service;

    let files = service.get_git_diff_files(&task_id);

    // exec_git이 지우는 끝 줄바꿈을 뺀 내용과 비교한다
    if kind == "committed" {
        assert_eq!(files, []);
        assert_eq!(
            service.get_original_file_content(&task_id, new_path),
            current.trim_end()
        );
        return;
    }
    let expected_original_path = if kind == "chained" {
        new_path
    } else {
        old_path
    };
    assert_eq!(files.len(), 1, "{kind} {is_edited}");
    assert_eq!(files[0].path, final_path);
    assert_eq!(
        files[0].original_path.as_deref(),
        Some(expected_original_path)
    );
    assert_eq!(files[0].status, DiffFileStatus::Renamed);
    let original_lookup = files[0].original_path.clone().unwrap();
    let expected_original = if kind == "chained" {
        &current
    } else {
        &original
    };
    assert_eq!(
        service.get_original_file_content(&task_id, &original_lookup),
        expected_original.trim_end()
    );
    assert_eq!(service.get_file_content(&task_id, &files[0].path), current);
}

#[test]
fn reads_rename_originals_for_committed_staged_and_chained_renames() {
    for kind in ["committed", "staged", "chained"] {
        for is_edited in [false, true] {
            rename_case(kind, is_edited);
        }
    }
}

thread_local! {
    static IS_BACKGROUND_SCOPE: Cell<bool> = const { Cell::new(false) };
}

fn observe_background_scope(operation: &mut dyn FnMut()) {
    IS_BACKGROUND_SCOPE.with(|flag| flag.set(true));
    operation();
    IS_BACKGROUND_SCOPE.with(|flag| flag.set(false));
}

/// 경로 `/repo/<task>`마다 테스트가 풀어 줄 때까지 응답을 미루는 실행기. 호출 시점의 우선순위도 기록한다
struct GatedRunner {
    releases: Mutex<HashMap<String, mpsc::Sender<String>>>,
    started: Mutex<Vec<(String, bool)>>,
    is_gated: bool,
    default_output: String,
}

impl GatedRunner {
    fn new(is_gated: bool, default_output: String) -> Arc<Self> {
        Arc::new(Self {
            releases: Mutex::new(HashMap::new()),
            started: Mutex::new(Vec::new()),
            is_gated,
            default_output,
        })
    }

    fn task_of(command: &str) -> String {
        let rest = command.split("/repo/").nth(1).unwrap_or_default();
        rest.split('\'').next().unwrap_or_default().to_string()
    }

    fn started_tasks(&self) -> Vec<String> {
        self.started
            .lock()
            .unwrap()
            .iter()
            .map(|(task, _)| task.clone())
            .collect()
    }

    fn release(&self, task: &str, output: String) {
        let sender = self
            .releases
            .lock()
            .unwrap()
            .remove(task)
            .expect("시작한 조회");
        sender.send(output).unwrap();
    }

    fn release_all(&self, output: &str) {
        let senders: Vec<_> = self.releases.lock().unwrap().drain().collect();
        for (_, sender) in senders {
            let _ = sender.send(output.to_string());
        }
    }
}

impl ShellCommandRunner for GatedRunner {
    fn exec_git(
        &self,
        command: &str,
        _ssh_host: Option<&str>,
        _options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        let task = Self::task_of(command);
        let is_background = IS_BACKGROUND_SCOPE.with(Cell::get);
        if !self.is_gated {
            self.started.lock().unwrap().push((task, is_background));
            return Ok(self.default_output.clone());
        }
        let (sender, receiver) = mpsc::channel();
        self.releases.lock().unwrap().insert(task.clone(), sender);
        self.started.lock().unwrap().push((task, is_background));
        Ok(receiver.recv().unwrap())
    }
}

fn single_file_output(additions: i64) -> String {
    [
        "__KANVIBE_DIFF_NAME_STATUS__",
        "M\0a.ts",
        "__KANVIBE_DIFF_NUMSTAT__",
        &format!("{additions}\t0\ta.ts"),
        "__KANVIBE_DIFF_WORKING_TREE__",
    ]
    .join("\0")
        + "\0"
}

fn lazy_fixture(runner: Arc<GatedRunner>, task_ids: &[&str], hosts: &[(&str, &str)]) -> Fixture {
    let database = TemporaryDatabase::open();
    let files = Arc::new(HostFileAccess::new(
        runner as Arc<dyn ShellCommandRunner>,
        "/fixture/home",
    ));
    let service = DiffService::with_background_scope(
        Arc::clone(&database.database),
        files,
        observe_background_scope,
    );
    let fixture = Fixture { database, service };
    for task_id in task_ids {
        let host = hosts
            .iter()
            .find(|(id, _)| id == task_id)
            .map(|(_, host)| *host);
        fixture.database.database.connection().execute(
            "INSERT INTO kanban_tasks (id, title, status, branch_name, worktree_path, ssh_host, base_branch) VALUES (?1, ?1, 'progress', ?1, ?2, ?3, 'dev')",
            (task_id, format!("/repo/{task_id}"), host),
        ).unwrap();
    }
    fixture
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn saved_count(fixture: &Fixture, task_ids: &[&str]) -> usize {
    task_ids
        .iter()
        .filter(|task_id| {
            fixture
                .saved_stats(task_id)
                .is_some_and(|saved| saved.file_count == 1 && saved.additions != 9)
        })
        .count()
}

#[test]
fn returns_the_cache_without_waiting_for_slow_git() {
    let runner = GatedRunner::new(true, String::new());
    let fixture = lazy_fixture(Arc::clone(&runner), &["slow", "fast"], &[]);
    fixture.seed_stats("slow", stats(1, 9, 0));

    let initial = fixture.service.get_task_diff_stats(&ids(&["slow", "fast"]));
    assert_eq!(
        initial,
        BTreeMap::from([("slow".to_string(), stats(1, 9, 0))])
    );
    wait_until(|| runner.started_tasks().len() == 2);
    runner.release("fast", single_file_output(3));
    wait_until(|| fixture.saved_stats("fast").is_some());

    assert_eq!(
        fixture.service.get_task_diff_stats(&[]),
        BTreeMap::from([
            ("slow".to_string(), stats(1, 9, 0)),
            ("fast".to_string(), stats(1, 3, 0))
        ])
    );
    runner.release("slow", single_file_output(7));
    wait_until(|| fixture.saved_stats("slow") == Some(stats(1, 7, 0)));
}

#[test]
fn starts_every_refresh_and_merges_duplicates_in_flight() {
    let runner = GatedRunner::new(true, String::new());
    let fixture = lazy_fixture(Arc::clone(&runner), &["one", "two", "three"], &[]);

    fixture
        .service
        .get_task_diff_stats(&ids(&["one", "two", "three", "one"]));
    wait_until(|| runner.started_tasks().len() == 3);
    fixture
        .service
        .get_task_diff_stats(&ids(&["one", "two", "three"]));
    runner.release("two", single_file_output(2));
    wait_until(|| fixture.saved_stats("two").is_some());

    assert_eq!(runner.started_tasks().len(), 3);
    runner.release_all(&single_file_output(1));
    wait_until(|| saved_count(&fixture, &["one", "two", "three"]) == 3);
    assert_eq!(runner.started_tasks().len(), 3);
}

#[test]
fn starts_all_refreshes_regardless_of_host() {
    let runner = GatedRunner::new(true, String::new());
    let tasks = ["slow-1", "slow-2", "slow-3", "other-1", "local-1"];
    let hosts = [
        ("slow-1", "slow-host"),
        ("slow-2", "slow-host"),
        ("slow-3", "slow-host"),
        ("other-1", "other-host"),
    ];
    let fixture = lazy_fixture(Arc::clone(&runner), &tasks, &hosts);

    fixture.service.get_task_diff_stats(&ids(&tasks));
    wait_until(|| runner.started_tasks().len() == 5);

    let started: HashSet<String> = runner.started_tasks().into_iter().collect();
    assert_eq!(started, tasks.iter().map(|task| task.to_string()).collect());
    assert_eq!(saved_count(&fixture, &tasks), 0);
    runner.release_all(&single_file_output(1));
    wait_until(|| saved_count(&fixture, &tasks) == 5);
}

#[test]
fn board_refreshes_run_in_background_priority_and_detail_reads_in_foreground() {
    let runner = GatedRunner::new(false, single_file_output(1));
    let fixture = lazy_fixture(
        Arc::clone(&runner),
        &["board", "detail"],
        &[("board", "remote-host"), ("detail", "remote-host")],
    );

    fixture.service.get_task_diff_stats(&ids(&["board"]));
    wait_until(|| fixture.saved_stats("board").is_some());
    fixture.service.get_git_diff_files("detail");

    let priorities: Vec<bool> = runner
        .started
        .lock()
        .unwrap()
        .iter()
        .map(|(_, is_background)| *is_background)
        .collect();
    assert_eq!(priorities, [true, false]);
}

#[test]
fn deleted_tasks_are_not_scheduled() {
    let runner = GatedRunner::new(false, single_file_output(4));
    let fixture = lazy_fixture(Arc::clone(&runner), &["alive"], &[]);

    fixture
        .service
        .get_task_diff_stats(&ids(&["deleted", "alive"]));
    wait_until(|| fixture.saved_stats("alive").is_some());

    assert_eq!(runner.started_tasks(), ["alive"]);
}

#[test]
fn detail_and_board_share_one_read_and_save_for_the_same_task() {
    let runner = GatedRunner::new(true, String::new());
    let fixture = lazy_fixture(Arc::clone(&runner), &["same"], &[]);

    std::thread::scope(|scope| {
        let detail = scope.spawn(|| fixture.service.get_git_diff_files("same"));
        wait_until(|| runner.started_tasks().len() == 1);
        let duplicate_detail = scope.spawn(|| fixture.service.get_git_diff_files("same"));
        fixture.service.get_task_diff_stats(&ids(&["same", "same"]));
        std::thread::sleep(std::time::Duration::from_millis(20));
        runner.release("same", single_file_output(1));
        assert_eq!(detail.join().unwrap(), duplicate_detail.join().unwrap());
    });

    assert_eq!(runner.started_tasks().len(), 1);
    assert_eq!(fixture.saved_stats("same"), Some(stats(1, 1, 0)));
    let later_runner = GatedRunner::new(false, single_file_output(8));
    let files = Arc::new(HostFileAccess::new(
        later_runner as Arc<dyn ShellCommandRunner>,
        "/fixture/home",
    ));
    let later = DiffService::new(Arc::clone(&fixture.database.database), files);
    later.get_git_diff_files("same");
    assert_eq!(later.get_task_diff_stats(&[])["same"].additions, 8);
}

#[test]
fn detail_reads_join_a_board_refresh_already_in_flight() {
    let runner = GatedRunner::new(true, String::new());
    let fixture = lazy_fixture(Arc::clone(&runner), &["one", "two", "three"], &[]);

    fixture
        .service
        .get_task_diff_stats(&ids(&["one", "two", "three"]));
    wait_until(|| runner.started_tasks().len() == 3);
    std::thread::scope(|scope| {
        let detail = scope.spawn(|| fixture.service.get_git_diff_files("three"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(runner.started_tasks().len(), 3);
        runner.release("three", single_file_output(3));
        detail.join().unwrap();
    });
    runner.release("one", single_file_output(1));
    runner.release("two", single_file_output(2));
    wait_until(|| saved_count(&fixture, &["one", "two", "three"]) == 3);

    assert_eq!(runner.started_tasks().len(), 3);
    assert_eq!(
        fixture.service.get_task_diff_stats(&[])["three"].additions,
        3
    );
}

#[test]
fn board_refreshes_run_at_most_the_worker_limit_at_once_and_each_task_once() {
    let runner = GatedRunner::new(true, String::new());
    let tasks: Vec<String> = (0..12).map(|index| format!("task-{index}")).collect();
    let task_refs: Vec<&str> = tasks.iter().map(String::as_str).collect();
    let fixture = lazy_fixture(Arc::clone(&runner), &task_refs, &[]);
    let mut requested = tasks.clone();
    requested.push("task-0".to_string());
    requested.push("deleted".to_string());

    fixture.service.get_task_diff_stats(&requested);
    wait_until(|| runner.started_tasks().len() == DIFF_REFRESH_CONCURRENCY);
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert_eq!(runner.started_tasks().len(), DIFF_REFRESH_CONCURRENCY);

    runner.release_all(&single_file_output(1));
    wait_until(|| runner.started_tasks().len() == tasks.len());
    runner.release_all(&single_file_output(1));
    wait_until(|| saved_count(&fixture, &task_refs) == tasks.len());

    let started: Vec<String> = runner.started_tasks();
    let unique_started: HashSet<&String> = started.iter().collect();
    assert_eq!(started.len(), tasks.len());
    assert_eq!(unique_started.len(), tasks.len());
}
