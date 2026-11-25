//! KanVibe 탭 바가 부르는 진입점(TS `src/desktop/main/services/terminalTabService.ts`).
//! tmux와 zellij는 멀티플렉서 CLI를 실행해 탭을 읽고 바꾸며, Herdr는 Herdr 서비스에 맡기고,
//! terminal 세션은 터미널 레지스트리의 탭 목록에 그대로 위임한다.
//!
//! TS 모듈 전역이던 탭 닫기 대기열은 앱이 한 번 만드는 [`TerminalTabService`]의 필드다.

mod adapters;
mod close_queue;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use kanvibe_ai::javascript::js_trim;
use kanvibe_db::repositories::tasks::find_task;
use kanvibe_db::{Database, DatabaseError};
use kanvibe_domain::task::SessionType;
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use serde_json::Value;

use crate::herdr::{HerdrError, parse_herdr_session_target};
use crate::pane_mirror::{
    MirrorPane, build_tmux_list_mirror_panes_command, parse_tmux_mirror_pane_list,
};
use crate::terminal_tabs::{
    ZellijIdTargetingSupportCache, build_tmux_kill_window_command, build_tmux_list_windows_command,
    build_tmux_move_window_commands, build_tmux_new_window_command,
    build_tmux_rename_window_command, build_tmux_select_window_command,
    build_zellij_close_focused_tab_commands, build_zellij_close_tab_by_id_command,
    build_zellij_dump_layout_command, build_zellij_go_to_tab_by_id_command,
    build_zellij_go_to_tab_by_name_command, build_zellij_list_tabs_command,
    build_zellij_move_tab_by_id_commands, build_zellij_move_tab_commands,
    build_zellij_new_tab_command, build_zellij_query_tab_names_command,
    build_zellij_rename_focused_tab_commands, build_zellij_rename_tab_by_id_command,
    parse_tmux_window_list, parse_zellij_tab_list, parse_zellij_tab_names_with_focus,
};

use close_queue::{TabCloseQueues, TabCloseSessionKey};

/// 탭 명령은 짧게 끝나야 폴링이 밀리지 않는다
const TAB_COMMAND_TIMEOUT: Duration = Duration::from_millis(5_000);

const NO_SESSION_MESSAGE: &str = "작업에 연결된 세션이 없습니다.";
const TAB_NOT_FOUND_MESSAGE: &str = "탭을 찾을 수 없습니다.";

/// 탭 목록 조회 결과(TS `TerminalTabListResult`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTabListResult {
    pub is_ok: bool,
    pub tabs: Vec<TerminalTab>,
    pub error: Option<String>,
}

/// 탭 생성·선택·닫기·이름 변경·이동 결과(TS `TerminalTabMutationResult`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTabMutationResult {
    pub is_ok: bool,
    /// 탭을 닫은 뒤 KanVibe 창까지 닫아야 하는지. 닫기 결과에만 있다.
    /// 남은 탭 수와 같지 않다. 멀티플렉서의 마지막 탭은 닫지 않고 창만 닫으므로 탭이 하나 남은 채로도 참이다
    pub should_close_window: Option<bool>,
    pub error: Option<String>,
}

impl TerminalTabMutationResult {
    fn success() -> Self {
        Self {
            is_ok: true,
            should_close_window: None,
            error: None,
        }
    }

    fn closed(should_close_window: bool) -> Self {
        Self {
            is_ok: true,
            should_close_window: Some(should_close_window),
            error: None,
        }
    }

    fn failure(message: impl Into<String>) -> Self {
        Self {
            is_ok: false,
            should_close_window: None,
            error: Some(message.into()),
        }
    }
}

/// 탭을 닫기 전에 사용자에게 확인받을 정보(TS `TerminalTabClosePreview`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTabClosePreview {
    pub is_ok: bool,
    pub should_close_window: bool,
    /// 지금 관측된 프로세스 이름. `None`은 조회 불가이며 작업이 없다는 뜻이 아니다
    pub process_names: Option<Vec<String>>,
    pub error: Option<String>,
}

impl TerminalTabClosePreview {
    fn failure(message: impl Into<String>) -> Self {
        Self {
            is_ok: false,
            should_close_window: false,
            process_names: None,
            error: Some(message.into()),
        }
    }
}

/// TS에서 try 밖으로 던져져 호출자에게 그대로 거절되던 실패
#[derive(Debug, thiserror::Error)]
pub enum TerminalTabServiceError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    /// 원격 terminal 세션의 탭 목록을 읽지 못했다
    #[error("{0}")]
    RemoteTerminalTabs(String),
}

