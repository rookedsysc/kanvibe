//! 태스크 pane 목록(TS `getTaskSurfaces`)과 세션이 없을 때 띄우는 길.
//! 폰은 결과의 pane을 전부 펼쳐 탭으로 쓰고, 태블릿은 탭 하나의 pane을 좌표대로 조립한다.

use std::sync::Arc;

use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_process::worktree::is_session_alive;
use kanvibe_terminal::herdr::HerdrMirrorPaneOptions;
use kanvibe_terminal::pane_mirror::{
    MirrorPane, build_tmux_list_mirror_panes_command, build_zellij_list_mirror_panes_command,
    parse_tmux_mirror_pane_list, parse_zellij_mirror_pane_list,
};
use kanvibe_terminal::terminal_bridge::CallbackTerminalClient;
use kanvibe_terminal::terminal_registry::TerminalClient;
use serde::Serialize;

use crate::error::{MirrorResult, PaneMirrorError, SurfaceUnavailableReason, unavailable};
use crate::mirror::{MirrorSessionTarget, MirrorShared, PaneMirror, to_mirror_target};

/// 모바일이 보는 탭 하나. tmux window, zellij tab, Herdr tab, terminal 세션 탭 하나에 대응한다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MirrorTab {
    pub id: String,
    pub name: String,
    pub panes: Vec<MirrorPane>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSurfaces {
    pub task_id: String,
    pub session_type: SessionType,
    pub can_resize: bool,
    /// 화면 프레임만 받아 지난 출력이 스스로 쌓이지 않는 세션이다. 모바일이 올려 보기 시작할 때 갱신을 요청한다
    pub can_refresh_history: bool,
    /// 원격 Herdr처럼 직접 연결 권한을 발급할 수 있는 세션이다
    pub can_prepare_direct_connection: bool,
    pub tabs: Vec<MirrorTab>,
    /// 최소 응답에서만 있다. 이 탭 밖의 pane 좌표는 표시 전에 다시 읽어야 한다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometry_tab_id: Option<String>,
}

impl TaskSurfaces {
    pub(crate) fn panes(&self) -> impl Iterator<Item = &MirrorPane> {
        self.tabs.iter().flat_map(|tab| tab.panes.iter())
    }

    pub(crate) fn has_pane(&self, pane_id: &str) -> bool {
        self.panes().any(|pane| pane.id == pane_id)
    }
}

/// TS `getTaskSurfaces(taskId, { metadata: "minimal", tabId })`. 최소 응답은 Herdr에서만 좌표 조회를 줄인다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SurfaceQuery {
    pub is_minimal: bool,
    pub tab_id: Option<String>,
}

/// pane 목록 조회 범위. 선택한 pane이 있으면 Herdr는 그 pane의 탭 좌표만 읽는다
#[derive(Debug, Clone, Default)]
pub(crate) struct PaneListScope {
    pub is_minimal: bool,
    pub tab_id: Option<String>,
    pub selected_pane_id: Option<String>,
}

impl PaneListScope {
    pub fn selected(pane_id: &str) -> Self {
        Self {
            is_minimal: true,
            tab_id: None,
            selected_pane_id: Some(pane_id.to_string()),
        }
    }
}

impl PaneMirror {
    /// 태스크의 pane을 탭 단위로 묶어 돌려준다. 세션이 없으면 데스크탑이 태스크를 열 때와 같은 길로 띄운다
    pub fn get_task_surfaces(
        &self,
        task_id: &str,
        query: &SurfaceQuery,
    ) -> MirrorResult<TaskSurfaces> {
        self.shared.get_task_surfaces(task_id, query)
    }
}

impl MirrorShared {
    pub fn get_task_surfaces(
        &self,
        task_id: &str,
        query: &SurfaceQuery,
    ) -> MirrorResult<TaskSurfaces> {
        let task = self.find_mirror_task(task_id, true)?;
        let target = to_mirror_target(&task);
        if let Some(ssh_host) = target.ssh_host() {
            self.ensure_remote_host_reachable(ssh_host)?;
        }
        if target.session_type == SessionType::Terminal {
            self.refresh_remote_terminal_tabs(&task)?;
            return Ok(TaskSurfaces {
                task_id: task_id.to_string(),
                session_type: SessionType::Terminal,
                can_resize: true,
                can_refresh_history: false,
                can_prepare_direct_connection: false,
                tabs: self.list_terminal_mirror_tabs(task_id),
                geometry_tab_id: None,
            });
        }
        self.start_multiplexer_session_if_missing(&task, &target)?;
        let scope = PaneListScope {
            is_minimal: query.is_minimal,
            tab_id: query.tab_id.clone(),
            selected_pane_id: None,
        };
        let panes = self.list_mirror_panes(&target, &scope)?;
        if panes.is_empty() {
            return Err(unavailable(SurfaceUnavailableReason::SessionNotRunning));
        }
        Ok(build_multiplexer_surfaces(task_id, &target, query, panes))
    }

