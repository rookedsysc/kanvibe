//! 경계의 실제 구현: 시스템 `ssh` 터널 프로세스, DB의 원격 호스트 목록과 hook client ID, 상태 파일 반영.

use std::collections::HashSet;
use std::io::Read;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::kanvibe_state_store::HostKanvibeStateStore;
use kanvibe_process::shell_environment::{
    ShellEnvironment, ShellPlatform, create_local_shell_environment, current_user_home_directory,
};
use kanvibe_process::task_state_service::BoardUpdateNotifier;
use kanvibe_process::task_state_service::status_sync::TaskStatusSync;

use super::boundaries::{
    HookClientIdSource, HookTunnelProcess, HookTunnelProcessExit, HookTunnelProcessSpawner,
    HostTaskStatusRefresher, RemoteHostsSource,
};

/// 실패 사유로 남기는 ssh stderr 끝부분 길이. 여러 조각으로 나뉘어 와도 이어 붙여 끝부분을 둔다
const LAST_ERROR_OUTPUT_CHARACTERS: usize = 4096;

/// 시스템 `ssh`로 터널을 띄운다. 자식에는 서버 런타임 값(`PORT`, `HOST`, `NODE_ENV`, `KANVIBE_*`)을 걷어 낸 환경만 넘긴다
pub struct SystemHookTunnelProcessSpawner {
    environment: ShellEnvironment,
}

impl SystemHookTunnelProcessSpawner {
    /// `parent_environment`는 읽기만 한다. 보통 지금 프로세스의 환경을 한 번 모아 넘긴다
    pub fn from_parent_environment(parent_environment: &ShellEnvironment) -> Self {
        let environment = create_local_shell_environment(
            parent_environment,
            ShellPlatform::current(),
            &current_user_home_directory(),
        );
        Self { environment }
    }

    pub fn environment(&self) -> &ShellEnvironment {
        &self.environment
    }
}

impl HookTunnelProcessSpawner for SystemHookTunnelProcessSpawner {
    fn spawn(&self, ssh_arguments: &[String]) -> Result<Arc<dyn HookTunnelProcess>, String> {
        let mut child = Command::new("ssh")
            .args(ssh_arguments)
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        let last_error_output = Arc::new(Mutex::new(String::new()));
        if let Some(stderr) = child.stderr.take() {
            let output = Arc::clone(&last_error_output);
            std::thread::spawn(move || keep_last_error_output(stderr, &output));
        }
        Ok(Arc::new(SystemHookTunnelProcess {
            process_id: child.id(),
            // 앱이 비정상 종료되면 이 stdin이 닫혀 원격 `cat`이 끝나고 ssh도 함께 끝난다
            stdin: Mutex::new(child.stdin.take()),
            child: Mutex::new(Some(child)),
            has_exited: AtomicBool::new(false),
            last_error_output,
        }))
    }
}

fn keep_last_error_output(mut stderr: impl Read, output: &Mutex<String>) {
    let mut buffer = [0u8; 4096];
    while let Ok(count) = stderr.read(&mut buffer) {
        if count == 0 {
            return;
        }
        let mut output = output.lock().unwrap();
        output.push_str(&String::from_utf8_lossy(&buffer[..count]));
        let skipped = output
            .chars()
            .count()
            .saturating_sub(LAST_ERROR_OUTPUT_CHARACTERS);
        *output = output.chars().skip(skipped).collect();
    }
}

struct SystemHookTunnelProcess {
    process_id: u32,
    stdin: Mutex<Option<ChildStdin>>,
    child: Mutex<Option<Child>>,
    has_exited: AtomicBool,
    last_error_output: Arc<Mutex<String>>,
}

impl HookTunnelProcess for SystemHookTunnelProcess {
    fn kill(&self) {
        if self.has_exited.load(Ordering::SeqCst) {
            return;
        }
        // SAFETY: 아직 회수하지 않은 자식 ssh의 pid에만 신호를 보낸다
        unsafe {
            libc::kill(self.process_id as libc::pid_t, libc::SIGTERM);
        }
    }

    fn wait(&self) -> HookTunnelProcessExit {
        let child = self.child.lock().unwrap().take();
        let exit_code = child.and_then(|mut child| child.wait().ok()?.code());
        self.has_exited.store(true, Ordering::SeqCst);
        self.stdin.lock().unwrap().take();
        HookTunnelProcessExit {
            exit_code,
            last_error_output: self.last_error_output.lock().unwrap().clone(),
        }
    }
}

/// DB에서 원격 task·원격 프로젝트의 호스트를 읽는다. 완료한 task도 worktree의 hook이 쓰므로 상태로 거르지 않는다
pub struct DatabaseRemoteHosts {
    database: Arc<Database>,
}

impl DatabaseRemoteHosts {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

const REMOTE_HOSTS_SQL: &str = "SELECT ssh_host FROM kanban_tasks WHERE ssh_host IS NOT NULL \
     UNION ALL SELECT ssh_host FROM projects WHERE ssh_host IS NOT NULL";

impl RemoteHostsSource for DatabaseRemoteHosts {
    fn remote_ssh_hosts(&self) -> Result<Vec<String>, String> {
        let connection = self.database.connection();
        let mut statement = connection
            .prepare(REMOTE_HOSTS_SQL)
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.map(|row| row.map_err(|error| error.to_string()))
            .filter(|host| host.as_ref().map_or(true, |host| !host.is_empty()))
            .collect()
    }
}

/// `app_settings`의 hook client ID. 처음 부를 때 만든다
pub struct DatabaseHookClientIds {
    database: Arc<Database>,
}

impl DatabaseHookClientIds {
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

impl HookClientIdSource for DatabaseHookClientIds {
    fn hook_client_id(&self) -> Result<String, String> {
        AppSettingsService::new(&self.database, ShortcutPlatform::current())
            .get_or_create_hook_client_id()
            .map_err(|error| error.to_string())
    }
}

/// 원격 상태 파일을 `TaskStatusSync`로 DB에 반영한다
pub struct TaskStatusFileRefresher {
    database: Arc<Database>,
    files: Arc<HostFileAccess>,
    board_notifier: Arc<dyn BoardUpdateNotifier>,
}

impl TaskStatusFileRefresher {
    pub fn new(
        database: Arc<Database>,
        files: Arc<HostFileAccess>,
        board_notifier: Arc<dyn BoardUpdateNotifier>,
    ) -> Self {
        Self {
            database,
            files,
            board_notifier,
        }
    }
}

impl HostTaskStatusRefresher for TaskStatusFileRefresher {
    fn refresh_host_task_statuses(
        &self,
        ssh_hosts: &HashSet<String>,
        is_current: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        let state_store = HostKanvibeStateStore::new(&self.files);
        TaskStatusSync::new(&self.database, &state_store, self.board_notifier.as_ref())
            .refresh_host_task_statuses(ssh_hosts, is_current)
    }
}
