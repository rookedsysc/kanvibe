use std::{path::Path, process::Command};

use serde::{Deserialize, Serialize};

pub const MODULE_NAME: &str = "worktree";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeToolStatus {
    pub git_available: bool,
    pub tmux_available: bool,
    pub zellij_available: bool,
    pub ssh_available: bool,
    pub gh_available: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSummary {
    pub path: String,
    pub branch: String,
    pub is_bare: bool,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorktreeRequest {
    pub project_path: String,
    pub branch_name: String,
    pub base_branch: String,
    pub session_type: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorktreeResponse {
    pub worktree_path: String,
    pub session_name: String,
}

fn command_exists(binary: &str) -> bool {
    Command::new("sh")
        .args(["-lc", &format!("command -v {binary} >/dev/null 2>&1")])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn run_shell(command: &str) -> Result<String, String> {
    let output = Command::new("sh")
        .args(["-lc", command])
        .output()
        .map_err(|error| format!("shell command failed to start: {error}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[tauri::command]
pub fn get_runtime_tool_status() -> RuntimeToolStatus {
    RuntimeToolStatus {
        git_available: command_exists("git"),
        tmux_available: command_exists("tmux"),
        zellij_available: command_exists("zellij"),
        ssh_available: command_exists("ssh"),
        gh_available: command_exists("gh"),
    }
}

#[tauri::command]
pub fn list_worktrees(project_path: String) -> Result<Vec<WorktreeSummary>, String> {
    let output = run_shell(&format!(
        "git -C \"{project_path}\" worktree list --porcelain"
    ))?;

    if output.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut worktrees = Vec::new();
    for block in output.split("\n\n") {
        let mut path = String::new();
        let mut branch = String::new();
        let mut is_bare = false;

        for line in block.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = value.to_string();
            } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
                branch = value.to_string();
            } else if line == "bare" {
                is_bare = true;
            }
        }

        if !path.is_empty() {
            worktrees.push(WorktreeSummary {
                path,
                branch,
                is_bare,
            });
        }
    }

    Ok(worktrees)
}

#[tauri::command]
pub fn create_worktree_session(
    request: CreateWorktreeRequest,
) -> Result<CreateWorktreeResponse, String> {
    let project_name = Path::new(&request.project_path)
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "project name could not be resolved".to_string())?;
    let worktree_base = Path::new(&request.project_path)
        .parent()
        .ok_or_else(|| "project parent path could not be resolved".to_string())?
        .join(format!("{project_name}__worktrees"));
    let branch_slug = request.branch_name.replace('/', "-");
    let worktree_path = worktree_base.join(branch_slug);
    let session_name = format!("{project_name}-{}", request.branch_name).replace('/', "-");

    run_shell(&format!(
        "mkdir -p \"{}\" && git -C \"{}\" worktree add \"{}\" -b \"{}\" \"{}\"",
        worktree_base.to_string_lossy(),
        request.project_path,
        worktree_path.to_string_lossy(),
        request.branch_name,
        request.base_branch,
    ))?;

    match request.session_type.as_str() {
        "zellij" => {
            run_shell(&format!(
                "zellij --session \"{}\" --new-session-with-cwd \"{}\" -d",
                session_name,
                worktree_path.to_string_lossy()
            ))?;
        }
        _ => {
            run_shell(&format!(
                "tmux new-session -d -s \"{}\" -c \"{}\"",
                session_name,
                worktree_path.to_string_lossy()
            ))?;
        }
    }

    Ok(CreateWorktreeResponse {
        worktree_path: worktree_path.to_string_lossy().to_string(),
        session_name,
    })
}
