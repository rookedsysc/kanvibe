//! 공유 서버 `kanvibe`에서 task Space 키와 Herdr workspace ID를 잇는 연결 파일(`kanvibe-workspaces.json`).
//!
//! 공유 세션의 Herdr 명령은 실행할 때마다 이 파일로 자기 Space를 찾는다. 원격에서는 그 읽기 하나가 SSH 왕복이라,
//! 터미널 하나를 여는 동안 이어지는 명령들이 같은 파일을 수십 번 읽는다. 짧은 동안은 한 번 읽은 내용을 함께 쓴다.
//! 이 프로세스가 파일을 고치면 곧바로 버리고, 다른 KanVibe 프로세스가 고친 내용은 이 시간이 지난 뒤에 보인다.

use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use kanvibe_domain::project::posix_dirname;
use kanvibe_process::exec::ExecGitOptions;
use kanvibe_process::host_file_access::HostFileError;
use regex::Regex;
use serde_json::{Map, Value};

use super::response::{HerdrWorkspaceList, parse_json};
use super::shared_outcome::SharedOutcome;
use super::{HERDR_START_TIMEOUT, HerdrError, HerdrService, parse_target};
use crate::herdr::build_herdr_session_cleanup_command;

const WORKSPACE_MAPPING_REUSE: Duration = Duration::from_millis(2_000);
const WORKSPACE_MAPPING_FILE_NAME: &str = "kanvibe-workspaces.json";
/// 연결 파일에는 다른 task의 Space ID도 있으므로 소유자만 읽고 쓴다
const WORKSPACE_MAPPING_FILE_MODE: u32 = 0o600;

static WORKSPACE_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^w[0-9A-Za-z]+$").expect("유효한 정규식"));

/// task Space 키 → workspace ID. 키 순서는 파일에 적힌 순서를 따른다(JS 객체와 같다)
pub(super) type WorkspaceMapping = Map<String, Value>;

type MappingOutcome = Result<WorkspaceMapping, HerdrError>;

pub(super) struct WorkspaceMappingRead {
    read_at: Instant,
    mapping: Arc<SharedOutcome<MappingOutcome>>,
}

pub(super) fn parse_workspace_mapping(content: &str) -> MappingOutcome {
    if content.is_empty() {
        return Ok(WorkspaceMapping::new());
    }
    let invalid = || HerdrError::Message("Herdr Space 연결 파일이 올바르지 않습니다.".to_string());
    let Value::Object(mapping) = parse_json(content)? else {
        return Err(invalid());
    };
    let is_valid = mapping.values().all(|workspace_id| {
        workspace_id
            .as_str()
            .is_some_and(|workspace_id| WORKSPACE_ID_PATTERN.is_match(workspace_id))
    });
    if !is_valid {
        return Err(invalid());
    }
    Ok(mapping)
}

