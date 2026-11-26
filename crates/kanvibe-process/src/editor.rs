//! 작업 폴더를 로컬 VS Code로 연다(TS `editorService.ts`). 원격 작업이면 Remote-SSH로 붙어서 연다.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use kanvibe_db::repositories::{projects::find_project, tasks::find_task};
use kanvibe_db::{Database, DatabaseResult};

use crate::shell_environment::ShellEnvironment;

pub const VS_CODE_COMMAND: &str = "code";
const VS_CODE_LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);
const NO_TARGET_PATH_ERROR: &str = "no-target-path";

/// TS `EditorOpenResult { ok, error? }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorOpenResult {
    pub is_ok: bool,
    pub error: Option<String>,
}

/// 편집기 프로세스를 띄운다. 실패하면 Node `execFile` 오류 메시지를 돌려준다
pub trait EditorProcessLauncher {
    fn launch(
        &self,
        program: &str,
        arguments: &[String],
        environment: &ShellEnvironment,
        timeout: Duration,
    ) -> Result<(), String>;
}

/// 실제 프로세스를 띄우는 실행기. 넘겨받은 좁은 셸 환경만 자식에게 준다
pub struct SystemEditorProcessLauncher;

fn spawn_error_message(program: &str, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => format!("spawn {program} ENOENT"),
        std::io::ErrorKind::PermissionDenied => format!("spawn {program} EACCES"),
        _ => format!("spawn {program} {error}"),
    }
}

impl EditorProcessLauncher for SystemEditorProcessLauncher {
    fn launch(
        &self,
        program: &str,
        arguments: &[String],
        environment: &ShellEnvironment,
        timeout: Duration,
    ) -> Result<(), String> {
        let mut child = Command::new(program)
            .args(arguments)
            .env_clear()
            .envs(environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| spawn_error_message(program, &error))?;
        let process_id = child.id();
        let mut stderr_stream = child.stderr.take().expect("piped stderr");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stderr = String::new();
            let _ = stderr_stream.read_to_string(&mut stderr);
            let _ = sender.send((child.wait(), stderr));
        });
        let command_line = std::iter::once(program.to_string())
            .chain(arguments.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        let (status, stderr) = match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => {
                // SAFETY: 아직 회수하지 않은 자식 프로세스의 pid에만 신호를 보낸다
                unsafe { libc::kill(process_id as libc::pid_t, libc::SIGTERM) };
                return Err(format!("Command failed: {command_line}\n"));
            }
        };
        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(format!("Command failed: {command_line}\n{stderr}")),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// VS Code CLI 인자. 원격 작업이라도 명령은 항상 로컬에서 돈다.
/// 원격 셸에서 `code`를 실행하면 그 머신에서 창을 띄우려 하므로, 로컬 VS Code에 `--remote ssh-remote+<host>`로 붙을 곳을 알려 준다.
pub fn build_vs_code_open_args(target_path: &str, ssh_host: Option<&str>) -> Vec<String> {
    match ssh_host.filter(|host| !host.is_empty()) {
        Some(host) => vec![
            "--remote".into(),
            format!("ssh-remote+{host}"),
            target_path.into(),
        ],
        None => vec![target_path.into()],
    }
}

