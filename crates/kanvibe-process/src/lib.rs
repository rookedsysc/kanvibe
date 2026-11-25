//! 프로세스·git·파일·SSH를 다루는 코어. 모든 명령은 `exec::ShellCommandRunner::exec_git` 하나를 거친다.
//! 프로세스 전역 환경 변수는 읽기만 하고, 자식 프로세스에는 `shell_environment`로 좁힌 환경만 넘긴다.

pub mod editor;
pub mod exec;
pub mod git_exclude;
pub mod git_operations;
pub mod git_ref;
pub mod github_cli_dependency;
pub mod github_project_icon;
pub mod hooks;
pub mod host_file_access;
pub mod kanvibe_project_state;
pub mod kanvibe_state_store;
pub mod project_color;
pub mod project_color_service;
pub mod project_search;
pub mod remote_session_dependency;
pub mod request_scope;
pub mod shell_environment;
pub mod ssh_config;
pub mod task_cli;
pub mod task_state_service;
pub mod task_target_propagation;
pub mod worktree;