    /// 원격 terminal 세션은 앱 메모리에서 사라진 탭을 daemon 목록으로 되살린 뒤 읽는다
    pub fn refresh_remote_terminal_tabs(&self, task: &KanbanTask) -> MirrorResult<()> {
        let ssh_host = task.ssh_host.as_deref().filter(|host| !host.is_empty());
        let session_name = task.session_name.as_deref().filter(|name| !name.is_empty());
        if let (Some(ssh_host), Some(session_name)) = (ssh_host, session_name) {
            self.terminals
                .list_remote_terminal_tabs(&task.id, ssh_host, session_name)?;
        }
        Ok(())
    }

    pub fn list_mirror_panes(
        &self,
        target: &MirrorSessionTarget,
        scope: &PaneListScope,
    ) -> MirrorResult<Vec<MirrorPane>> {
        let ssh_host = target.ssh_host();
        match target.session_type {
            SessionType::Herdr => {
                let options = HerdrMirrorPaneOptions {
                    is_minimal: scope.is_minimal,
                    tab_id: scope.tab_id.clone(),
                    selected_pane_id: scope.selected_pane_id.clone(),
                };
                Ok(self
                    .herdr
                    .list_mirror_panes(&target.session_name, ssh_host, &options)?)
            }
            SessionType::Zellij => {
                if !self.has_zellij_pane_id_support(ssh_host) {
                    return Err(unavailable(SurfaceUnavailableReason::ZellijTooOld));
                }
                let command = build_zellij_list_mirror_panes_command(&target.session_name);
                Ok(parse_zellij_mirror_pane_list(
                    &self.run_command(&command, ssh_host)?,
                ))
            }
            _ => {
                let command = build_tmux_list_mirror_panes_command(&target.session_name);
                Ok(parse_tmux_mirror_pane_list(
                    &self.run_command(&command, ssh_host)?,
                ))
            }
        }
    }

    /// terminal 세션의 탭 목록. 멀티플렉서가 없어 탭 하나가 PTY 하나, 즉 pane 하나이고 pane 식별자로 탭 식별자를 쓴다
    pub fn list_terminal_mirror_tabs(&self, task_id: &str) -> Vec<MirrorTab> {
        self.terminals
            .list_local_terminal_tabs(task_id)
            .into_iter()
            .map(|tab| {
                let (columns, rows) = self.terminals.terminal_tab_size(task_id, &tab.id);
                let pane = MirrorPane {
                    id: tab.id.clone(),
                    tab_id: tab.id.clone(),
                    tab_name: tab.name.clone(),
                    command: String::new(),
                    left: 0,
                    top: 0,
                    width: i64::from(columns),
                    height: i64::from(rows),
                };
                MirrorTab {
                    id: tab.id,
                    name: tab.name,
                    panes: vec![pane],
                }
            })
            .collect()
    }

    /// 원격 호스트에 닿는지 먼저 본다. 세션 확인은 연결 실패도 "세션 없음"으로 접어, 그대로 두면 화면이 세션이 꺼졌다고 잘못 안내한다
    fn ensure_remote_host_reachable(&self, ssh_host: &str) -> MirrorResult<()> {
        self.run_command("true", Some(ssh_host))
            .map(|_| ())
            .map_err(|error| {
                eprintln!("[kanvibe] Mobile remote host unreachable: {error}");
                unavailable(SurfaceUnavailableReason::RemoteUnreachable)
            })
    }

    pub fn is_session_alive(&self, target: &MirrorSessionTarget) -> bool {
        let herdr = HerdrAliveCheck(self.herdr.as_ref());
        is_session_alive(
            self.runner.as_ref(),
            &herdr,
            target.session_type,
            &target.session_name,
            target.ssh_host(),
        )
    }

