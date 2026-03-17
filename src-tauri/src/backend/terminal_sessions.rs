use std::process::Command;

use serde::{Deserialize, Serialize};

pub const MODULE_NAME: &str = "terminalSessions";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionSummary {
    pub session_type: String,
    pub name: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminalSessionRequest {
    pub session_type: String,
    pub session_name: String,
    pub working_directory: String,
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
pub fn list_terminal_sessions(session_type: String) -> Result<Vec<TerminalSessionSummary>, String> {
    let command = match session_type.as_str() {
        "zellij" => "zellij list-sessions --no-formatting",
        _ => "tmux list-sessions -F '#S'",
    };

    let output = run_shell(command)?;
    if output.is_empty() {
        return Ok(Vec::new());
    }

    Ok(output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| TerminalSessionSummary {
            session_type: session_type.clone(),
            name: line.trim().to_string(),
        })
        .collect())
}

#[tauri::command]
pub fn create_terminal_session(
    request: CreateTerminalSessionRequest,
) -> Result<TerminalSessionSummary, String> {
    match request.session_type.as_str() {
        "zellij" => {
            run_shell(&format!(
                "zellij --session \"{}\" --new-session-with-cwd \"{}\" -d",
                request.session_name, request.working_directory
            ))?;
        }
        _ => {
            run_shell(&format!(
                "tmux new-session -d -s \"{}\" -c \"{}\"",
                request.session_name, request.working_directory
            ))?;
        }
    }

    Ok(TerminalSessionSummary {
        session_type: request.session_type,
        name: request.session_name,
    })
}
