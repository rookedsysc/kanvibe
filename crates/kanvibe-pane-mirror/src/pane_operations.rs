//! 모바일 탭 바의 조작: 새 탭, pane 닫기와 닫기 전 확인, 세션 제어 단축키.
//! 모바일 탭 하나는 pane 하나다. 같은 창의 다른 pane은 그대로 남긴다.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use kanvibe_domain::task::SessionType;
use kanvibe_process::worktree::quote_for_posix_shell;
use kanvibe_terminal::pane_mirror::{
    MirrorPane, build_herdr_close_pane_command, build_herdr_pane_shortcut_command,
    build_screen_redraw, build_tmux_capture_pane_command, build_tmux_close_pane_command,
    build_tmux_pane_shortcut_command, build_zellij_close_pane_command,
    build_zellij_pane_shortcut_command,
};
use serde::Serialize;

use crate::error::{MirrorResult, PaneMirrorError, session_not_running};
use crate::mirror::{MirrorSessionTarget, MirrorShared, PaneMirror, lock, to_mirror_target};
use crate::surfaces::{PaneListScope, SurfaceQuery, TaskSurfaces};

/// 현재 명령이 이 이름 중 하나면 입력을 기다리는 셸일 뿐이라 닫아도 잃는 작업이 없다
const IDLE_SHELL_COMMANDS: [&str; 5] = ["sh", "bash", "zsh", "fish", "dash"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedTerminalTab {
    pub surfaces: TaskSurfaces,
    pub pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClosedPane {
    pub surfaces: TaskSurfaces,
}

/// 닫으면 잃을 수 있는 실행 중인 프로세스 이름. 빈 목록이면 확인 없이 닫아도 된다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneCloseInfo {
    pub process_names: Vec<String>,
}

/// 단축키를 실행한 뒤의 pane 목록. tmux 복사 모드처럼 출력 흐름에 나오지 않는 화면은 `screen`으로 함께 보낸다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShortcutOutcome {
    pub surfaces: TaskSurfaces,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
}

impl PaneMirror {
    /// 탭 바의 새 탭 만들기와 같은 길로 탭을 만들고, 새 탭의 첫 pane이 목록에 나타날 때까지 기다린다
    pub fn create_mobile_terminal_tab(&self, task_id: &str) -> MirrorResult<CreatedTerminalTab> {
        let shared = &self.shared;
        let previous = shared.get_task_surfaces(task_id, &SurfaceQuery::default())?;
        let previous_tab_ids: HashSet<String> =
            previous.tabs.iter().map(|tab| tab.id.clone()).collect();
        shared
            .tabs
            .create_terminal_tab(task_id)
            .map_err(PaneMirrorError::Failed)?;
        let mut created = None;
        let is_found = shared.poll_until(|| {
            let surfaces = shared.get_task_surfaces(task_id, &SurfaceQuery::default())?;
            let new_pane_id = surfaces
                .tabs
                .iter()
                .find(|tab| !previous_tab_ids.contains(&tab.id) && !tab.panes.is_empty())
                .map(|tab| tab.panes[0].id.clone());
            created = new_pane_id.map(|pane_id| CreatedTerminalTab { surfaces, pane_id });
            Ok(created.is_some())
        })?;
        match (is_found, created) {
            (true, Some(created)) => Ok(created),
            _ => Err(PaneMirrorError::Failed(
                "추가한 터미널 탭을 찾을 수 없습니다.".to_string(),
            )),
        }
    }