pub(super) fn mapped_workspace_id(
    mapping: &WorkspaceMapping,
    workspace_key: &str,
) -> Option<String> {
    mapping
        .get(workspace_key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn host_key(ssh_host: Option<&str>) -> String {
    ssh_host.unwrap_or_default().to_string()
}

/// Node `path.posix.join(directory, name)`. Herdr가 주는 경로는 이미 정규화된 절대 경로다
pub(super) fn join_posix_path(directory: &str, name: &str) -> String {
    format!("{}/{name}", directory.trim_end_matches('/'))
}

impl HerdrService {
    pub(super) fn get_workspace_mapping_path(
        &self,
        ssh_host: Option<&str>,
    ) -> Result<String, HerdrError> {
        let key = host_key(ssh_host);
        if let Some(cached) = self.workspace_mapping_paths.lock().unwrap().get(&key) {
            return Ok(cached.clone());
        }
        let shared = self
            .list_herdr_sessions(ssh_host)?
            .into_iter()
            .find(|session| session.name == "kanvibe");
        let Some(session_dir) = shared
            .and_then(|session| session.session_dir)
            .filter(|session_dir| !session_dir.is_empty())
        else {
            return Err(HerdrError::Message(
                "공유 Herdr 세션 경로를 찾을 수 없습니다.".to_string(),
            ));
        };
        let mapping_path = join_posix_path(&session_dir, WORKSPACE_MAPPING_FILE_NAME);
        self.workspace_mapping_paths
            .lock()
            .unwrap()
            .insert(key, mapping_path.clone());
        Ok(mapping_path)
    }

    /// 캐시를 거치지 않고 연결 파일을 바로 읽는다
    pub(super) fn read_workspace_mapping_file(&self, ssh_host: Option<&str>) -> MappingOutcome {
        let mapping_path = self.get_workspace_mapping_path(ssh_host)?;
        let content =
            self.files
                .read_text_file(&mapping_path, ssh_host, &ExecGitOptions::default())?;
        parse_workspace_mapping(&content)
    }

    /// 최근 2초 안에 시작한 읽기가 있으면 그 결과를 함께 쓴다. 실패한 읽기는 다음 호출에 물려주지 않는다
    fn read_workspace_mapping(&self, ssh_host: Option<&str>) -> MappingOutcome {
        let key = host_key(ssh_host);
        let now = self.now();
        let mapping = {
            let mut reads = self.workspace_mapping_reads.lock().unwrap();
            if let Some(recent) = reads.get(&key)
                && now.duration_since(recent.read_at) < WORKSPACE_MAPPING_REUSE
            {
                let shared = Arc::clone(&recent.mapping);
                drop(reads);
                return shared.wait();
            }
            let mapping = SharedOutcome::new();
            let read = WorkspaceMappingRead {
                read_at: now,
                mapping: Arc::clone(&mapping),
            };
            reads.insert(key.clone(), read);
            mapping
        };
        let outcome = self.read_workspace_mapping_file(ssh_host);
        if outcome.is_err() {
            let mut reads = self.workspace_mapping_reads.lock().unwrap();
            if reads
                .get(&key)
                .is_some_and(|read| Arc::ptr_eq(&read.mapping, &mapping))
            {
                reads.remove(&key);
            }
        }
        mapping.complete(outcome.clone());
        outcome
    }

    pub(super) fn forget_workspace_mapping(&self, ssh_host: Option<&str>) {
        self.workspace_mapping_reads
            .lock()
            .unwrap()
            .remove(&host_key(ssh_host));
    }

    /// TS `getHerdrWorkspaceId`. 공유 서버 세션이 아니면 `None`, 연결된 Space가 없으면 오류다
    pub fn get_herdr_workspace_id(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Option<String>, HerdrError> {
        let Some(workspace_key) = parse_target(session_name)?.workspace_key else {
            return Ok(None);
        };
        let mapping = self.read_workspace_mapping(ssh_host)?;
        match mapped_workspace_id(&mapping, &workspace_key) {
            Some(workspace_id) => Ok(Some(workspace_id)),
            None => Err(HerdrError::Message(
                "task에 연결된 Herdr Space가 없습니다.".to_string(),
            )),
        }
    }

    /// TS `getHerdrClientSocketPath`. 실행 중인 서버 소켓 옆의 client 소켓 경로
    pub fn get_herdr_client_socket_path(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<String, HerdrError> {
        let server_name = parse_target(session_name)?.server_name;
        let session = self.find_running_session(&server_name, ssh_host)?;
        let socket_path = session.socket_path.unwrap_or_default();
        Ok(join_posix_path(
            posix_dirname(&socket_path),
            "herdr-client.sock",
        ))
    }

    /// TS `isHerdrTaskAlive`. Space 연결 파일 읽기와 Space 목록 조회는 서로의 결과를 쓰지 않으므로 원격 왕복이 겹치게 한다
    pub fn is_herdr_task_alive(&self, session_name: &str, ssh_host: Option<&str>) -> bool {
        let Ok(target) = parse_target(session_name) else {
            return false;
        };
        let (workspace_id, workspaces) = std::thread::scope(|scope| {
            let workspaces = scope.spawn(|| {
                self.query::<HerdrWorkspaceList>(
                    &target.server_name,
                    &["workspace", "list"],
                    ssh_host,
                )
            });
            let workspace_id = self.get_herdr_workspace_id(session_name, ssh_host);
            (
                workspace_id,
                workspaces.join().expect("Space 목록 조회 스레드"),
            )
        });
        match (workspace_id, workspaces) {
            (Ok(None), Ok(_)) => true,
            (Ok(Some(workspace_id)), Ok(workspaces)) => workspaces.contains(&workspace_id),
            _ => false,
        }
    }

    /// TS `closeHerdrTaskWorkspace`. 독립 서버는 세션째 정리하고, 공유 서버는 그 task의 Space만 닫고 연결을 지운다
    pub fn close_herdr_task_workspace(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HerdrError> {
        let target = parse_target(session_name)?;
        let Some(workspace_key) = target.workspace_key else {
            let cleanup =
                build_herdr_session_cleanup_command(session_name).map_err(HerdrError::Message)?;
            let options = ExecGitOptions {
                timeout: Some(HERDR_START_TIMEOUT),
                ..ExecGitOptions::default()
            };
            self.runner.exec_git(&cleanup, ssh_host, &options)?;
            return Ok(());
        };
        let mapping_path = self.get_workspace_mapping_path(ssh_host)?;
        self.update_workspace_mapping(&mapping_path, ssh_host, |content| {
            let mut mapping = parse_workspace_mapping(content)?;
            let Some(workspace_id) = mapped_workspace_id(&mapping, &workspace_key) else {
                return Ok(content.to_string());
            };
            let live: HerdrWorkspaceList =
                self.query(&target.server_name, &["workspace", "list"], ssh_host)?;
            if live.contains(&workspace_id) {
                self.run_herdr_command(
                    &target.server_name,
                    &["workspace", "close", &workspace_id],
                    ssh_host,
                )?;
            }
            mapping.shift_remove(&workspace_key);
            Ok(Value::Object(mapping).to_string())
        })?;
        self.forget_workspace_mapping(ssh_host);
        Ok(())
    }

    /// 파일 잠금 안에서 연결 파일을 고친다. 갱신 중 실패는 원래 오류 그대로 돌려준다
    pub(super) fn update_workspace_mapping(
        &self,
        mapping_path: &str,
        ssh_host: Option<&str>,
        mut update: impl FnMut(&str) -> Result<String, HerdrError>,
    ) -> Result<(), HerdrError> {
        let mut update_failure: Option<HerdrError> = None;
        let mut file_update = |content: &str| {
            update(content).map_err(|error| {
                let message = error.to_string();
                update_failure = Some(error);
                HostFileError::Message(message)
            })
        };
        let result = self.files.update_text_file(
            mapping_path,
            ssh_host,
            Some(WORKSPACE_MAPPING_FILE_MODE),
            &ExecGitOptions::default(),
            &mut file_update,
        );
        match (result, update_failure) {
            (Ok(()), _) => Ok(()),
            (Err(_), Some(error)) => Err(error),
            (Err(error), None) => Err(error.into()),
        }
    }
}
