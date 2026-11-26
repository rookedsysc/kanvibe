//! 서비스 테스트가 함께 쓰는 가짜와 임시 DB. TS 테스트의 `vi.mock("@/lib/database")`,
//! `vi.mock("@/lib/gitOperations")`, `vi.mock("@/lib/boardNotifier")` 자리다.
//! 사용자 홈, git 설정, 네트워크에는 닿지 않는다.

use std::sync::{Arc, Mutex};

use kanvibe_db::repositories::projects::{NewProject, insert_project};
use kanvibe_db::repositories::tasks::{NewKanbanTask, insert_task};
use kanvibe_db::{Database, DatabaseLocation};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, TaskStatus};
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};

use crate::board_notifier::{BoardEvent, BoardEventSubscription, BoardNotifier};

/// 임시 디렉터리 안의 `kanvibe.db`. 디렉터리는 값이 살아 있는 동안 남는다
pub(crate) struct TemporaryDatabase {
    pub database: Arc<Database>,
    pub directory: tempfile::TempDir,
}

impl TemporaryDatabase {
    pub fn open() -> Self {
        let directory = tempfile::tempdir().expect("임시 디렉터리");
        let location = DatabaseLocation::in_app_data_directory(directory.path(), None);
        let database = Arc::new(Database::open(&location).expect("임시 DB"));
        Self {
            database,
            directory,
        }
    }

    pub fn insert_project(&self, project: NewProject) -> Project {
        insert_project(&self.database.connection(), &project).expect("프로젝트 저장")
    }

    pub fn insert_task(&self, task: NewKanbanTask) -> KanbanTask {
        insert_task(&self.database.connection(), &task).expect("작업 저장")
    }
}

/// 필드를 모두 비운 새 프로젝트. 테스트는 필요한 필드만 바꾼다
pub(crate) fn new_project(name: &str, repo_path: &str) -> NewProject {
    NewProject {
        name: name.into(),
        repo_path: repo_path.into(),
        default_branch: "main".into(),
        ssh_host: None,
        is_worktree: false,
        color: None,
        icon_data_url: None,
    }
}

/// 필드를 모두 비운 새 작업
pub(crate) fn new_task(title: &str, status: TaskStatus) -> NewKanbanTask {
    NewKanbanTask {
        title: title.into(),
        description: None,
        status,
        branch_name: None,
        worktree_path: None,
        session_type: None,
        session_name: None,
        ssh_host: None,
        agent_type: None,
        project_id: None,
        base_branch: None,
        pr_url: None,
        priority: None,
    }
}

/// 실행기에 들어온 호출 하나
#[derive(Debug, Clone)]
pub(crate) struct RecordedCommand {
    pub command: String,
    pub ssh_host: Option<String>,
    pub options: ExecGitOptions,
}

type CommandHandler = dyn Fn(&RecordedCommand) -> Result<String, ExecGitError> + Send + Sync;

/// 호출을 기록하고 handler가 정한 결과를 돌려준다
pub(crate) struct ScriptedCommandRunner {
    calls: Mutex<Vec<RecordedCommand>>,
    handler: Box<CommandHandler>,
}

impl ScriptedCommandRunner {
    pub fn new(
        handler: impl Fn(&RecordedCommand) -> Result<String, ExecGitError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            handler: Box::new(handler),
        }
    }

    /// 모든 호출에 같은 stdout을 돌려준다
    pub fn returning(output: &str) -> Self {
        let output = output.to_string();
        Self::new(move |_| Ok(output.clone()))
    }

    pub fn calls(&self) -> Vec<RecordedCommand> {
        self.calls.lock().unwrap().clone()
    }

    pub fn commands(&self) -> Vec<String> {
        self.calls().into_iter().map(|call| call.command).collect()
    }
}

impl ShellCommandRunner for ScriptedCommandRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        let call = RecordedCommand {
            command: command.to_string(),
            ssh_host: ssh_host.map(str::to_string),
            options: options.clone(),
        };
        self.calls.lock().unwrap().push(call.clone());
        (self.handler)(&call)
    }
}

/// 보드 이벤트를 받은 순서대로 모은다
pub(crate) struct RecordedBoardEvents {
    events: Arc<Mutex<Vec<BoardEvent>>>,
    subscription: Mutex<Option<BoardEventSubscription>>,
}

impl RecordedBoardEvents {
    pub fn subscribe(notifier: &Arc<BoardNotifier>) -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let subscription =
            notifier.subscribe(move |event| sink.lock().unwrap().push(event.clone()));
        Self {
            events,
            subscription: Mutex::new(Some(subscription)),
        }
    }

    pub fn events(&self) -> Vec<BoardEvent> {
        self.events.lock().unwrap().clone()
    }

    pub fn unsubscribe(&self) {
        self.subscription.lock().unwrap().take();
    }

    /// `board-updated` 이벤트 개수
    pub fn board_update_count(&self) -> usize {
        self.events()
            .iter()
            .filter(|event| matches!(event, BoardEvent::BoardUpdated))
            .count()
    }
}

/// ssh_host와 상관없이 이 머신의 `sh -c`로 실행해 원격 셸을 흉내 낸다. `exec_git`처럼 앞뒤 공백을 뺀 stdout을 돌려준다.
/// 사용자 홈과 git 전역 설정이 끼어들지 않도록 PATH만 물려받고 HOME과 git 설정 위치는 테스트가 정한다
pub(crate) struct LoopbackShellRunner {
    environment: Vec<(String, String)>,
    calls: Mutex<Vec<RecordedCommand>>,
}

impl LoopbackShellRunner {
    pub fn new(home_directory: &std::path::Path) -> Self {
        let environment = vec![
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("HOME".into(), home_directory.to_string_lossy().into_owned()),
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("LANG".into(), "C.UTF-8".into()),
        ];
        Self {
            environment,
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<RecordedCommand> {
        self.calls.lock().unwrap().clone()
    }

    /// 같은 좁은 환경으로 git을 직접 돌려 fixture 저장소를 만든다
    pub fn git(&self, directory: &std::path::Path, arguments: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .env_clear()
            .envs(self.environment.iter().cloned())
            .output()
            .expect("git 실행");
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl ShellCommandRunner for LoopbackShellRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        self.calls.lock().unwrap().push(RecordedCommand {
            command: command.to_string(),
            ssh_host: ssh_host.map(str::to_string),
            options: options.clone(),
        });
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(command)
            .env_clear()
            .envs(self.environment.iter().cloned())
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| ExecGitError::new(error.to_string()))?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            return Ok(stdout.trim().to_string());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Err(ExecGitError {
            message: format!("Command failed: {command}\n{stderr}"),
            exit_code: output.status.code(),
            stdout,
            stderr,
            ..ExecGitError::default()
        })
    }
}

/// 조건이 참이 될 때까지 기다린다(TS `vi.waitFor`). 2초가 지나면 실패한다
pub(crate) fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "조건을 기다리다 시간이 지났다"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
