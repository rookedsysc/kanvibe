//! 미리보기용 worktree 파일 읽기(TS `readTaskWorkspaceFile`). 실패를 빈 문자열로 삼키지 않고 사유를 돌려준다.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;

use kanvibe_process::exec::ExecGitOptions;
use kanvibe_process::host_file_access::quote_shell_argument;
use serde::Serialize;

use super::{DiffService, paths, validate_file_path};

/// 미리보기 한 번에 싣는 파일 크기의 상한. 넘는 파일은 내용을 읽기 전에 거절한다
const MAX_TASK_WORKSPACE_FILE_BYTES: u64 = 1024 * 1024;
const REMOTE_FILE_MISSING_MARKER: &str = "__KANVIBE_FILE_MISSING__";
const REMOTE_FILE_TOO_LARGE_MARKER: &str = "__KANVIBE_FILE_TOO_LARGE__";
const REMOTE_FILE_CONTENT_MARKER: &str = "__KANVIBE_FILE_CONTENT__";

/// 미리보기가 파일을 열지 못한 이유. 화면은 사유별 안내를 낸다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "kebab-case")]
pub enum TaskWorkspaceFileFailure {
    #[error("outside-worktree")]
    OutsideWorktree,
    #[error("not-found")]
    NotFound,
    #[error("too-large")]
    TooLarge,
    #[error("unreadable")]
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskWorkspaceFile {
    /// worktree 기준 상대 경로
    pub path: String,
    pub content: String,
}

/// 이름 붙은 파이프를 열다 멈추지 않도록 막힘 없이 연다
fn open_without_blocking(resolved_path: &str) -> Result<File, TaskWorkspaceFileFailure> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(resolved_path)
        .map_err(|error| match error.raw_os_error() {
            Some(libc::ENOENT | libc::ENOTDIR) => TaskWorkspaceFileFailure::NotFound,
            _ => TaskWorkspaceFileFailure::Unreadable,
        })
}

fn read_local_workspace_file(resolved_path: &str) -> Result<String, TaskWorkspaceFileFailure> {
    let mut file = open_without_blocking(resolved_path)?;
    let metadata = file
        .metadata()
        .map_err(|_| TaskWorkspaceFileFailure::Unreadable)?;
    if !metadata.is_file() {
        return Err(TaskWorkspaceFileFailure::NotFound);
    }
    if metadata.len() > MAX_TASK_WORKSPACE_FILE_BYTES {
        return Err(TaskWorkspaceFileFailure::TooLarge);
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| TaskWorkspaceFileFailure::Unreadable)?;
    // 크기를 잰 뒤에 파일이 커졌을 수 있다
    if bytes.len() as u64 > MAX_TASK_WORKSPACE_FILE_BYTES {
        return Err(TaskWorkspaceFileFailure::TooLarge);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn build_remote_read_command(resolved_path: &str) -> String {
    let quoted_path = quote_shell_argument(resolved_path);
    [
        format!("if ! test -f {quoted_path}; then printf '%s' {REMOTE_FILE_MISSING_MARKER};"),
        format!("elif test \"$(wc -c < {quoted_path})\" -gt {MAX_TASK_WORKSPACE_FILE_BYTES}; then printf '%s' {REMOTE_FILE_TOO_LARGE_MARKER};"),
        format!("else printf '%s\\n' {REMOTE_FILE_CONTENT_MARKER} && cat {quoted_path}; fi"),
    ]
    .join(" ")
}

/// 절대 경로로 온 요청을 worktree 기준 상대 경로로 바꾼다. AI 기록은 파일을 대개 절대 경로로 남긴다
fn to_worktree_relative_path(worktree_path: &str, file_path: &str) -> String {
    if paths::is_absolute(file_path) {
        paths::relative(worktree_path, file_path)
    } else {
        file_path.to_string()
    }
}

impl DiffService {
    /// 원격에서는 크기를 먼저 재고 상한 안일 때만 내용을 보내게 해서, 큰 파일이 SSH를 타고 넘어오지 않게 한다
    fn read_remote_workspace_file(
        &self,
        resolved_path: &str,
        ssh_host: &str,
    ) -> Result<String, TaskWorkspaceFileFailure> {
        let options = ExecGitOptions {
            max_buffer_bytes: Some(MAX_TASK_WORKSPACE_FILE_BYTES as usize + 4096),
            ..ExecGitOptions::default()
        };
        let command = build_remote_read_command(resolved_path);
        let output = self
            .exec(&command, Some(ssh_host), &options)
            .map_err(|error| {
                eprintln!("미리보기 파일 읽기 실패: {error}");
                TaskWorkspaceFileFailure::Unreadable
            })?;
        match output.as_str() {
            REMOTE_FILE_MISSING_MARKER => Err(TaskWorkspaceFileFailure::NotFound),
            REMOTE_FILE_TOO_LARGE_MARKER => Err(TaskWorkspaceFileFailure::TooLarge),
            REMOTE_FILE_CONTENT_MARKER => Ok(String::new()),
            _ => output
                .strip_prefix(&format!("{REMOTE_FILE_CONTENT_MARKER}\n"))
                .map(str::to_string)
                .ok_or(TaskWorkspaceFileFailure::Unreadable),
        }
    }

    /// 미리보기에 쓸 worktree 안 파일 하나를 읽는다. 태스크나 worktree가 없는 경우도 파일을 찾지 못한 것으로 본다
    pub fn read_task_workspace_file(
        &self,
        task_id: &str,
        file_path: &str,
    ) -> Result<TaskWorkspaceFile, TaskWorkspaceFileFailure> {
        let worktree = self
            .task_worktree(task_id)
            .map_err(|_| TaskWorkspaceFileFailure::NotFound)?;
        let relative_request = to_worktree_relative_path(&worktree.worktree_path, file_path);
        let resolved_path = validate_file_path(&worktree.worktree_path, &relative_request)
            .map_err(|_| TaskWorkspaceFileFailure::OutsideWorktree)?;
        let content = match worktree.ssh_host.as_deref() {
            Some(ssh_host) => self.read_remote_workspace_file(&resolved_path, ssh_host)?,
            None => read_local_workspace_file(&resolved_path)?,
        };
        Ok(TaskWorkspaceFile {
            path: paths::relative(&worktree.worktree_path, &resolved_path),
            content,
        })
    }
}