/// 탭 서비스가 쓰는 터미널 레지스트리 기능. [`crate::terminal_registry::TerminalRegistry`]가 구현한다
pub trait TerminalTabRegistry: Send + Sync {
    fn list_local_terminal_tabs(&self, task_id: &str) -> Vec<TerminalTab>;
    fn list_remote_terminal_tabs(
        &self,
        task_id: &str,
        ssh_host: &str,
        session_name: &str,
    ) -> Result<Vec<TerminalTab>, String>;
    /// 남은 탭 수를 돌려준다
    fn close_remote_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        ssh_host: &str,
        session_name: &str,
    ) -> Result<usize, String>;
    /// 남은 탭 수를 돌려준다
    fn close_local_terminal_tab(&self, task_id: &str, tab_id: &str) -> usize;
    fn local_terminal_tab_process(&self, task_id: &str, tab_id: &str) -> Option<String>;
    fn create_local_terminal_tab(&self, task_id: &str);
    fn select_local_terminal_tab(&self, task_id: &str, tab_id: &str);
    fn rename_local_terminal_tab(&self, task_id: &str, tab_id: &str, name: &str);
    fn move_local_terminal_tab(&self, task_id: &str, tab_id: &str, target_index: usize);
    fn herdr_task_active_tab_id(&self, task_id: &str) -> Option<String>;
    fn focus_herdr_task_tab(&self, task_id: &str, tab_id: &str) -> Result<(), String>;
}