struct EditorTarget {
    target_path: String,
    ssh_host: Option<String>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

/// 작업을 열 경로와 호스트. worktree가 있으면 그 작업의 실제 작업 사본이므로 프로젝트 저장소보다 먼저 쓴다
fn resolve_task_editor_target(
    database: &Database,
    task_id: &str,
) -> DatabaseResult<Option<EditorTarget>> {
    let connection = database.connection();
    let Some(task) = find_task(&connection, task_id)? else {
        return Ok(None);
    };
    let project = match non_empty(task.project_id.clone()) {
        Some(project_id) => find_project(&connection, &project_id)?,
        None => None,
    };
    let project_repo_path = project.as_ref().map(|project| project.repo_path.clone());
    let project_ssh_host = project.and_then(|project| project.ssh_host);
    let Some(target_path) = non_empty(task.worktree_path).or(non_empty(project_repo_path)) else {
        return Ok(None);
    };
    Ok(Some(EditorTarget {
        target_path,
        ssh_host: non_empty(task.ssh_host).or(non_empty(project_ssh_host)),
    }))
}

/// 작업 폴더를 로컬 VS Code로 연다. 원격 작업이면 Remote-SSH로 붙어서 연다
pub fn open_task_in_vs_code(
    database: &Database,
    launcher: &dyn EditorProcessLauncher,
    environment: &ShellEnvironment,
    task_id: &str,
) -> DatabaseResult<EditorOpenResult> {
    let Some(target) = resolve_task_editor_target(database, task_id)? else {
        return Ok(EditorOpenResult {
            is_ok: false,
            error: Some(NO_TARGET_PATH_ERROR.into()),
        });
    };
    let arguments = build_vs_code_open_args(&target.target_path, target.ssh_host.as_deref());
    match launcher.launch(
        VS_CODE_COMMAND,
        &arguments,
        environment,
        VS_CODE_LAUNCH_TIMEOUT,
    ) {
        Ok(()) => Ok(EditorOpenResult {
            is_ok: true,
            error: None,
        }),
        Err(message) => {
            eprintln!("VS Code 실행 실패: {message}");
            Ok(EditorOpenResult {
                is_ok: false,
                error: Some(message),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use kanvibe_db::DatabaseLocation;
    use kanvibe_db::repositories::projects::{NewProject, insert_project};
    use kanvibe_db::repositories::tasks::{NewKanbanTask, insert_task};
    use kanvibe_domain::task::TaskStatus;

    use super::*;

    #[derive(Default)]
    struct RecordingLauncher {
        launches: RefCell<Vec<(String, Vec<String>, ShellEnvironment)>>,
        failure: Option<String>,
    }

    impl EditorProcessLauncher for RecordingLauncher {
        fn launch(
            &self,
            program: &str,
            arguments: &[String],
            environment: &ShellEnvironment,
            _timeout: Duration,
        ) -> Result<(), String> {
            self.launches.borrow_mut().push((
                program.into(),
                arguments.to_vec(),
                environment.clone(),
            ));
            self.failure.clone().map_or(Ok(()), Err)
        }
    }

    struct Fixture {
        _directory: tempfile::TempDir,
        database: Database,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let database = Database::open(&DatabaseLocation::in_app_data_directory(
                directory.path(),
                None,
            ))
            .unwrap();
            Self {
                _directory: directory,
                database,
            }
        }

        fn insert_project(&self, repo_path: &str, ssh_host: Option<&str>) -> String {
            let project = NewProject {
                name: "repo".into(),
                repo_path: repo_path.into(),
                default_branch: "main".into(),
                ssh_host: ssh_host.map(str::to_string),
                is_worktree: false,
                color: None,
                icon_data_url: None,
            };
            insert_project(&self.database.connection(), &project)
                .unwrap()
                .id
        }

        fn insert_task(&self, project_id: Option<&str>, worktree_path: Option<&str>) -> String {
            let task = NewKanbanTask {
                title: "task".into(),
                description: None,
                status: TaskStatus::Todo,
                branch_name: None,
                worktree_path: worktree_path.map(str::to_string),
                session_type: None,
                session_name: None,
                ssh_host: None,
                agent_type: None,
                project_id: project_id.map(str::to_string),
                base_branch: None,
                pr_url: None,
                priority: None,
            };
            insert_task(&self.database.connection(), &task).unwrap().id
        }

        fn open(&self, launcher: &RecordingLauncher, task_id: &str) -> EditorOpenResult {
            let environment: ShellEnvironment =
                [("PATH".to_string(), "/usr/local/bin".to_string())].into();
            open_task_in_vs_code(&self.database, launcher, &environment, task_id).unwrap()
        }
    }

    fn opened() -> EditorOpenResult {
        EditorOpenResult {
            is_ok: true,
            error: None,
        }
    }

    #[test]
    fn local_task_opens_only_the_worktree_path() {
        let fixture = Fixture::new();
        let project_id = fixture.insert_project("/workspace/repo", None);
        let task_id = fixture.insert_task(
            Some(&project_id),
            Some("/workspace/repo__worktrees/feature"),
        );
        let launcher = RecordingLauncher::default();

        assert_eq!(fixture.open(&launcher, &task_id), opened());

        let (program, arguments, environment) = launcher.launches.borrow()[0].clone();
        assert_eq!(program, "code");
        assert_eq!(arguments, ["/workspace/repo__worktrees/feature"]);
        assert_eq!(
            environment,
            [("PATH".to_string(), "/usr/local/bin".to_string())].into()
        );
    }

    #[test]
    fn remote_task_attaches_local_vs_code_through_remote_ssh() {
        let fixture = Fixture::new();
        let project_id = fixture.insert_project("/remote/repo", Some("build-box"));
        let task_id =
            fixture.insert_task(Some(&project_id), Some("/remote/repo__worktrees/feature"));
        let launcher = RecordingLauncher::default();

        assert_eq!(fixture.open(&launcher, &task_id), opened());

        assert_eq!(
            launcher.launches.borrow()[0].1,
            [
                "--remote",
                "ssh-remote+build-box",
                "/remote/repo__worktrees/feature"
            ]
        );
    }

    #[test]
    fn task_without_worktree_opens_the_project_repository() {
        let fixture = Fixture::new();
        let project_id = fixture.insert_project("/workspace/repo", None);
        let task_id = fixture.insert_task(Some(&project_id), None);
        let launcher = RecordingLauncher::default();

        fixture.open(&launcher, &task_id);

        assert_eq!(launcher.launches.borrow()[0].1, ["/workspace/repo"]);
    }

    #[test]
    fn task_without_any_path_does_not_launch_vs_code() {
        let fixture = Fixture::new();
        let task_id = fixture.insert_task(None, None);
        let launcher = RecordingLauncher::default();

        let result = fixture.open(&launcher, &task_id);

        assert_eq!(
            result,
            EditorOpenResult {
                is_ok: false,
                error: Some("no-target-path".into())
            }
        );
        assert!(launcher.launches.borrow().is_empty());
    }

    #[test]
    fn missing_code_command_returns_the_failure_reason() {
        let fixture = Fixture::new();
        let task_id = fixture.insert_task(None, Some("/workspace/repo__worktrees/feature"));
        let launcher = RecordingLauncher {
            failure: Some("spawn code ENOENT".into()),
            ..RecordingLauncher::default()
        };

        let result = fixture.open(&launcher, &task_id);

        assert_eq!(
            result,
            EditorOpenResult {
                is_ok: false,
                error: Some("spawn code ENOENT".into())
            }
        );
    }

    #[test]
    fn system_launcher_reports_a_missing_program_like_node() {
        let environment: ShellEnvironment =
            [("PATH".to_string(), "/nonexistent".to_string())].into();

        let error = SystemEditorProcessLauncher
            .launch(
                "kanvibe-missing-editor",
                &[],
                &environment,
                Duration::from_secs(1),
            )
            .unwrap_err();

        assert_eq!(error, "spawn kanvibe-missing-editor ENOENT");
    }
}
