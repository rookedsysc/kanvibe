//! pane chat이 읽는 pane 하나의 현재 모습(TS `paneChatSurface.ts`).
//!
//! 태스크의 세션 종류마다 어느 pane이 사용자가 보고 있는 pane인지, 그 pane의 root PID와 입력란 줄·커서가 어디인지 읽는다.
//! pane을 새로 만들거나 고르거나 크기를 바꾸지 않고, 이미 붙어 있는 클라이언트가 보고 있는 pane만 고른다.

mod layout;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use kanvibe_ai::pane_chat::codex_display::{TerminalCellRow, is_dim_hint_empty_composer};
use kanvibe_ai::pane_chat::snapshot::PaneChatLayout;
use kanvibe_domain::task::SessionType;
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use kanvibe_process::worktree::quote_for_posix_shell;
use regex::Regex;
use serde_json::Value;

use crate::herdr::{HerdrAgentSession, HerdrError, HerdrService, HerdrTaskPaneTab};
use crate::pane_mirror::{build_tmux_capture_pane_command, build_zellij_dump_pane_command};
use crate::pane_mirror::{
    build_zellij_list_mirror_panes_command, build_zellij_read_viewport_command,
};
use crate::screen::TerminalScreen;
use crate::terminal_registry::{
    HerdrSurfaceBounds, TaskTerminalAttachment, TerminalRegistry, TerminalTabSnapshot,
};

const PANE_CHAT_PROBE_TIMEOUT: Duration = Duration::from_millis(5_000);

/// Node `util.stripVTControlCharacters`가 쓰는 ANSI·OSC 패턴(ansi-regex). `\d`는 JS처럼 ASCII 숫자만 뜻하도록 풀어 썼다
static VT_CONTROL_SEQUENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"[\x1B\x{9B}][\[\]()#;?]*",
        r"(?:(?:(?:(?:;[-a-zA-Z0-9/#&.:=?%@~_]+)*",
        r"|[a-zA-Z0-9]+(?:;[-a-zA-Z0-9/#&.:=?%@~_]*)*)?",
        r"(?:\x07|\x1B\x5C|\x{9C}))",
        r"|(?:(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?",
        r"[0-9A-PR-TZcf-ntqry=><~]))",
    ))
    .expect("VT 제어 시퀀스 패턴")
});

/// 화면 글에서 색상·커서 이동·OSC 링크 같은 제어 시퀀스를 뗀다(Node `stripVTControlCharacters`)
pub(crate) fn strip_vt_control_characters(text: &str) -> String {
    VT_CONTROL_SEQUENCE.replace_all(text, "").into_owned()
}

/// 태스크 조회(TS `kanbanService.getTaskById`)에서 pane chat이 쓰는 값만 담는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatTask {
    pub id: String,
    pub session_type: Option<SessionType>,
    pub session_name: Option<String>,
    pub ssh_host: Option<String>,
    pub worktree_path: Option<String>,
    /// 태스크가 속한 프로젝트의 저장소 경로(TS `task.project?.repoPath`)
    pub project_repo_path: Option<String>,
}

impl PaneChatTask {
    /// TS는 빈 문자열 호스트를 로컬로 본다
    pub fn ssh_host(&self) -> Option<&str> {
        self.ssh_host.as_deref().filter(|host| !host.is_empty())
    }

    /// TS `task.sessionName`이 truthy일 때의 값
    pub fn session_name(&self) -> Option<&str> {
        self.session_name.as_deref().filter(|name| !name.is_empty())
    }
}

/// TS `getTaskById`. 태스크가 없으면 `Ok(None)`이다
pub trait PaneChatTaskLookup: Send + Sync {
    fn find_pane_chat_task(&self, task_id: &str) -> Result<Option<PaneChatTask>, String>;
}

/// 터미널 레지스트리(TS `@/lib/terminal`)에서 pane chat이 쓰는 부분
pub trait PaneChatTerminals: Send + Sync {
    fn list_local_terminal_tabs(&self, task_id: &str) -> Vec<TerminalTab>;
    fn read_local_terminal_tab_snapshot(
        &self,
        task_id: &str,
        tab_id: &str,
    ) -> Option<TerminalTabSnapshot>;
    fn read_task_terminal_attachment(&self, task_id: &str) -> Option<TaskTerminalAttachment>;
    fn herdr_task_active_pane_id(&self, task_id: &str) -> Option<String>;
    fn herdr_task_surface_bounds(&self, task_id: &str) -> Option<HerdrSurfaceBounds>;
    fn focus_herdr_task_pane(&self, task_id: &str, pane_id: &str) -> Result<(), String>;
}

