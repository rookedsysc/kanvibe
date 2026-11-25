//! KanVibe 터미널 계층. TS `src/lib/terminal*.ts`, tmux·zellij·Herdr 연동, 터미널 탭·pane 레이아웃 서비스,
//! pane chat 서비스의 UI가 아닌 부분을 옮긴다.
//!
//! 모든 셸 명령은 `kanvibe_process::exec::ShellCommandRunner::exec_git` 하나를 거친다.
//! 프로세스 전역 환경 변수는 [`environment::TerminalHostEnvironment`]가 시작할 때 한 번 읽고,
//! 자식 프로세스에는 `create_local_shell_environment`로 좁힌 환경만 넘긴다(CLAUDE.md 런타임 환경 규칙).

pub mod environment;
pub mod herdr;
pub mod herdr_client_proxy;
pub mod login_terminal;
pub mod mouse_selection;
pub mod osc52_clipboard;
pub mod pane_chat;
pub mod pane_layout_service;
pub mod pane_mirror;
pub mod pty;
pub mod remote_terminal_protocol;
pub mod remote_terminal_service;
pub mod running_agent_panes;
pub mod screen;
pub mod session_command_runner;
pub mod socket_forwarding;
pub mod terminal_bridge;
pub mod terminal_registry;
pub mod terminal_tab_service;
pub mod terminal_tabs;
pub mod tmux_clipboard;

#[cfg(test)]
pub(crate) mod test_support;
