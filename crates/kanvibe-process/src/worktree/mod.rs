//! git worktree와 세션의 생성·삭제, 세션 이름 규칙(TS `worktree.ts`).
//! Herdr 조작은 `HerdrTaskWorkspaces`로, pane 레이아웃 조회는 `PaneLayoutLookup`으로 받아 터미널·DB 계층에 묶이지 않는다.
//! tmux `set-clipboard` 사용자 설정 감지는 CLAUDE.md 예외 항목에 따라 터미널 계층이 맡는다.

mod cleanup;
mod naming;
mod sessions;
#[cfg(test)]
mod tests;
mod tmux;
mod zellij;

pub use cleanup::{ResourceCleanupOptions, remove_worktree_and_branch};
pub use naming::{
    build_managed_worktree_path, format_project_branch_session_name, format_session_name,
    quote_for_posix_shell, sanitize_herdr_session_name, sanitize_zellij_session_name,
};
pub use sessions::{
    HerdrTaskWorkspaces, PaneLayoutLookup, WorktreeSession, create_session_without_worktree,
    create_worktree_with_session, is_session_alive, remove_session_only,
};
pub use tmux::{
    PaneLayoutSelection, TmuxSessionBootstrapOptions, build_tmux_hide_status_bar_command,
    build_tmux_pane_layout_arguments, build_tmux_session_bootstrap_command,
};
pub use zellij::{
    ZELLIJ_LAYOUT_FILENAME, build_zellij_alive_session_check_command, generate_zellij_layout_kdl,
    parse_alive_zellij_session_names,
};

/// 정리 실패. 메시지는 TS 오류 문자열과 같다
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorktreeCleanupError {
    #[error("브랜치 정리 실패: {0}")]
    Branch(String),
    #[error("세션 정리 실패: {0}")]
    Session(String),
}