impl PaneChatTerminals for TerminalRegistry {
    fn list_local_terminal_tabs(&self, task_id: &str) -> Vec<TerminalTab> {
        TerminalRegistry::list_local_terminal_tabs(self, task_id)
    }

    fn read_local_terminal_tab_snapshot(
        &self,
        task_id: &str,
        tab_id: &str,
    ) -> Option<TerminalTabSnapshot> {
        TerminalRegistry::read_local_terminal_tab_snapshot(self, task_id, tab_id)
    }

    fn read_task_terminal_attachment(&self, task_id: &str) -> Option<TaskTerminalAttachment> {
        TerminalRegistry::read_task_terminal_attachment(self, task_id)
    }

    fn herdr_task_active_pane_id(&self, task_id: &str) -> Option<String> {
        TerminalRegistry::herdr_task_active_pane_id(self, task_id)
    }

    fn herdr_task_surface_bounds(&self, task_id: &str) -> Option<HerdrSurfaceBounds> {
        TerminalRegistry::herdr_task_surface_bounds(self, task_id)
    }

    fn focus_herdr_task_pane(&self, task_id: &str, pane_id: &str) -> Result<(), String> {
        TerminalRegistry::focus_herdr_task_pane(self, task_id, pane_id)
    }
}

/// Herdr 연동(TS `@/lib/herdr`)에서 pane chat이 쓰는 부분
pub trait PaneChatHerdr: Send + Sync {
    fn list_task_pane_tab_ids(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<HerdrTaskPaneTab>, HerdrError>;
    fn run_command(
        &self,
        session_name: &str,
        arguments: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<Value, HerdrError>;
}

impl PaneChatHerdr for HerdrService {
    fn list_task_pane_tab_ids(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<HerdrTaskPaneTab>, HerdrError> {
        self.list_herdr_task_pane_tab_ids(session_name, ssh_host)
    }

    fn run_command(
        &self,
        session_name: &str,
        arguments: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<Value, HerdrError> {
        self.run_herdr_command(session_name, arguments, ssh_host)
    }
}

/// Herdr pane의 지금 화면을 그리는 출력과 그 크기
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrPaneFrame {
    pub output: String,
    pub width: u16,
    pub height: u16,
}

/// 모바일 브리지 서비스(TS `mobileBridgeService`)에서 pane chat이 쓰는 부분
pub trait PaneChatMobileBridge: Send + Sync {
    fn read_herdr_pane_frame_for_chat(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> Result<HerdrPaneFrame, String>;
    /// pane에 입력 바이트를 그대로 쓴다(TS `writeToPane`)
    fn write_to_pane(&self, task_id: &str, pane_id: &str, bytes: &str) -> Result<(), String>;
}

/// pane을 읽다가 난 실패. 호출자는 대부분 대상을 확인하지 못한 것으로 보고 넘어간다
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PaneChatSurfaceError(pub String);

impl From<ExecGitError> for PaneChatSurfaceError {
    fn from(error: ExecGitError) -> Self {
        Self(error.message)
    }
}

impl From<HerdrError> for PaneChatSurfaceError {
    fn from(error: HerdrError) -> Self {
        Self(error.to_string())
    }
}

impl From<serde_json::Error> for PaneChatSurfaceError {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

/// pane 하나의 지금 모습. 화면을 캡처하지 않은 조회는 커서 행·화면 줄·관찰 순번이 비어 있다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatSurface {
    pub task: PaneChatTask,
    pub pane_id: String,
    pub root_pid: Option<i64>,
    /// Herdr가 그 pane에서 돈다고 알려 준 에이전트의 대화
    pub agent_session: Option<HerdrAgentSession>,
    /// 일반 터미널과 Herdr의 현재 화면. tmux와 Zellij는 비워 두고, 보여 줄 때만 `read_pane_chat_screen`으로 스크롤백을 읽는다
    pub screen: String,
    pub input_line: Option<String>,
    pub cursor_column: Option<usize>,
    pub cursor_row: Option<usize>,
    pub viewport_lines: Option<Vec<String>>,
    pub viewport_columns: Option<usize>,
    /// 커서 줄이 CLI가 dim으로 그린 빈 입력란 안내인지
    pub is_input_placeholder: bool,
    /// 화면을 캡처한 순서. 늦게 끝난 조회가 더 최근 관찰을 덮지 못하게 견준다
    pub observation: Option<u64>,
}

impl PaneChatSurface {
    fn bare(task: PaneChatTask, pane_id: String, root_pid: Option<i64>) -> Self {
        Self {
            task,
            pane_id,
            root_pid,
            agent_session: None,
            screen: String::new(),
            input_line: None,
            cursor_column: None,
            cursor_row: None,
            viewport_lines: None,
            viewport_columns: None,
            is_input_placeholder: false,
            observation: None,
        }
    }
}

/// pane chat 서비스가 pane을 읽는 길. 실제 구현은 [`PaneChatSurfaceReader`]이고 서비스 테스트는 가짜를 쓴다
pub trait PaneChatSurfaces: Send + Sync {
    /// `should_capture`가 거짓이면 화면을 읽지 않고 pane과 root PID만 확인한다
    fn read_pane_chat_surface(
        &self,
        task_id: &str,
        requested_pane_id: Option<&str>,
        should_capture: bool,
    ) -> Result<Option<PaneChatSurface>, PaneChatSurfaceError>;
    /// pane의 CLI 화면을 스크롤백까지 읽는다
    fn read_pane_chat_screen(
        &self,
        surface: &PaneChatSurface,
    ) -> Result<String, PaneChatSurfaceError>;
    fn read_pane_chat_layout(
        &self,
        task_id: &str,
        terminal_tab_id: Option<&str>,
    ) -> Result<PaneChatLayout, PaneChatSurfaceError>;
}

/// pane을 실제로 읽는 구현. 앱이 한 번 만들어 서비스에 넘긴다
pub struct PaneChatSurfaceReader {
    tasks: Arc<dyn PaneChatTaskLookup>,
    runner: Arc<dyn ShellCommandRunner>,
    terminals: Arc<dyn PaneChatTerminals>,
    herdr: Arc<dyn PaneChatHerdr>,
    mobile_bridge: Arc<dyn PaneChatMobileBridge>,
    /// TS 모듈 전역 `nextObservation`
    next_observation: AtomicU64,
}

impl PaneChatSurfaceReader {
    pub fn new(
        tasks: Arc<dyn PaneChatTaskLookup>,
        runner: Arc<dyn ShellCommandRunner>,
        terminals: Arc<dyn PaneChatTerminals>,
        herdr: Arc<dyn PaneChatHerdr>,
        mobile_bridge: Arc<dyn PaneChatMobileBridge>,
    ) -> Self {
        Self {
            tasks,
            runner,
            terminals,
            herdr,
            mobile_bridge,
            next_observation: AtomicU64::new(0),
        }
    }

    fn run(&self, task: &PaneChatTask, command: &str) -> Result<String, PaneChatSurfaceError> {
        let options = ExecGitOptions {
            timeout: Some(PANE_CHAT_PROBE_TIMEOUT),
            ..ExecGitOptions::default()
        };
        Ok(self.runner.exec_git(command, task.ssh_host(), &options)?)
    }

    fn read_terminal_surface(
        &self,
        task: PaneChatTask,
        requested_pane_id: Option<&str>,
        observation: Option<u64>,
    ) -> Option<PaneChatSurface> {
        // 원격 plain PTY의 로컬 PID는 ssh이므로 원격 CLI의 신원을 보장할 수 없다.
        if task.ssh_host().is_some() {
            return None;
        }
        let tabs = self.terminals.list_local_terminal_tabs(&task.id);
        let tab = match requested_pane_id {
            Some(pane_id) => tabs.iter().find(|entry| entry.id == pane_id),
            None => tabs.iter().find(|entry| entry.is_active),
        }?;
        let snapshot = self
            .terminals
            .read_local_terminal_tab_snapshot(&task.id, &tab.id)?;
        let reading = snapshot.reading;
        let mut surface =
            PaneChatSurface::bare(task, tab.id.clone(), Some(i64::from(snapshot.pid)));
        surface.input_line = Some(reading.input_line);
        surface.cursor_column = Some(reading.cursor_column);
        if observation.is_some() {
            surface.screen = strip_vt_control_characters(&reading.screen);
            surface.cursor_row = Some(reading.cursor_row);
            surface.viewport_lines = Some(reading.viewport_lines);
            surface.viewport_columns = Some(reading.viewport_columns);
            surface.is_input_placeholder = reading.is_input_placeholder;
            surface.observation = observation;
        }
        Some(surface)
    }

    /// 데스크톱에 실제로 붙어 있는 tmux client가 보고 있는 pane. grouped session은 창을 공유해도 활성 창이 달라 client의 pane을 쓴다
    fn read_attached_tmux_pane(
        &self,
        task: &PaneChatTask,
        session_name: &str,
    ) -> Result<Option<String>, PaneChatSurfaceError> {
        let attachment = self.terminals.read_task_terminal_attachment(&task.id);
        let Some(attachment) = attachment.filter(|attachment| {
            task.ssh_host().is_none()
                && Some(attachment.session_type) == task.session_type
                && attachment.session_name == session_name
        }) else {
            return Ok(None);
        };
        let clients = self.run(
            task,
            "tmux list-clients -F '#{client_pid}\t#{session_name}\t#{session_id}\t#{session_group}\t#{pane_id}'",
        )?;
        let client: Option<Vec<&str>> = clients
            .split('\n')
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .find(|entry| js_number(entry[0]) == f64::from(attachment.pid));
        let Some(client) = client else {
            return Ok(None);
        };
        let canonical = self.run(
            task,
            &format!(
                "tmux display-message -p -t {} '#{{session_id}}\t#{{session_group}}'",
                quote_for_posix_shell(session_name)
            ),
        )?;
        let canonical: Vec<&str> = kanvibe_ai::javascript::js_trim_end(&canonical)
            .split('\t')
            .collect();
        let canonical_group = canonical.get(1).copied().filter(|group| !group.is_empty());
        let is_same_session = client.get(2) == canonical.first();
        let is_same_group = canonical_group.is_some() && client.get(3).copied() == canonical_group;
        if !is_same_session && !is_same_group {
            return Ok(None);
        }
        Ok(client
            .get(4)
            .filter(|pane_id| !pane_id.is_empty())
            .map(|pane_id| pane_id.to_string()))
    }

    fn read_tmux_surface(
        &self,
        task: PaneChatTask,
        requested_pane_id: Option<&str>,
        observation: Option<u64>,
    ) -> Result<Option<PaneChatSurface>, PaneChatSurfaceError> {
        let session_name = task.session_name().unwrap_or_default().to_string();
        let selected_pane_id = match requested_pane_id {
            Some(pane_id) => pane_id.to_string(),
            None => match self.read_attached_tmux_pane(&task, &session_name)? {
                Some(pane_id) => pane_id,
                None => return Ok(None),
            },
        };
        let format = "#{pane_id}\t#{pane_pid}\t#{window_active}\t#{pane_active}";
        let output = self.run(
            &task,
            &format!(
                "tmux list-panes -s -t {} -F {}",
                quote_for_posix_shell(&session_name),
                quote_for_posix_shell(format)
            ),
        )?;
        let pane = kanvibe_ai::javascript::js_trim(&output)
            .split('\n')
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .find(|entry| entry[0] == selected_pane_id);
        let Some(pane) = pane else {
            return Ok(None);
        };
        let root_pid = pane.get(1).map_or(f64::NAN, |pid| js_number(pid));
        if !is_safe_integer(root_pid) || root_pid < 1.0 {
            return Ok(None);
        }
        let mut surface = PaneChatSurface::bare(task, pane[0].to_string(), Some(root_pid as i64));
        if observation.is_some() {
            self.capture_tmux_cursor_and_screen(&mut surface)?;
            surface.observation = observation;
        }
        Ok(Some(surface))
    }

    fn capture_tmux_cursor_and_screen(
        &self,
        surface: &mut PaneChatSurface,
    ) -> Result<(), PaneChatSurfaceError> {
        let target = quote_for_posix_shell(&surface.pane_id);
        let cursor_and_screen = self.run(
            &surface.task,
            &format!(
                "tmux display-message -p -t {target} '#{{cursor_x}}\t#{{cursor_y}}\t#{{pane_width}}\t#{{pane_height}}' \\; capture-pane -ep -t {target}"
            ),
        )?;
        let mut rows = cursor_and_screen.split('\n');
        let cursor = rows.next().unwrap_or_default();
        let lines: Vec<&str> = rows.collect();
        let capture = TmuxCursorCapture::parse(cursor);
        let visible_lines = match capture.height.filter(|height| *height > 0) {
            Some(height) => &lines[..(height as usize).min(lines.len())],
            None => &lines[..],
        };
        let viewport_lines: Vec<String> = visible_lines
            .iter()
            .map(|line| strip_vt_control_characters(line))
            .collect();
        surface.cursor_row = None;
        if let (Some(column), Some(row)) = (capture.column, capture.row)
            && row >= 0
            && (row as usize) < lines.len()
        {
            surface.cursor_column = usize::try_from(column).ok();
            surface.cursor_row = Some(row as usize);
            surface.input_line = viewport_lines.get(row as usize).cloned();
        }
        if let Some((width, height, row, column)) = capture.within_viewport() {
            surface.viewport_columns = Some(width);
            let mut terminal = TerminalScreen::new(clamp_to_u16(width), clamp_to_u16(height));
            terminal.write(lines[..height.min(lines.len())].join("\r\n").as_bytes());
            let cursor_line = terminal.row(row);
            surface.is_input_placeholder = is_dim_hint_empty_composer(
                cursor_line
                    .as_ref()
                    .map(|line| line as &dyn TerminalCellRow),
                column,
            );
        }
        surface.viewport_lines = Some(viewport_lines);
        Ok(())
    }

    fn read_herdr_surface(
        &self,
        task: PaneChatTask,
        requested_pane_id: Option<&str>,
        observation: Option<u64>,
    ) -> Result<Option<PaneChatSurface>, PaneChatSurfaceError> {
        let pane_id = match requested_pane_id {
            Some(pane_id) => Some(pane_id.to_string()),
            None => self.terminals.herdr_task_active_pane_id(&task.id),
        };
        let Some(pane_id) = pane_id.filter(|pane_id| !pane_id.is_empty()) else {
            return Ok(None);
        };
        let session_name = task.session_name().unwrap_or_default().to_string();
        // 채팅 폴링마다 도는 확인이라 pane이 있는지만 읽는다.
        let task_panes = self
            .herdr
            .list_task_pane_tab_ids(&session_name, task.ssh_host())?;
        let Some(pane) = task_panes
            .into_iter()
            .find(|entry| entry.pane_id == pane_id)
        else {
            return Ok(None);
        };
        let response = self.herdr.run_command(
            &session_name,
            &["pane", "process-info", "--pane", &pane_id],
            task.ssh_host(),
        )?;
        let root_pid = read_herdr_root_pid(&response)?;
        let mut surface = PaneChatSurface::bare(task, pane_id, root_pid);
        surface.agent_session = pane.agent_session;
        if observation.is_some() {
            self.capture_herdr_frame(&mut surface)?;
            surface.observation = observation;
        }
        Ok(Some(surface))
    }

    /// Herdr가 그 pane에 그린 화면을 headless 터미널에 다시 그려 입력란 줄과 커서를 읽는다
    fn capture_herdr_frame(
        &self,
        surface: &mut PaneChatSurface,
    ) -> Result<(), PaneChatSurfaceError> {
        let frame = self
            .mobile_bridge
            .read_herdr_pane_frame_for_chat(&surface.task.id, &surface.pane_id)
            .map_err(PaneChatSurfaceError)?;
        let mut terminal = TerminalScreen::new(frame.width, frame.height);
        terminal.write(frame.output.as_bytes());
        let reading = terminal.read();
        surface.screen =
            kanvibe_ai::javascript::js_trim_end(&reading.viewport_lines.join("\n")).to_string();
        surface.input_line = Some(reading.input_line);
        surface.cursor_column = Some(reading.cursor_column);
        surface.cursor_row = Some(reading.cursor_row);
        surface.viewport_lines = Some(reading.viewport_lines);
        surface.viewport_columns = Some(reading.viewport_columns);
        surface.is_input_placeholder = reading.is_input_placeholder;
        Ok(())
    }

    fn read_zellij_surface(
        &self,
        task: PaneChatTask,
        requested_pane_id: Option<&str>,
    ) -> Result<Option<PaneChatSurface>, PaneChatSurfaceError> {
        let session_name = task.session_name().unwrap_or_default().to_string();
        let command = match requested_pane_id {
            Some(_) => build_zellij_list_mirror_panes_command(&session_name),
            None => build_zellij_read_viewport_command(&session_name),
        };
        let response: Value = serde_json::from_str(&self.run(&task, &command)?)?;
        let panes = match requested_pane_id {
            Some(_) => Some(&response),
            None => response.get(0),
        };
        let Some(Value::Array(panes)) = panes else {
            return Ok(None);
        };
        let pane = match requested_pane_id {
            Some(pane_id) => panes
                .iter()
                .find(|entry| is_zellij_terminal(entry) && zellij_pane_id(entry) == pane_id),
            None => find_zellij_focused_pane(panes, &response),
        };
        let Some(pane) = pane else {
            return Ok(None);
        };
        // PID를 제공하지 않는 Zellij 버전은 정확한 화면만 반환하고 입력은 허용하지 않는다.
        let pid = pane
            .get("terminal_pid")
            .filter(|pid| !pid.is_null())
            .or_else(|| pane.get("pid"));
        let root_pid = positive_safe_integer(pid);
        Ok(Some(PaneChatSurface::bare(
            task,
            zellij_pane_id(pane),
            root_pid,
        )))
    }
}

impl PaneChatSurfaces for PaneChatSurfaceReader {
    fn read_pane_chat_surface(
        &self,
        task_id: &str,
        requested_pane_id: Option<&str>,
        should_capture: bool,
    ) -> Result<Option<PaneChatSurface>, PaneChatSurfaceError> {
        let observation =
            should_capture.then(|| self.next_observation.fetch_add(1, Ordering::SeqCst) + 1);
        let task = self
            .tasks
            .find_pane_chat_task(task_id)
            .map_err(PaneChatSurfaceError)?;
        let Some(task) = task.filter(|task| task.session_name().is_some()) else {
            return Ok(None);
        };
        match task.session_type {
            Some(SessionType::Terminal) => {
                Ok(self.read_terminal_surface(task, requested_pane_id, observation))
            }
            Some(SessionType::Tmux) => self.read_tmux_surface(task, requested_pane_id, observation),
            Some(SessionType::Herdr) => {
                self.read_herdr_surface(task, requested_pane_id, observation)
            }
            Some(SessionType::Zellij) => self.read_zellij_surface(task, requested_pane_id),
            None => Ok(None),
        }
    }

    /// tmux와 Zellij는 외부 명령을 한 번 더 실행하고 큰 문자열을 만들므로, 대상 확인과 떼어 화면을 실제로 보여 줄 때만 부른다.
    /// 일반 터미널과 Herdr는 surface를 읽을 때 이미 얻은 화면을 그대로 쓴다.
    fn read_pane_chat_screen(
        &self,
        surface: &PaneChatSurface,
    ) -> Result<String, PaneChatSurfaceError> {
        let task = &surface.task;
        let command = match task.session_type {
            Some(SessionType::Tmux) => build_tmux_capture_pane_command(&surface.pane_id),
            Some(SessionType::Zellij) => build_zellij_dump_pane_command(
                task.session_name.as_deref().unwrap_or_default(),
                &surface.pane_id,
            ),
            _ => return Ok(surface.screen.clone()),
        };
        Ok(strip_vt_control_characters(&self.run(task, &command)?))
    }

    fn read_pane_chat_layout(
        &self,
        task_id: &str,
        terminal_tab_id: Option<&str>,
    ) -> Result<PaneChatLayout, PaneChatSurfaceError> {
        layout::read_pane_chat_layout(self, task_id, terminal_tab_id)
    }
}

/// tmux `display-message`가 알려 준 커서와 pane 크기. 숫자가 아니면 비어 있다(JS `Number.isInteger` 판정)
struct TmuxCursorCapture {
    column: Option<i64>,
    row: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
}

impl TmuxCursorCapture {
    fn parse(cursor: &str) -> Self {
        let mut fields = cursor.split('\t').map(|field| js_integer(js_number(field)));
        Self {
            column: fields.next().flatten(),
            row: fields.next().flatten(),
            width: fields.next().flatten(),
            height: fields.next().flatten(),
        }
    }

    /// 크기가 양수이고 커서가 pane 안에 있을 때의 (너비, 높이, 행, 열)
    fn within_viewport(&self) -> Option<(usize, usize, usize, usize)> {
        let (width, height) = (self.width?, self.height?);
        let (row, column) = (self.row?, self.column?);
        let is_inside =
            width > 0 && height > 0 && (0..height).contains(&row) && (0..width).contains(&column);
        is_inside.then_some((
            width as usize,
            height as usize,
            row as usize,
            column as usize,
        ))
    }
}

/// Herdr `pane process-info`에서 셸 PID를, 없으면 첫 foreground 프로세스 PID를 읽는다
fn read_herdr_root_pid(response: &Value) -> Result<Option<i64>, PaneChatSurfaceError> {
    let info = response
        .get("process_info")
        .filter(|info| !info.is_null())
        .ok_or_else(|| {
            PaneChatSurfaceError("Herdr pane 프로세스 정보를 읽지 못했습니다.".to_string())
        })?;
    let foreground_pid = || {
        info.get("foreground_processes")
            .and_then(|processes| processes.get(0))
            .and_then(|process| process.get("pid"))
    };
    let pid = info
        .get("shell_pid")
        .filter(|pid| !pid.is_null())
        .or_else(foreground_pid);
    Ok(positive_safe_integer(pid))
}

fn is_zellij_terminal(entry: &Value) -> bool {
    entry.get("is_plugin") == Some(&Value::Bool(false))
}

/// TS `` `terminal_${entry.id}` ``
fn zellij_pane_id(entry: &Value) -> String {
    let id = entry.get("id").map_or_else(
        || "undefined".to_string(),
        kanvibe_ai::javascript::js_string,
    );
    format!("terminal_{id}")
}

/// 활성 탭에서 focus된 터미널 pane. 비활성 탭도 자기 focus pane을 기억하므로 탭부터 고른다
fn find_zellij_focused_pane<'a>(panes: &'a [Value], response: &Value) -> Option<&'a Value> {
    let tabs = response.get(1).and_then(Value::as_array)?;
    let active_tab = tabs
        .iter()
        .find(|entry| entry.get("active") == Some(&Value::Bool(true)))?;
    panes.iter().find(|entry| {
        is_zellij_terminal(entry)
            && entry.get("tab_id") == active_tab.get("tab_id")
            && entry.get("is_focused") == Some(&Value::Bool(true))
    })
}

fn clamp_to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// JS `Number(text)`. 빈 글은 0이고 숫자가 아니면 NaN이다
pub(crate) fn js_number(text: &str) -> f64 {
    let trimmed = kanvibe_ai::javascript::js_trim(text);
    if trimmed.is_empty() {
        return 0.0;
    }
    let is_numeric = trimmed.chars().all(|character| {
        character.is_ascii_digit() || matches!(character, '+' | '-' | '.' | 'e' | 'E')
    });
    match is_numeric {
        true => trimmed.parse().unwrap_or(f64::NAN),
        false => f64::NAN,
    }
}

/// JS `Number.isInteger`를 통과한 값
fn js_integer(number: f64) -> Option<i64> {
    (number.is_finite() && number.fract() == 0.0).then_some(number as i64)
}

/// JS `Number.isSafeInteger`
pub(crate) fn is_safe_integer(number: f64) -> bool {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    number.is_finite() && number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER
}

/// JSON 값이 양의 안전한 정수인 PID일 때만 돌려준다
fn positive_safe_integer(value: Option<&Value>) -> Option<i64> {
    let number = value?.as_f64()?;
    (is_safe_integer(number) && number > 0.0).then_some(number as i64)
}