    /// pane 하나를 닫는다. 태스크별로 한 줄로 세워, 두 기기가 마지막 두 pane을 동시에 닫아도 하나는 남긴다
    pub fn close_mobile_pane(&self, task_id: &str, pane_id: &str) -> MirrorResult<ClosedPane> {
        let queue = Arc::clone(
            lock(&self.shared.pane_close_queues)
                .entry(task_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        );
        let result = {
            let _turn = lock(&queue);
            self.shared.close_session_pane(task_id, pane_id)
        };
        let mut queues = lock(&self.shared.pane_close_queues);
        if queues
            .get(task_id)
            .is_some_and(|current| Arc::ptr_eq(current, &queue) && Arc::strong_count(&queue) == 2)
        {
            queues.remove(task_id);
        }
        result
    }

    /// 조회에 실패하면 없다고 단정하지 않고 오류를 올려, 부른 쪽이 확인을 거치게 한다
    pub fn get_mobile_pane_close_info(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> MirrorResult<PaneCloseInfo> {
        let surfaces = self
            .shared
            .get_task_surfaces(task_id, &SurfaceQuery::default())?;
        let pane = surfaces
            .panes()
            .find(|pane| pane.id == pane_id)
            .ok_or(PaneMirrorError::PaneNotFound)?;
        let current_command = match surfaces.session_type {
            SessionType::Terminal => self
                .shared
                .terminals
                .local_terminal_tab_process(task_id, &pane.tab_id)
                .unwrap_or_default(),
            _ => pane.command.clone(),
        };
        let is_running_program =
            !current_command.is_empty() && !IDLE_SHELL_COMMANDS.contains(&current_command.as_str());
        Ok(PaneCloseInfo {
            process_names: is_running_program
                .then_some(current_command)
                .into_iter()
                .collect(),
        })
    }

    pub fn execute_mobile_terminal_shortcut(
        &self,
        task_id: &str,
        pane_id: &str,
        action: &str,
    ) -> MirrorResult<ShortcutOutcome> {
        let is_known_action = build_tmux_pane_shortcut_command(pane_id, action).is_some()
            || build_zellij_pane_shortcut_command("", pane_id, "", action).is_some();
        if !is_known_action {
            return Err(input_error("지원하지 않는 단축키입니다"));
        }
        let shared = &self.shared;
        let target = shared.find_mirror_target(task_id)?;
        if target.session_type == SessionType::Terminal {
            return Err(input_error(
                "일반 터미널에서는 세션 제어 단축키를 사용할 수 없습니다",
            ));
        }
        let panes = shared.list_mirror_panes(&target, &PaneListScope::default())?;
        let pane = panes
            .iter()
            .find(|candidate| candidate.id == pane_id)
            .ok_or_else(session_not_running)?;
        let command = build_shortcut_command(&target, pane, action)?
            .ok_or_else(|| input_error("이 세션에서 지원하지 않는 단축키입니다"))?;
        shared.run_command(&command, target.ssh_host())?;
        let screen = shared.read_shortcut_screen(&target, pane_id, action)?;
        let surfaces = shared.wait_for_shortcut_surfaces(task_id, pane, &panes, action)?;
        Ok(ShortcutOutcome { surfaces, screen })
    }
}

fn input_error(message: &str) -> PaneMirrorError {
    PaneMirrorError::MobileTaskInput(message.to_string())
}

fn build_shortcut_command(
    target: &MirrorSessionTarget,
    pane: &MirrorPane,
    action: &str,
) -> MirrorResult<Option<String>> {
    Ok(match target.session_type {
        SessionType::Herdr => {
            build_herdr_pane_shortcut_command(&target.session_name, &pane.id, action)?
        }
        SessionType::Zellij => {
            build_zellij_pane_shortcut_command(&target.session_name, &pane.id, &pane.tab_id, action)
        }
        _ => build_tmux_pane_shortcut_command(&pane.id, action),
    })
}

impl MirrorShared {
    fn close_session_pane(&self, task_id: &str, pane_id: &str) -> MirrorResult<ClosedPane> {
        let surfaces = self.get_task_surfaces(task_id, &SurfaceQuery::default())?;
        let pane_count = surfaces.panes().count();
        let pane = surfaces
            .panes()
            .find(|pane| pane.id == pane_id)
            .ok_or(PaneMirrorError::PaneNotFound)?;
        if pane_count == 1 {
            return Err(PaneMirrorError::LastPane);
        }
        let task = self.find_mirror_task(task_id, false)?;
        let target = to_mirror_target(&task);
        match (target.session_type, target.ssh_host()) {
            (SessionType::Terminal, Some(ssh_host)) => {
                self.terminals.close_remote_terminal_tab(
                    task_id,
                    &pane.tab_id,
                    ssh_host,
                    &target.session_name,
                )?;
            }
            (SessionType::Terminal, None) => {
                self.terminals
                    .close_local_terminal_tab(task_id, &pane.tab_id);
            }
            _ => {
                let command = build_close_pane_command(&target, pane_id)?;
                self.run_command(&command, target.ssh_host())?;
            }
        }
        Ok(ClosedPane {
            surfaces: self.wait_until_pane_disappears(task_id, pane_id)?,
        })
    }

    fn wait_until_pane_disappears(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> MirrorResult<TaskSurfaces> {
        let mut latest = None;
        let has_disappeared = self.poll_until(|| {
            let surfaces = self.get_task_surfaces(task_id, &SurfaceQuery::default())?;
            let has_disappeared = !surfaces.has_pane(pane_id);
            latest = Some(surfaces);
            Ok(has_disappeared)
        })?;
        match (has_disappeared, latest) {
            (true, Some(surfaces)) => Ok(surfaces),
            _ => Err(PaneMirrorError::Failed(
                "탭이 닫히지 않았습니다.".to_string(),
            )),
        }
    }

    /// pipe-pane은 프로그램 출력만 전하므로 복사 모드의 화면은 따로 떠서 모바일에 돌려준다
    fn read_shortcut_screen(
        &self,
        target: &MirrorSessionTarget,
        pane_id: &str,
        action: &str,
    ) -> MirrorResult<Option<String>> {
        let is_copy_mode_action = action == "copy-mode" || action.starts_with("scroll-");
        if target.session_type != SessionType::Tmux || !is_copy_mode_action {
            return Ok(None);
        }
        let capture = match action {
            "scroll-bottom" => build_tmux_capture_pane_command(pane_id),
            _ => format!(
                "tmux capture-pane -peM -t {}",
                quote_for_posix_shell(pane_id)
            ),
        };
        let screen = self.run_command(&capture, target.ssh_host())?;
        Ok(Some(build_screen_redraw(&screen, None)))
    }

    /// 분할은 새 pane이 목록에 늦게 나타날 수 있어, 명령을 다시 실행하지 않고 나타날 때까지 기다린다
    fn wait_for_shortcut_surfaces(
        &self,
        task_id: &str,
        pane: &MirrorPane,
        previous_panes: &[MirrorPane],
        action: &str,
    ) -> MirrorResult<TaskSurfaces> {
        let mut surfaces = self.get_task_surfaces(task_id, &SurfaceQuery::default())?;
        if action != "split-right" && action != "split-down" {
            return Ok(surfaces);
        }
        let previous_pane_ids: HashSet<&str> = previous_panes
            .iter()
            .map(|previous| previous.id.as_str())
            .collect();
        let has_new_pane = |surfaces: &TaskSurfaces| {
            surfaces.tabs.iter().any(|tab| {
                tab.id == pane.tab_id
                    && tab
                        .panes
                        .iter()
                        .any(|created| !previous_pane_ids.contains(created.id.as_str()))
            })
        };
        let is_found = self.poll_until(|| {
            if has_new_pane(&surfaces) {
                return Ok(true);
            }
            surfaces = self.get_task_surfaces(task_id, &SurfaceQuery::default())?;
            Ok(has_new_pane(&surfaces))
        })?;
        match is_found {
            true => Ok(surfaces),
            false => Err(PaneMirrorError::Failed(
                "추가한 pane을 찾을 수 없습니다.".to_string(),
            )),
        }
    }
}

fn build_close_pane_command(target: &MirrorSessionTarget, pane_id: &str) -> MirrorResult<String> {
    Ok(match target.session_type {
        SessionType::Herdr => build_herdr_close_pane_command(&target.session_name, pane_id)?,
        SessionType::Zellij => build_zellij_close_pane_command(&target.session_name, pane_id),
        _ => build_tmux_close_pane_command(pane_id),
    })
}
