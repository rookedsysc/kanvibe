//! Herdr 서버 기동과 task workspace 준비. 같은 세션의 동시 연결은 진행 중인 준비에 합류하고,
//! 공유 서버의 Space 준비는 호스트마다 하나씩 실행해 연결 파일 갱신이 서로 엇갈리지 않게 한다.

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use kanvibe_ai::javascript::js_trim;
use kanvibe_domain::pane_layout::PaneLayoutType;
use kanvibe_process::exec::ExecGitOptions;
use kanvibe_process::host_file_access::quote_shell_argument;
use kanvibe_process::worktree::PaneLayoutSelection;
use serde::Deserialize;

use super::response::{HerdrPane, HerdrPaneList, HerdrWorkspaceList};
use super::shared_outcome::SharedOutcome;
use super::workspace_mapping::{mapped_workspace_id, parse_workspace_mapping};
use super::{HERDR_START_TIMEOUT, HerdrError, HerdrService, herdr_command, parse_target};
use crate::herdr::build_herdr_session_cleanup_command;

const SERVER_START_POLL_INTERVAL: Duration = Duration::from_millis(100);

type BootstrapOutcome = Result<Option<String>, HerdrError>;
type BootstrapFlight = Arc<SharedOutcome<BootstrapOutcome>>;

/// 진행 중인 부트스트랩. `by_session`은 (호스트, 세션 이름), `by_shared_host`는 공유 서버 Space 준비의 호스트(로컬은 빈 문자열) 기준이다
#[derive(Default)]
pub(super) struct BootstrapFlights {
    by_session: HashMap<(Option<String>, String), BootstrapFlight>,
    by_shared_host: HashMap<String, BootstrapFlight>,
}

#[derive(Deserialize)]
struct CreatedRootPane {
    pane_id: String,
}

#[derive(Deserialize)]
struct CreatedWorkspace {
    root_pane: CreatedRootPane,
}

#[derive(Deserialize)]
struct SplitPane {
    pane: CreatedRootPane,
}

/// Herdr는 서버 프로세스가 자기 세션의 리더일 때만 원격 서버를 로컬 machine으로 저장한다. nohup만으로는 ssh 명령 셸의 세션에 남아 등록이 거부된다.
/// setsid가 없는 원격(macOS 등)은 perl로 세션을 분리하고, 둘 다 없으면 machine 등록은 포기하더라도 서버는 띄울 수 있게 nohup으로 둔다.
/// 명령 앞에 exec를 붙여야 분리된 셸 프로세스가 그대로 Herdr 서버가 되어 세션 리더 자격을 이어받는다.
pub(super) fn build_detached_remote_server_launch_command(command: &str) -> String {
    let server_shell = format!(
        "sh -c {} </dev/null >/dev/null 2>&1 &",
        quote_shell_argument(&format!("exec {command}"))
    );
    [
        format!("if command -v setsid >/dev/null 2>&1; then setsid {server_shell}"),
        format!(
            "elif command -v perl >/dev/null 2>&1; then perl -MPOSIX -e 'POSIX::setsid() or die; exec @ARGV or die' {server_shell}"
        ),
        format!("else nohup {server_shell}"),
        "fi".to_string(),
    ]
    .join("\n")
}

impl HerdrService {
    /// TS `ensureHerdrSession`. 공유 서버 세션이면 task Space ID를, 독립 서버면 `None`을 돌려준다.
    /// 같은 (호스트, 세션)의 두 번째 호출은 진행 중인 준비가 끝나기를 기다려 같은 결과를 받는다
    pub fn ensure_herdr_session(
        &self,
        session_name: &str,
        cwd: &str,
        layout: Option<&PaneLayoutSelection>,
        ssh_host: Option<&str>,
    ) -> Result<Option<String>, HerdrError> {
        let session_key = (ssh_host.map(str::to_string), session_name.to_string());
        let shared_key = ssh_host.unwrap_or_default().to_string();
        let is_shared = parse_target(session_name)?.workspace_key.is_some();
        let (flight, previous) = {
            let mut flights = self.bootstraps.lock().unwrap();
            if let Some(current) = flights.by_session.get(&session_key) {
                let current = Arc::clone(current);
                drop(flights);
                return current.wait();
            }
            let flight: BootstrapFlight = SharedOutcome::new();
            let previous = match is_shared {
                true => flights
                    .by_shared_host
                    .insert(shared_key.clone(), Arc::clone(&flight)),
                false => None,
            };
            flights
                .by_session
                .insert(session_key.clone(), Arc::clone(&flight));
            (flight, previous)
        };
        if let Some(previous) = previous {
            // 앞선 Space 준비의 성공 여부와 상관없이 끝나기만 기다린다.
            let _ = previous.wait();
        }
        let outcome = self.bootstrap_herdr_session(session_name, cwd, layout, ssh_host);
        {
            let mut flights = self.bootstraps.lock().unwrap();
            flights.by_session.remove(&session_key);
            if flights
                .by_shared_host
                .get(&shared_key)
                .is_some_and(|current| Arc::ptr_eq(current, &flight))
            {
                flights.by_shared_host.remove(&shared_key);
            }
        }
        flight.complete(outcome.clone());
        outcome
    }