/// 탭 서비스가 쓰는 Herdr 기능. [`crate::herdr::HerdrService`]가 구현한다
pub trait TerminalTabHerdr: Send + Sync {
    fn read_tabs(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<TerminalTab>, HerdrError>;
    /// 결과 JSON의 `result`를 돌려준다
    fn run_command(
        &self,
        session_name: &str,
        arguments: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<Value, HerdrError>;
    fn move_tab(
        &self,
        session_name: &str,
        tab_id: &str,
        insert_index: i64,
        ssh_host: Option<&str>,
    ) -> Result<(), HerdrError>;
    /// 모든 탭의 pane을 실행 명령과 함께 읽는다
    fn list_mirror_panes(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<MirrorPane>, HerdrError>;
}

/// 탭 서비스가 쓰는 협력자. 앱이 시작할 때 한 번 만든다
pub struct TerminalTabServiceDependencies {
    pub database: Arc<Database>,
    pub runner: Arc<dyn ShellCommandRunner>,
    pub registry: Arc<dyn TerminalTabRegistry>,
    pub herdr: Arc<dyn TerminalTabHerdr>,
    /// pane 축 서비스와 함께 쓰는 zellij 버전 판정 캐시
    pub zellij_id_targeting_support: Arc<ZellijIdTargetingSupportCache>,
}

/// 작업에 연결된 세션 좌표
#[derive(Debug, Clone)]
struct TerminalSessionTarget {
    session_type: SessionType,
    session_name: String,
    ssh_host: Option<String>,
    worktree_path: Option<String>,
}

impl TerminalSessionTarget {
    fn ssh_host(&self) -> Option<&str> {
        self.ssh_host.as_deref()
    }

    fn close_queue_key(&self) -> TabCloseSessionKey {
        (
            self.ssh_host.clone(),
            self.session_type,
            self.session_name.clone(),
        )
    }
}

/// 멀티플렉서 명령 실패. 사용자에게는 원래 오류 메시지만 보여 준다(TS `toFailure`)
#[derive(Debug)]
struct TabCommandFailure(String);

impl From<ExecGitError> for TabCommandFailure {
    fn from(error: ExecGitError) -> Self {
        Self(error.message)
    }
}

impl From<HerdrError> for TabCommandFailure {
    fn from(error: HerdrError) -> Self {
        Self(error.to_string())
    }
}

impl From<String> for TabCommandFailure {
    fn from(message: String) -> Self {
        Self(message)
    }
}

type TabCommandResult<T> = Result<T, TabCommandFailure>;

fn into_mutation_result(
    result: TabCommandResult<TerminalTabMutationResult>,
) -> TerminalTabMutationResult {
    result.unwrap_or_else(|TabCommandFailure(message)| TerminalTabMutationResult::failure(message))
}

/// Herdr 탭 ID `w1:t2`의 워크스페이스 부분
fn herdr_workspace_id(tab_id: &str) -> &str {
    tab_id.split(':').next().unwrap_or_default()
}

fn find_tab_name(tabs: &[TerminalTab], tab_id: &str) -> Option<String> {
    tabs.iter()
        .find(|tab| tab.id == tab_id)
        .map(|tab| tab.name.clone())
}

/// 같은 탭의 실행 명령을 처음 나온 순서대로 중복 없이 모은다. 하나도 없으면 조회 불가와 같이 `None`이다
fn collect_tab_process_names(panes: &[MirrorPane], tab_id: &str) -> Option<Vec<String>> {
    let mut seen_names = HashSet::new();
    let process_names: Vec<String> = panes
        .iter()
        .filter(|pane| pane.tab_id == tab_id)
        .map(|pane| js_trim(&pane.command))
        .filter(|name| !name.is_empty() && seen_names.insert(*name))
        .map(str::to_string)
        .collect();
    (!process_names.is_empty()).then_some(process_names)
}

pub struct TerminalTabService {
    database: Arc<Database>,
    runner: Arc<dyn ShellCommandRunner>,
    registry: Arc<dyn TerminalTabRegistry>,
    herdr: Arc<dyn TerminalTabHerdr>,
    zellij_id_targeting_support: Arc<ZellijIdTargetingSupportCache>,
    /// 마지막 탭 보호를 위해 같은 호스트·세션의 조회와 종료를 한 작업으로 처리한다
    tab_close_queues: TabCloseQueues,
}

impl TerminalTabService {
    pub fn new(dependencies: TerminalTabServiceDependencies) -> Self {
        Self {
            database: dependencies.database,
            runner: dependencies.runner,
            registry: dependencies.registry,
            herdr: dependencies.herdr,
            zellij_id_targeting_support: dependencies.zellij_id_targeting_support,
            tab_close_queues: TabCloseQueues::default(),
        }
    }

    pub fn list_terminal_tabs(
        &self,
        task_id: &str,
    ) -> Result<TerminalTabListResult, TerminalTabServiceError> {
        let Some(target) = self.find_session_target(task_id)? else {
            return Ok(TerminalTabListResult {
                is_ok: false,
                tabs: Vec::new(),
                error: Some(NO_SESSION_MESSAGE.to_string()),
            });
        };
        let tabs = match target.session_type {
            SessionType::Terminal => self
                .read_terminal_session_tabs(task_id, &target)
                .map_err(TabCommandFailure),
            _ => self.read_multiplexer_tabs_for_display(task_id, &target),
        };
        let result = match tabs {
            Ok(tabs) => TerminalTabListResult {
                is_ok: true,
                tabs,
                error: None,
            },
            Err(TabCommandFailure(message)) => TerminalTabListResult {
                is_ok: false,
                tabs: Vec::new(),
                error: Some(message),
            },
        };
        Ok(result)
    }

    pub fn create_terminal_tab(
        &self,
        task_id: &str,
    ) -> Result<TerminalTabMutationResult, TerminalTabServiceError> {
        let Some(target) = self.find_session_target(task_id)? else {
            return Ok(TerminalTabMutationResult::failure(NO_SESSION_MESSAGE));
        };
        if target.session_type == SessionType::Terminal {
            self.refresh_remote_terminal_tabs(task_id, &target)?;
            self.registry.create_local_terminal_tab(task_id);
            return Ok(TerminalTabMutationResult::success());
        }

        let created = match target.session_type {
            SessionType::Herdr => self.create_herdr_tab(task_id, &target),
            SessionType::Tmux => self
                .run_tab_command(
                    &build_tmux_new_window_command(
                        &target.session_name,
                        target.worktree_path.as_deref(),
                    ),
                    target.ssh_host(),
                )
                .map(|_| ())
                .map_err(TabCommandFailure::from),
            _ => self
                .run_tab_command(
                    &build_zellij_new_tab_command(
                        &target.session_name,
                        target.worktree_path.as_deref(),
                    ),
                    target.ssh_host(),
                )
                .map(|_| ())
                .map_err(TabCommandFailure::from),
        };
        Ok(into_mutation_result(
            created.map(|()| TerminalTabMutationResult::success()),
        ))
    }

    /// 조회 중 어떤 실패도 호출자에게 던지지 않고 실패 결과로 돌려준다
    pub fn preview_terminal_tab_close(
        &self,
        task_id: &str,
        tab_id: &str,
    ) -> TerminalTabClosePreview {
        let target = match self.find_session_target(task_id) {
            Ok(Some(target)) => target,
            Ok(None) => return TerminalTabClosePreview::failure(NO_SESSION_MESSAGE),
            Err(error) => return TerminalTabClosePreview::failure(error.to_string()),
        };
        let tabs = match target.session_type {
            SessionType::Terminal => self
                .read_terminal_session_tabs(task_id, &target)
                .map_err(TabCommandFailure),
            _ => self.read_tabs(&target),
        };
        let tabs = match tabs {
            Ok(tabs) => tabs,
            Err(TabCommandFailure(message)) => return TerminalTabClosePreview::failure(message),
        };
        if !tabs.iter().any(|tab| tab.id == tab_id) {
            return TerminalTabClosePreview::failure(TAB_NOT_FOUND_MESSAGE);
        }
        let should_close_window = target.session_type != SessionType::Terminal && tabs.len() == 1;
        if should_close_window {
            return TerminalTabClosePreview {
                is_ok: true,
                should_close_window: true,
                process_names: None,
                error: None,
            };
        }
        TerminalTabClosePreview {
            is_ok: true,
            should_close_window: false,
            process_names: self.read_tab_process_names(task_id, &target, tab_id),
            error: None,
        }
    }

    pub fn select_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
    ) -> Result<TerminalTabMutationResult, TerminalTabServiceError> {
        let Some(target) = self.find_session_target(task_id)? else {
            return Ok(TerminalTabMutationResult::failure(NO_SESSION_MESSAGE));
        };
        if target.session_type == SessionType::Terminal {
            self.refresh_remote_terminal_tabs(task_id, &target)?;
            self.registry.select_local_terminal_tab(task_id, tab_id);
            return Ok(TerminalTabMutationResult::success());
        }

        let selected = match target.session_type {
            SessionType::Tmux => self
                .run_tab_command(&build_tmux_select_window_command(tab_id), target.ssh_host())
                .map(|_| ())
                .map_err(TabCommandFailure::from),
            SessionType::Herdr => self.select_herdr_tab(task_id, &target, tab_id),
            _ => self
                .build_zellij_select_command(&target, tab_id)
                .and_then(|command| {
                    self.run_tab_command(&command, target.ssh_host())
                        .map(|_| ())
                        .map_err(TabCommandFailure::from)
                }),
        };
        Ok(into_mutation_result(
            selected.map(|()| TerminalTabMutationResult::success()),
        ))
    }

    /// 멀티플렉서 세션의 마지막 탭은 닫지 않고 창만 닫는다.
    /// 그 탭을 지우면 세션까지 사라지는데, 세션 정리는 태스크 삭제 경로에만 있어야 한다.
    /// 목록을 읽지 못하면 세션이 이미 없는 것으로 보고 역시 아무것도 지우지 않는다.
    pub fn close_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
    ) -> Result<TerminalTabMutationResult, TerminalTabServiceError> {
        let Some(target) = self.find_session_target(task_id)? else {
            return Ok(TerminalTabMutationResult::failure(NO_SESSION_MESSAGE));
        };
        if target.session_type == SessionType::Terminal {
            let remaining_tab_count = match target.ssh_host() {
                Some(ssh_host) => self.registry.close_remote_terminal_tab(
                    task_id,
                    tab_id,
                    ssh_host,
                    &target.session_name,
                ),
                None => Ok(self.registry.close_local_terminal_tab(task_id, tab_id)),
            };
            let result = match remaining_tab_count {
                Ok(remaining_tab_count) => {
                    TerminalTabMutationResult::closed(remaining_tab_count == 0)
                }
                Err(message) => TerminalTabMutationResult::failure(message),
            };
            return Ok(result);
        }

        let result = self
            .tab_close_queues
            .run_in_turn(target.close_queue_key(), || {
                into_mutation_result(self.close_multiplexer_tab(&target, tab_id))
            });
        Ok(result)
    }

    pub fn rename_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        name: &str,
    ) -> Result<TerminalTabMutationResult, TerminalTabServiceError> {
        let trimmed_name = js_trim(name);
        if trimmed_name.is_empty() {
            return Ok(TerminalTabMutationResult::failure(
                "탭 이름은 비워 둘 수 없습니다.",
            ));
        }
        let Some(target) = self.find_session_target(task_id)? else {
            return Ok(TerminalTabMutationResult::failure(NO_SESSION_MESSAGE));
        };
        if target.session_type == SessionType::Terminal {
            self.refresh_remote_terminal_tabs(task_id, &target)?;
            self.registry
                .rename_local_terminal_tab(task_id, tab_id, trimmed_name);
            return Ok(TerminalTabMutationResult::success());
        }
        Ok(into_mutation_result(self.rename_multiplexer_tab(
            &target,
            tab_id,
            trimmed_name,
        )))
    }

