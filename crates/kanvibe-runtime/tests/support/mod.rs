//! 런타임 통합 테스트가 함께 쓰는 격리 환경. 임시 HOME·데이터 디렉터리·tmux 소켓 디렉터리·git 저장소를 만들고,
//! 사용자의 실제 KanVibe 데이터, 포트, tmux 서버에는 닿지 않는다.

use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hmac::{Hmac, Mac};
use kanvibe_db::repositories::app_settings::find_app_setting;
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_runtime::{DesktopRuntime, RuntimeConfig};
use kanvibe_services::kanban::CreateTaskInput;
use sha2::Sha256;

pub const REPOSITORY_NAME: &str = "sample-app";
/// 좁힌 환경에 그대로 남아야 하는 값. 걸러야 할 값과 함께 넣어 필터가 전부를 지우지 않았음을 확인한다
pub const ALLOWED_CANARY: (&str, &str) = ("RUNTIME_TEST_ALLOWED", "kept");
/// 스냅샷에 넣지만 자식 환경에는 나타나면 안 되는 서버·런타임 값
pub const BLOCKED_KEYS: [&str; 5] = [
    "PORT",
    "HOST",
    "NODE_ENV",
    "KANVIBE_APP_DATA_DIR",
    "KANVIBE_QA_MODE",
];

pub struct RuntimeFixture {
    root: tempfile::TempDir,
    pub hook_server_port: u16,
}

/// 실제 KanVibe 포트(9736·19736·29736)와 겹치지 않게 OS가 비어 있다고 준 포트를 쓴다. 아무도 듣지 않는다
fn unused_local_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

pub fn has_tmux() -> bool {
    Command::new("tmux").arg("-V").output().is_ok()
}

impl RuntimeFixture {
    pub fn new() -> Self {
        let fixture = Self {
            root: tempfile::tempdir().unwrap(),
            hook_server_port: unused_local_port(),
        };
        for directory in ["home", "data", "tmux", "workspace"] {
            std::fs::create_dir_all(fixture.path(directory)).unwrap();
        }
        fixture
    }

    pub fn path(&self, relative_path: &str) -> PathBuf {
        self.root.path().join(relative_path)
    }

    pub fn home(&self) -> String {
        self.path("home").to_string_lossy().into_owned()
    }

    pub fn workspace(&self) -> String {
        self.path("workspace").to_string_lossy().into_owned()
    }

    /// 앱이 시작할 때 잡아 두는 환경 스냅샷. 서버 런타임 값과 KanVibe 내부 값도 일부러 넣는다
    pub fn base_environment(&self) -> BTreeMap<String, String> {
        let mut environment = self.child_environment();
        for (key, value) in [
            ("PORT", "48123"),
            ("HOST", "0.0.0.0"),
            ("NODE_ENV", "production"),
            ("KANVIBE_QA_MODE", "1"),
            ("SHELL", "/bin/sh"),
            ALLOWED_CANARY,
        ] {
            environment.insert(key.into(), value.into());
        }
        environment.insert(
            "KANVIBE_APP_DATA_DIR".into(),
            self.path("data").to_string_lossy().into_owned(),
        );
        environment
    }

    /// 테스트가 직접 띄우는 git·tmux의 환경. `TMUX_TMPDIR`로 소켓을 임시 디렉터리에 두어 사용자 tmux 서버와 분리한다
    fn child_environment(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("HOME".into(), self.home()),
            (
                "TMUX_TMPDIR".into(),
                self.path("tmux").to_string_lossy().into_owned(),
            ),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ])
    }

    pub fn config(&self) -> RuntimeConfig {
        RuntimeConfig::new(
            self.path("data"),
            self.hook_server_port,
            self.home(),
            self.base_environment(),
        )
    }

    pub fn open_runtime(&self) -> DesktopRuntime {
        DesktopRuntime::new(self.config()).unwrap()
    }

    pub fn run(&self, program: &str, arguments: &[&str], directory: &Path) -> Output {
        Command::new(program)
            .args(arguments)
            .current_dir(directory)
            .env_clear()
            .envs(self.child_environment())
            .output()
            .unwrap()
    }

    pub fn tmux(&self, arguments: &[&str]) -> Output {
        self.run("tmux", arguments, self.root.path())
    }

    /// `main` 브랜치에 커밋 하나가 있는 저장소를 작업 공간에 만든다
    pub fn create_git_repository(&self) -> PathBuf {
        let repository = self.path("workspace").join(REPOSITORY_NAME);
        std::fs::create_dir_all(&repository).unwrap();
        std::fs::write(repository.join("README.md"), "# sample\n").unwrap();
        let identity = [
            "-c",
            "user.name=Runtime Test",
            "-c",
            "user.email=runtime@test.invalid",
        ];
        for arguments in [
            vec!["init", "-q", "-b", "main"],
            vec!["add", "README.md"],
            [identity.as_slice(), &["commit", "-q", "-m", "initial"]].concat(),
        ] {
            let output = self.run("git", &arguments, &repository);
            assert!(output.status.success(), "git {arguments:?}: {output:?}");
        }
        repository
    }
}