    fn bootstrap_herdr_session(
        &self,
        session_name: &str,
        cwd: &str,
        layout: Option<&PaneLayoutSelection>,
        ssh_host: Option<&str>,
    ) -> BootstrapOutcome {
        let target = parse_target(session_name)?;
        let workspaces = self.list_or_start_workspaces(&target.server_name, ssh_host)?;
        let Some(workspace_key) = target.workspace_key else {
            let is_reusable = !workspaces.workspaces.is_empty()
                && self.close_unrestorable_panes(&target.server_name, None, ssh_host)?;
            if !is_reusable {
                self.create_herdr_workspace(&target.server_name, cwd, layout, None, ssh_host)?;
            }
            return Ok(None);
        };
        let mapping = self.read_workspace_mapping_file(ssh_host)?;
        if let Some(workspace_id) = mapped_workspace_id(&mapping, &workspace_key)
            && workspaces.contains(&workspace_id)
            && self.close_unrestorable_panes(&target.server_name, Some(&workspace_id), ssh_host)?
        {
            return Ok(Some(workspace_id));
        }
        self.assign_shared_workspace(&target.server_name, &workspace_key, cwd, layout, ssh_host)
            .map(Some)
    }

    /// 서버가 꺼져 있으면 띄우고 응답할 때까지 기다린다. SSH 접속 실패는 서버 부재가 아니므로 그대로 돌려준다
    fn list_or_start_workspaces(
        &self,
        server_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<HerdrWorkspaceList, HerdrError> {
        let list =
            || self.query::<HerdrWorkspaceList>(server_name, &["workspace", "list"], ssh_host);
        match list() {
            Ok(workspaces) => return Ok(workspaces),
            Err(error) if error.is_ssh_connection_error() => return Err(error),
            Err(_) => {}
        }
        self.start_herdr_server(server_name, ssh_host)?;
        let deadline = self.now() + HERDR_START_TIMEOUT;
        loop {
            match list() {
                Ok(workspaces) => return Ok(workspaces),
                Err(error) if error.is_ssh_connection_error() || self.now() >= deadline => {
                    return Err(error);
                }
                Err(_) => std::thread::sleep(SERVER_START_POLL_INTERVAL),
            }
        }
    }

    fn start_herdr_server(
        &self,
        server_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HerdrError> {
        let command = herdr_command(server_name, &["server"])?;
        if let Some(ssh_host) = ssh_host {
            let launch = build_detached_remote_server_launch_command(&command);
            self.runner
                .exec_git(&launch, Some(ssh_host), &ExecGitOptions::default())?;
            return Ok(());
        }
        // nohup만 쓰면 앱의 프로세스 그룹에 남아 종료 신호를 함께 받으므로 새 세션으로 분리한다.
        let mut server = Command::new(&self.posix_shell_path);
        server
            .args(["-c", &format!("exec {command}")])
            .env_clear()
            .envs(&self.environment.shell_environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: fork 뒤 exec 전에 async-signal-safe인 setsid만 부른다
        unsafe {
            server.pre_exec(|| match libc::setsid() {
                -1 => Err(std::io::Error::last_os_error()),
                _ => Ok(()),
            });
        }
        let mut child = server
            .spawn()
            .map_err(|error| HerdrError::Message(error.to_string()))?;
        // 서버는 앱보다 오래 살 수 있다. 끝나면 좀비로 남지 않게 따로 거둔다.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    /// 파일 잠금 안에서 연결을 다시 확인하고, 없거나 닫힌 Space면 새로 만들어 연결한다.
    /// 연결 파일을 쓰지 못하면 방금 만든 Space를 닫는다
    fn assign_shared_workspace(
        &self,
        server_name: &str,
        workspace_key: &str,
        cwd: &str,
        layout: Option<&PaneLayoutSelection>,
        ssh_host: Option<&str>,
    ) -> Result<String, HerdrError> {
        let mapping_path = self.get_workspace_mapping_path(ssh_host)?;
        let mut workspace_id: Option<String> = None;
        let mut created_id: Option<String> = None;
        let updated = self.update_workspace_mapping(&mapping_path, ssh_host, |content| {
            let mut mapping = parse_workspace_mapping(content)?;
            let live: HerdrWorkspaceList =
                self.query(server_name, &["workspace", "list"], ssh_host)?;
            if let Some(existing) = mapped_workspace_id(&mapping, workspace_key)
                && live.contains(&existing)
            {
                workspace_id = Some(existing);
                return Ok(content.to_string());
            }
            let created = self.create_herdr_workspace(
                server_name,
                cwd,
                layout,
                Some(workspace_key),
                ssh_host,
            )?;
            created_id = Some(created.clone());
            workspace_id = Some(created.clone());
            mapping.insert(workspace_key.to_string(), created.into());
            Ok(serde_json::Value::Object(mapping).to_string())
        });
        if let Err(error) = updated {
            if let Some(created_id) = created_id {
                self.run_herdr_command(
                    server_name,
                    &["workspace", "close", &created_id],
                    ssh_host,
                )?;
            }
            return Err(error);
        }
        self.forget_workspace_mapping(ssh_host);
        Ok(workspace_id.unwrap_or_default())
    }

    /// 저장된 작업 경로가 사라져 Herdr가 복원하지 못한 pane을 닫는다.
    /// 이런 pane은 프로세스 없이 오류 문구만 보여 주고 Herdr에 다시 띄우는 명령도 없어서, 닫는 것 말고는 되살릴 방법이 없다.
    /// workspace_id가 있으면 공유 서버에서 그 task Space의 pane만 다루고, 없으면 task 전용 서버의 pane 전체를 다룬다.
    /// 반환값은 정상 pane이 하나라도 남았는지다. 모두 닫혔다면 Herdr가 workspace도 함께 닫으므로 호출자가 새로 만들어야 한다.
    fn close_unrestorable_panes(
        &self,
        server_name: &str,
        workspace_id: Option<&str>,
        ssh_host: Option<&str>,
    ) -> Result<bool, HerdrError> {
        let HerdrPaneList { panes } = self.query(server_name, &["pane", "list"], ssh_host)?;
        let workspace_prefix = workspace_id.map(|workspace_id| format!("{workspace_id}:"));
        let workspace_panes: Vec<&HerdrPane> = panes
            .iter()
            .filter(|pane| {
                workspace_prefix
                    .as_ref()
                    .is_none_or(|prefix| pane.pane_id.starts_with(prefix))
            })
            .collect();
        let unrestorable_panes: Vec<&HerdrPane> = workspace_panes
            .iter()
            .copied()
            .filter(|pane| pane.is_unrestorable())
            .collect();
        for pane in &unrestorable_panes {
            self.run_herdr_command(server_name, &["pane", "close", &pane.pane_id], ssh_host)?;
        }
        Ok(unrestorable_panes.len() < workspace_panes.len())
    }

    /// 새 workspace를 만들고 레이아웃을 적용한다. 레이아웃이 실패하면 공유 서버는 그 Space만, 독립 서버는 세션째 정리한다
    fn create_herdr_workspace(
        &self,
        server_name: &str,
        cwd: &str,
        layout: Option<&PaneLayoutSelection>,
        workspace_key: Option<&str>,
        ssh_host: Option<&str>,
    ) -> Result<String, HerdrError> {
        let mut arguments = vec!["workspace", "create", "--cwd", cwd];
        match workspace_key {
            Some(workspace_key) => arguments.extend(["--label", workspace_key, "--no-focus"]),
            None => arguments.push("--focus"),
        }
        let created: CreatedWorkspace = self.query(server_name, &arguments, ssh_host)?;
        let root_pane_id = created.root_pane.pane_id;
        let workspace_id = root_pane_id
            .split(':')
            .next()
            .unwrap_or_default()
            .to_string();
        let Err(error) = self.apply_herdr_layout(server_name, &root_pane_id, cwd, layout, ssh_host)
        else {
            return Ok(workspace_id);
        };
        if workspace_key.is_some() {
            self.run_herdr_command(
                server_name,
                &["workspace", "close", &workspace_id],
                ssh_host,
            )?;
        } else {
            let cleanup =
                build_herdr_session_cleanup_command(server_name).map_err(HerdrError::Message)?;
            let options = ExecGitOptions {
                timeout: Some(HERDR_START_TIMEOUT),
                ..ExecGitOptions::default()
            };
            self.runner.exec_git(&cleanup, ssh_host, &options)?;
        }
        Err(error)
    }

    fn split_herdr_pane(
        &self,
        server_name: &str,
        pane_id: &str,
        direction: &str,
        cwd: &str,
        ssh_host: Option<&str>,
    ) -> Result<String, HerdrError> {
        let arguments = [
            "pane",
            "split",
            pane_id,
            "--direction",
            direction,
            "--cwd",
            cwd,
        ];
        let split: SplitPane = self.query(server_name, &arguments, ssh_host)?;
        Ok(split.pane.pane_id)
    }

    /// 레이아웃 종류대로 pane을 나누고, 위치별 시작 명령을 보낸다
    fn apply_herdr_layout(
        &self,
        server_name: &str,
        root_pane_id: &str,
        cwd: &str,
        layout: Option<&PaneLayoutSelection>,
        ssh_host: Option<&str>,
    ) -> Result<(), HerdrError> {
        let Some(layout) = layout else {
            return Ok(());
        };
        let pane_ids =
            self.split_layout_panes(server_name, root_pane_id, cwd, layout.layout_type, ssh_host)?;
        for pane in &layout.panes {
            let target = usize::try_from(pane.position)
                .ok()
                .and_then(|position| pane_ids.get(position));
            if let Some(pane_id) = target.filter(|_| !js_trim(&pane.command).is_empty()) {
                // 초기 로그인 셸에는 bracketed-paste 대신 원문과 Enter를 한 번에 버퍼링한다.
                let text = format!("{}\r", pane.command);
                self.run_herdr_command(
                    server_name,
                    &["pane", "send-text", pane_id, &text],
                    ssh_host,
                )?;
            }
        }
        Ok(())
    }

    /// 레이아웃 위치 순서대로 pane ID를 돌려준다. 위치 0은 언제나 처음 pane이다
    fn split_layout_panes(
        &self,
        server_name: &str,
        root_pane_id: &str,
        cwd: &str,
        layout_type: PaneLayoutType,
        ssh_host: Option<&str>,
    ) -> Result<Vec<String>, HerdrError> {
        let split = |pane_id: &str, direction: &str| {
            self.split_herdr_pane(server_name, pane_id, direction, cwd, ssh_host)
        };
        let root = root_pane_id.to_string();
        let pane_ids = match layout_type {
            PaneLayoutType::Single => vec![root],
            PaneLayoutType::Horizontal2 => {
                let bottom = split(&root, "down")?;
                vec![root, bottom]
            }
            PaneLayoutType::Vertical2 => {
                let right = split(&root, "right")?;
                vec![root, right]
            }
            PaneLayoutType::LeftRightTb => {
                let right = split(&root, "right")?;
                let right_bottom = split(&right, "down")?;
                vec![root, right, right_bottom]
            }
            PaneLayoutType::LeftTbRight => {
                let right = split(&root, "right")?;
                let left_bottom = split(&root, "down")?;
                vec![root, left_bottom, right]
            }
            PaneLayoutType::Quad => {
                let right = split(&root, "right")?;
                let left_bottom = split(&root, "down")?;
                let right_bottom = split(&right, "down")?;
                vec![root, right, left_bottom, right_bottom]
            }
        };
        Ok(pane_ids)
    }
}