    /// 목표 위치는 탭 바의 배열 위치다. 음수는 거부한다
    pub fn move_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        target_index: i64,
    ) -> Result<TerminalTabMutationResult, TerminalTabServiceError> {
        let Ok(target_position) = usize::try_from(target_index) else {
            return Ok(TerminalTabMutationResult::failure(
                "이동할 위치가 올바르지 않습니다.",
            ));
        };
        let Some(target) = self.find_session_target(task_id)? else {
            return Ok(TerminalTabMutationResult::failure(NO_SESSION_MESSAGE));
        };
        if target.session_type == SessionType::Terminal {
            self.refresh_remote_terminal_tabs(task_id, &target)?;
            self.registry
                .move_local_terminal_tab(task_id, tab_id, target_position);
            return Ok(TerminalTabMutationResult::success());
        }
        Ok(into_mutation_result(self.move_multiplexer_tab(
            &target,
            tab_id,
            target_position,
        )))
    }

    /// 세션 종류나 이름이 비어 있으면 연결된 세션이 없는 것으로 본다. 빈 호스트는 로컬이다
    fn find_session_target(
        &self,
        task_id: &str,
    ) -> Result<Option<TerminalSessionTarget>, DatabaseError> {
        let task = {
            let connection = self.database.connection();
            find_task(&connection, task_id)?
        };
        let Some(task) = task else {
            return Ok(None);
        };
        let (Some(session_type), Some(session_name)) = (task.session_type, task.session_name)
        else {
            return Ok(None);
        };
        if session_name.is_empty() {
            return Ok(None);
        }
        Ok(Some(TerminalSessionTarget {
            session_type,
            session_name,
            ssh_host: task.ssh_host.filter(|ssh_host| !ssh_host.is_empty()),
            worktree_path: task.worktree_path,
        }))
    }

    /// 원격 terminal 세션은 탭 조작 전에 원격 탭 목록을 레지스트리로 다시 읽는다. 실패는 호출자에게 그대로 던진다
    fn refresh_remote_terminal_tabs(
        &self,
        task_id: &str,
        target: &TerminalSessionTarget,
    ) -> Result<(), TerminalTabServiceError> {
        if let Some(ssh_host) = target.ssh_host() {
            self.registry
                .list_remote_terminal_tabs(task_id, ssh_host, &target.session_name)
                .map_err(TerminalTabServiceError::RemoteTerminalTabs)?;
        }
        Ok(())
    }

    fn read_terminal_session_tabs(
        &self,
        task_id: &str,
        target: &TerminalSessionTarget,
    ) -> Result<Vec<TerminalTab>, String> {
        match target.ssh_host() {
            Some(ssh_host) => {
                self.registry
                    .list_remote_terminal_tabs(task_id, ssh_host, &target.session_name)
            }
            None => Ok(self.registry.list_local_terminal_tabs(task_id)),
        }
    }

    /// 공유 Space의 Herdr 탭은 전역 focus 대신 이 task 클라이언트가 고른 탭을 활성으로 표시한다
    fn read_multiplexer_tabs_for_display(
        &self,
        task_id: &str,
        target: &TerminalSessionTarget,
    ) -> TabCommandResult<Vec<TerminalTab>> {
        let tabs = self.read_tabs(target)?;
        let active_tab_id = match target.session_type {
            SessionType::Herdr => self.registry.herdr_task_active_tab_id(task_id),
            _ => None,
        };
        let Some(active_tab_id) = active_tab_id.filter(|tab_id| !tab_id.is_empty()) else {
            return Ok(tabs);
        };
        let marked_tabs = tabs
            .into_iter()
            .map(|tab| TerminalTab {
                is_active: tab.id == active_tab_id,
                ..tab
            })
            .collect();
        Ok(marked_tabs)
    }

    fn read_tabs(&self, target: &TerminalSessionTarget) -> TabCommandResult<Vec<TerminalTab>> {
        match target.session_type {
            SessionType::Herdr => Ok(self
                .herdr
                .read_tabs(&target.session_name, target.ssh_host())?),
            SessionType::Tmux => {
                // 탭 바가 주기적으로 조회하므로 세션이 꺼져 있을 때의 실패를 매번 오류 로그로 남기지 않는다.
                let output = self.run_polling_tab_command(
                    &build_tmux_list_windows_command(&target.session_name),
                    target.ssh_host(),
                )?;
                Ok(parse_tmux_window_list(&output))
            }
            _ => self.read_zellij_tabs(target),
        }
    }

    fn read_zellij_tabs(
        &self,
        target: &TerminalSessionTarget,
    ) -> TabCommandResult<Vec<TerminalTab>> {
        if self.has_zellij_tab_id_support(target.ssh_host()) {
            let output = self.run_tab_command(
                &build_zellij_list_tabs_command(&target.session_name),
                target.ssh_host(),
            )?;
            return Ok(parse_zellij_tab_list(&output));
        }

        let (tab_names_output, layout_output) = std::thread::scope(|scope| {
            let layout_reader = scope.spawn(|| {
                self.run_tab_command(
                    &build_zellij_dump_layout_command(&target.session_name),
                    target.ssh_host(),
                )
                .unwrap_or_default()
            });
            let tab_names_output = self.run_tab_command(
                &build_zellij_query_tab_names_command(&target.session_name),
                target.ssh_host(),
            );
            (
                tab_names_output,
                layout_reader.join().expect("zellij 레이아웃 조회 스레드"),
            )
        });
        Ok(parse_zellij_tab_names_with_focus(
            &tab_names_output?,
            &layout_output,
        ))
    }

    fn create_herdr_tab(
        &self,
        task_id: &str,
        target: &TerminalSessionTarget,
    ) -> TabCommandResult<()> {
        let tabs = self.read_tabs(target)?;
        let workspace_tab = tabs.iter().find(|tab| tab.is_active).or(tabs.first());
        let workspace_id = workspace_tab
            .map(|tab| herdr_workspace_id(&tab.id))
            .unwrap_or_default();
        if workspace_id.is_empty() {
            return Err(TabCommandFailure(
                "Herdr 워크스페이스를 찾을 수 없습니다.".to_string(),
            ));
        }
        let is_shared_space = parse_herdr_session_target(&target.session_name)?
            .workspace_key
            .is_some();
        let mut arguments = vec!["tab", "create", "--workspace", workspace_id];
        if let Some(worktree_path) = target
            .worktree_path
            .as_deref()
            .filter(|path| !path.is_empty())
        {
            arguments.extend(["--cwd", worktree_path]);
        }
        arguments.push(if is_shared_space {
            "--no-focus"
        } else {
            "--focus"
        });
        let created =
            self.herdr
                .run_command(&target.session_name, &arguments, target.ssh_host())?;
        if is_shared_space {
            let created_tab_id = created["tab"]["tab_id"].as_str().ok_or_else(|| {
                "Cannot read properties of undefined (reading 'tab_id')".to_string()
            })?;
            self.registry
                .focus_herdr_task_tab(task_id, created_tab_id)?;
        }
        Ok(())
    }

    /// 공유 Space에서는 전역 focus를 바꾸지 않고 이 task 클라이언트에만 선택을 전달한다
    fn select_herdr_tab(
        &self,
        task_id: &str,
        target: &TerminalSessionTarget,
        tab_id: &str,
    ) -> TabCommandResult<()> {
        if parse_herdr_session_target(&target.session_name)?
            .workspace_key
            .is_none()
        {
            self.herdr.run_command(
                &target.session_name,
                &["tab", "focus", tab_id],
                target.ssh_host(),
            )?;
            return Ok(());
        }
        if !self.read_tabs(target)?.iter().any(|tab| tab.id == tab_id) {
            return Err(TabCommandFailure(TAB_NOT_FOUND_MESSAGE.to_string()));
        }
        self.registry.focus_herdr_task_tab(task_id, tab_id)?;
        Ok(())
    }

    fn build_zellij_select_command(
        &self,
        target: &TerminalSessionTarget,
        tab_id: &str,
    ) -> TabCommandResult<String> {
        if self.has_zellij_tab_id_support(target.ssh_host()) {
            return Ok(build_zellij_go_to_tab_by_id_command(
                &target.session_name,
                tab_id,
            ));
        }
        let Some(tab_name) = find_tab_name(&self.read_zellij_tabs(target)?, tab_id) else {
            return Err(TabCommandFailure(TAB_NOT_FOUND_MESSAGE.to_string()));
        };
        Ok(build_zellij_go_to_tab_by_name_command(
            &target.session_name,
            &tab_name,
        ))
    }

    /// 같은 세션의 닫기 대기열 안에서 실행된다. 목록을 읽지 못하면 탭이 없는 것으로 본다
    fn close_multiplexer_tab(
        &self,
        target: &TerminalSessionTarget,
        tab_id: &str,
    ) -> TabCommandResult<TerminalTabMutationResult> {
        let tabs_before_close = self.read_tabs(target).unwrap_or_default();
        if tabs_before_close.len() <= 1 {
            return Ok(TerminalTabMutationResult::closed(true));
        }

        match target.session_type {
            SessionType::Tmux => {
                self.run_tab_command(&build_tmux_kill_window_command(tab_id), target.ssh_host())?;
            }
            SessionType::Herdr => {
                if !tabs_before_close.iter().any(|tab| tab.id == tab_id) {
                    return Ok(TerminalTabMutationResult::failure(TAB_NOT_FOUND_MESSAGE));
                }
                self.herdr.run_command(
                    &target.session_name,
                    &["tab", "close", tab_id],
                    target.ssh_host(),
                )?;
            }
            _ if self.has_zellij_tab_id_support(target.ssh_host()) => {
                self.run_tab_command(
                    &build_zellij_close_tab_by_id_command(&target.session_name, tab_id),
                    target.ssh_host(),
                )?;
            }
            _ => {
                let Some(tab_name) = find_tab_name(&self.read_zellij_tabs(target)?, tab_id) else {
                    return Ok(TerminalTabMutationResult::failure(TAB_NOT_FOUND_MESSAGE));
                };
                self.run_tab_commands(
                    &build_zellij_close_focused_tab_commands(&target.session_name, &tab_name),
                    target.ssh_host(),
                )?;
            }
        }
        Ok(TerminalTabMutationResult::closed(false))
    }

    fn rename_multiplexer_tab(
        &self,
        target: &TerminalSessionTarget,
        tab_id: &str,
        name: &str,
    ) -> TabCommandResult<TerminalTabMutationResult> {
        match target.session_type {
            SessionType::Tmux => {
                self.run_tab_command(
                    &build_tmux_rename_window_command(tab_id, name),
                    target.ssh_host(),
                )?;
            }
            SessionType::Herdr => {
                self.herdr.run_command(
                    &target.session_name,
                    &["tab", "rename", tab_id, name],
                    target.ssh_host(),
                )?;
            }
            _ if self.has_zellij_tab_id_support(target.ssh_host()) => {
                self.run_tab_command(
                    &build_zellij_rename_tab_by_id_command(&target.session_name, tab_id, name),
                    target.ssh_host(),
                )?;
            }
            _ => {
                let Some(tab_name) = find_tab_name(&self.read_zellij_tabs(target)?, tab_id) else {
                    return Ok(TerminalTabMutationResult::failure(TAB_NOT_FOUND_MESSAGE));
                };
                self.run_tab_commands(
                    &build_zellij_rename_focused_tab_commands(
                        &target.session_name,
                        &tab_name,
                        name,
                    ),
                    target.ssh_host(),
                )?;
            }
        }
        Ok(TerminalTabMutationResult::success())
    }

    /// 두 멀티플렉서 모두 "몇 번째로 옮겨라"를 그대로 받지 못한다.
    /// tmux는 목표 window의 앞뒤 중 어디에 끼울지, zellij는 어느 방향으로 몇 칸 밀지 정해야 해서
    /// 현재 목록을 먼저 읽는다. 탭 바가 넘기는 값은 배열 위치이므로 실제 대상도 여기서 고른다.
    fn move_multiplexer_tab(
        &self,
        target: &TerminalSessionTarget,
        tab_id: &str,
        target_position: usize,
    ) -> TabCommandResult<TerminalTabMutationResult> {
        let current_tabs = self.read_tabs(target)?;
        let Some(current_tab) = current_tabs.iter().find(|tab| tab.id == tab_id) else {
            return Ok(TerminalTabMutationResult::failure(TAB_NOT_FOUND_MESSAGE));
        };
        let bounded_position = target_position.min(current_tabs.len() - 1);
        let destination_tab = &current_tabs[bounded_position];

        let move_commands = match target.session_type {
            SessionType::Tmux => build_tmux_move_window_commands(
                &target.session_name,
                tab_id,
                current_tab.native_index,
                destination_tab.native_index,
            ),
            SessionType::Herdr => {
                return self.move_herdr_tab(target, &current_tabs, current_tab, destination_tab);
            }
            _ if self.has_zellij_tab_id_support(target.ssh_host()) => {
                build_zellij_move_tab_by_id_commands(
                    &target.session_name,
                    tab_id,
                    current_tab.native_index,
                    bounded_position as i64,
                )
            }
            _ => build_zellij_move_tab_commands(
                &target.session_name,
                &current_tab.name,
                current_tab.native_index,
                bounded_position as i64,
            ),
        };
        self.run_tab_commands(&move_commands, target.ssh_host())?;
        Ok(TerminalTabMutationResult::success())
    }

    /// Herdr는 워크스페이스 안의 삽입 경계를 받으므로, 뒤로 옮길 때는 목표 탭 다음 경계로 바꾼다
    fn move_herdr_tab(
        &self,
        target: &TerminalSessionTarget,
        current_tabs: &[TerminalTab],
        current_tab: &TerminalTab,
        destination_tab: &TerminalTab,
    ) -> TabCommandResult<TerminalTabMutationResult> {
        let workspace_id = herdr_workspace_id(&current_tab.id);
        if herdr_workspace_id(&destination_tab.id) != workspace_id {
            return Ok(TerminalTabMutationResult::failure(
                "Herdr 탭은 같은 워크스페이스 안에서 이동할 수 있습니다.",
            ));
        }
        let workspace_tabs: Vec<&TerminalTab> = current_tabs
            .iter()
            .filter(|tab| herdr_workspace_id(&tab.id) == workspace_id)
            .collect();
        // 두 탭 모두 이 워크스페이스에 있음을 위에서 확인했다
        let position_of = |tab_id: &str| {
            workspace_tabs
                .iter()
                .position(|tab| tab.id == tab_id)
                .unwrap_or_default()
        };
        let current_position = position_of(&current_tab.id);
        let destination_position = position_of(&destination_tab.id);
        let insert_index = match current_position < destination_position {
            true => destination_position + 1,
            false => destination_position,
        };
        self.herdr.move_tab(
            &target.session_name,
            &current_tab.id,
            insert_index as i64,
            target.ssh_host(),
        )?;
        Ok(TerminalTabMutationResult::success())
    }

    /// 조회 실패를 작업 없음으로 간주하면 실행 중인 프로세스를 경고 없이 종료할 수 있어 실패는 `None`이다.
    /// 원격 terminal 세션의 로컬 PTY는 ssh라 원격 작업 이름으로 보여 주지 않는다
    fn read_tab_process_names(
        &self,
        task_id: &str,
        target: &TerminalSessionTarget,
        tab_id: &str,
    ) -> Option<Vec<String>> {
        match target.session_type {
            SessionType::Terminal if target.ssh_host().is_none() => self
                .registry
                .local_terminal_tab_process(task_id, tab_id)
                .map(|process_name| vec![process_name]),
            SessionType::Tmux => {
                let command = build_tmux_list_mirror_panes_command(&target.session_name);
                let output = self.run_tab_command(&command, target.ssh_host()).ok()?;
                collect_tab_process_names(&parse_tmux_mirror_pane_list(&output), tab_id)
            }
            SessionType::Herdr => {
                let panes = self
                    .herdr
                    .list_mirror_panes(&target.session_name, target.ssh_host())
                    .ok()?;
                collect_tab_process_names(&panes, tab_id)
            }
            _ => None,
        }
    }

    fn has_zellij_tab_id_support(&self, ssh_host: Option<&str>) -> bool {
        self.zellij_id_targeting_support
            .resolve_zellij_id_targeting_support(
                ssh_host,
                self.runner.as_ref(),
                &tab_command_options(false),
            )
    }

    fn run_tab_command(
        &self,
        command: &str,
        ssh_host: Option<&str>,
    ) -> Result<String, ExecGitError> {
        self.runner
            .exec_git(command, ssh_host, &tab_command_options(false))
    }

    /// 실패가 예상되는 주기적 조회라 오류 로그를 남기지 않는다
    fn run_polling_tab_command(
        &self,
        command: &str,
        ssh_host: Option<&str>,
    ) -> Result<String, ExecGitError> {
        self.runner
            .exec_git(command, ssh_host, &tab_command_options(true))
    }

    fn run_tab_commands(
        &self,
        commands: &[String],
        ssh_host: Option<&str>,
    ) -> Result<(), ExecGitError> {
        for command in commands {
            self.run_tab_command(command, ssh_host)?;
        }
        Ok(())
    }
}

fn tab_command_options(is_failure_expected: bool) -> ExecGitOptions {
    ExecGitOptions {
        timeout: Some(TAB_COMMAND_TIMEOUT),
        is_failure_expected,
        ..ExecGitOptions::default()
    }
}