    /// 세션이 아직 없으면 화면 없는 클라이언트로 붙어 세션을 만들고, 살아난 것을 확인한 뒤 떨어진다.
    /// 멀티플렉서 세션은 클라이언트가 떨어져도 남고 데스크탑 화면은 붙어 있지 않았으니 바뀌지 않는다
    fn start_multiplexer_session_if_missing(
        &self,
        task: &KanbanTask,
        target: &MirrorSessionTarget,
    ) -> MirrorResult<()> {
        if self.is_session_alive(target) {
            return Ok(());
        }
        // 설치에는 사용자 동의가 필요해 모바일이 대신 설치하지 않고, 세션 문제와 구분해 안내한다
        if target.session_type == SessionType::Herdr && !self.is_herdr_available(target.ssh_host())
        {
            return Err(unavailable(SurfaceUnavailableReason::HerdrMissing));
        }
        let startup_client = Arc::new(CallbackTerminalClient::new(|_| {}));
        let started = self.attach_startup_client(task, &startup_client, target);
        startup_client.close(None);
        started.map_err(|error| {
            eprintln!("[kanvibe] Mobile session start failed: {error}");
            unavailable(SurfaceUnavailableReason::SessionNotRunning)
        })
    }

    fn attach_startup_client(
        &self,
        task: &KanbanTask,
        startup_client: &Arc<CallbackTerminalClient>,
        target: &MirrorSessionTarget,
    ) -> MirrorResult<()> {
        let client: Arc<dyn TerminalClient> = startup_client.clone();
        let attachment = self
            .attacher
            .attach_task_terminal(task, None, client, None, None)?;
        let is_alive = self.poll_until(|| Ok(self.is_session_alive(target)));
        if let Some(attachment) = attachment {
            self.terminals.client_closed(&attachment);
        }
        match is_alive? {
            true => Ok(()),
            false => Err(PaneMirrorError::Failed(format!(
                "세션이 {}ms 안에 살아나지 않았습니다: {}",
                self.timings.session_start_timeout.as_millis(),
                target.session_name
            ))),
        }
    }

    fn is_herdr_available(&self, ssh_host: Option<&str>) -> bool {
        self.session_dependencies
            .status(SessionType::Herdr, ssh_host)
            .is_ok_and(|status| status.is_available)
    }
}

fn build_multiplexer_surfaces(
    task_id: &str,
    target: &MirrorSessionTarget,
    query: &SurfaceQuery,
    panes: Vec<MirrorPane>,
) -> TaskSurfaces {
    let is_herdr = target.session_type == SessionType::Herdr;
    let geometry_tab_id = (query.is_minimal && is_herdr)
        .then(|| {
            query.tab_id.clone().or_else(|| {
                panes
                    .iter()
                    .find(|pane| pane.width > 0)
                    .map(|pane| pane.tab_id.clone())
            })
        })
        .flatten();
    TaskSurfaces {
        task_id: task_id.to_string(),
        session_type: target.session_type,
        can_resize: true,
        can_refresh_history: is_herdr,
        can_prepare_direct_connection: is_herdr && target.ssh_host().is_some(),
        tabs: group_panes_into_tabs(panes),
        geometry_tab_id,
    }
}

/// pane 목록을 처음 나온 탭 순서대로 묶는다. 멀티플렉서가 이미 정렬해 주므로 다시 정렬하지 않는다
fn group_panes_into_tabs(panes: Vec<MirrorPane>) -> Vec<MirrorTab> {
    let mut tabs: Vec<MirrorTab> = Vec::new();
    for pane in panes {
        match tabs.iter_mut().find(|tab| tab.id == pane.tab_id) {
            Some(tab) => tab.panes.push(pane),
            None => tabs.push(MirrorTab {
                id: pane.tab_id.clone(),
                name: pane.tab_name.clone(),
                panes: vec![pane],
            }),
        }
    }
    tabs
}

/// 세션 확인 함수가 받는 Herdr 경계. 미러는 살아 있는지만 묻는다
struct HerdrAliveCheck<'a>(&'a dyn crate::ports::HerdrMirrorQueries);

impl kanvibe_process::worktree::HerdrTaskWorkspaces for HerdrAliveCheck<'_> {
    fn close_task_workspace(
        &self,
        _session_name: &str,
        _ssh_host: Option<&str>,
    ) -> Result<(), String> {
        Err("미러는 Herdr Space를 닫지 않습니다.".to_string())
    }

    fn is_task_alive(&self, session_name: &str, ssh_host: Option<&str>) -> bool {
        self.0.is_task_alive(session_name, ssh_host)
    }
}