impl Drop for RuntimeFixture {
    /// 이 테스트가 띄운 격리 tmux 서버만 끈다. `TMUX_TMPDIR`이 임시 디렉터리라 사용자 서버에는 닿지 않는다
    fn drop(&mut self) {
        if has_tmux() {
            let _ = self.tmux(&["kill-server"]);
        }
    }
}

/// 저장소를 스캔해 프로젝트로 등록하고 그 프로젝트를 돌려준다
pub fn register_sample_project(runtime: &DesktopRuntime, fixture: &RuntimeFixture) -> Project {
    let scan = runtime
        .project()
        .scan_and_register_projects(&fixture.workspace(), None)
        .unwrap();
    assert_eq!(scan.registered.len(), 1, "{scan:?}");
    runtime
        .project()
        .get_all_projects()
        .unwrap()
        .into_iter()
        .find(|project| project.name == REPOSITORY_NAME)
        .unwrap()
}

pub fn create_branch_task(
    runtime: &DesktopRuntime,
    project: &Project,
    branch_name: &str,
    session_type: SessionType,
) -> KanbanTask {
    runtime
        .kanban()
        .create_task(&CreateTaskInput {
            title: branch_name.to_string(),
            branch_name: Some(branch_name.to_string()),
            base_branch: Some("main".into()),
            session_type: Some(session_type),
            project_id: Some(project.id.clone()),
            ..CreateTaskInput::default()
        })
        .unwrap()
}

pub fn app_setting(runtime: &DesktopRuntime, key: &str) -> Option<String> {
    find_app_setting(&runtime.database().connection(), key)
        .unwrap()
        .map(|setting| setting.value)
}

/// 토큰 계산을 런타임 코드와 독립적으로 다시 한다
pub fn expected_task_token(runtime: &DesktopRuntime, task_id: &str) -> String {
    let secret = app_setting(runtime, "task_cli_secret").expect("task_cli_secret");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(task_id.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// worktree `.kanvibe/targets.json`에서 이 task의 알림 대상
pub fn read_own_target(worktree_path: &str, task_id: &str) -> serde_json::Value {
    let targets_path = Path::new(worktree_path).join(".kanvibe/targets.json");
    let content = std::fs::read_to_string(&targets_path).unwrap();
    let state: serde_json::Value = serde_json::from_str(&content).unwrap();
    state["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["taskId"] == task_id)
        .cloned()
        .unwrap_or_else(|| panic!("{task_id} 대상이 없다: {content}"))
}

/// 환경 변수 목록(`KEY=VALUE` 줄 또는 NUL 구분)에서 이름만 모은다
pub fn environment_keys(listing: &str) -> Vec<String> {
    listing
        .split(['\n', '\0'])
        .filter_map(|entry| entry.split_once('=').map(|(key, _)| key.to_string()))
        .collect()
}

pub fn assert_runtime_values_absent(keys: &[String], source: &str) {
    for blocked_key in BLOCKED_KEYS {
        assert!(
            !keys.iter().any(|key| key == blocked_key),
            "{source}에 {blocked_key}가 새었다: {keys:?}"
        );
    }
    assert!(
        !keys.iter().any(|key| key.starts_with("KANVIBE_")),
        "{source}에 KANVIBE_* 값이 새었다: {keys:?}"
    );
    assert!(
        keys.iter().any(|key| key == ALLOWED_CANARY.0),
        "{source}에서 일반 값까지 사라졌다: {keys:?}"
    );
}
