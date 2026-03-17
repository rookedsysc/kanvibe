use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use rand::RngCore;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use walkdir::WalkDir;

use super::{board_snapshot::BoardProjectSummary, db::open_database};

pub const MODULE_NAME: &str = "projects";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRegistrationResult {
    pub registered: Vec<BoardProjectSummary>,
    pub skipped: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHostConfig {
    pub host: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub private_key_path: String,
}

fn expand_home(path: &str) -> Result<PathBuf, String> {
    if let Some(stripped_path) = path.strip_prefix("~/") {
        let home_directory = env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
        return Ok(PathBuf::from(home_directory).join(stripped_path));
    }

    Ok(PathBuf::from(path))
}

fn validate_git_repo(repo_path: &Path) -> bool {
    Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn detect_default_branch(repo_path: &Path) -> String {
    Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "symbolic-ref",
            "--short",
            "HEAD",
        ])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| "main".to_string())
}

fn generate_id() -> String {
    let mut random_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut random_bytes);
    random_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[tauri::command]
pub fn scan_git_repositories(root_path: String) -> Result<Vec<String>, String> {
    let expanded_root = expand_home(&root_path)?;
    if !expanded_root.exists() {
        return Ok(Vec::new());
    }

    let repositories = WalkDir::new(expanded_root)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_dir() && entry.file_name() == ".git")
        .filter_map(|entry| {
            entry
                .path()
                .parent()
                .map(|path| path.to_string_lossy().to_string())
        })
        .collect::<Vec<_>>();

    Ok(repositories)
}

#[tauri::command]
pub fn list_projects() -> Result<Vec<BoardProjectSummary>, String> {
    let (connection, _) = open_database()?;
    let mut statement = connection
        .prepare(
            "SELECT id, name, repo_path, default_branch, color
             FROM projects
             ORDER BY name ASC",
        )
        .map_err(|error| format!("projects query prepare failed: {error}"))?;

    let rows = statement
        .query_map([], |row| {
            Ok(BoardProjectSummary {
                id: row.get("id")?,
                name: row.get("name")?,
                repo_path: row.get("repo_path")?,
                default_branch: row.get("default_branch")?,
                color: row.get("color")?,
            })
        })
        .map_err(|error| format!("projects query failed: {error}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("projects parse failed: {error}"))
}

#[tauri::command]
pub fn register_projects_from_scan(root_path: String) -> Result<ProjectRegistrationResult, String> {
    let candidates = scan_git_repositories(root_path)?;
    let (connection, _) = open_database()?;
    let mut registered = Vec::new();
    let mut skipped = Vec::new();

    for candidate in candidates {
        let repo_path = PathBuf::from(&candidate);
        if !validate_git_repo(&repo_path) {
            skipped.push(candidate);
            continue;
        }

        let project_name = repo_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("invalid repo path: {candidate}"))?
            .to_string();

        let existing = connection
            .query_row(
                "SELECT id, name, repo_path, default_branch, color FROM projects WHERE repo_path = ?1 OR name = ?2 LIMIT 1",
                params![candidate, project_name],
                |row| {
                    Ok(BoardProjectSummary {
                        id: row.get("id")?,
                        name: row.get("name")?,
                        repo_path: row.get("repo_path")?,
                        default_branch: row.get("default_branch")?,
                        color: row.get("color")?,
                    })
                },
            )
            .optional()
            .map_err(|error| format!("project duplicate query failed: {error}"))?;

        if existing.is_some() {
            skipped.push(candidate);
            continue;
        }

        let project = BoardProjectSummary {
            id: generate_id(),
            name: project_name,
            repo_path: candidate,
            default_branch: detect_default_branch(&repo_path),
            color: None,
        };

        connection
            .execute(
                "INSERT INTO projects (id, name, repo_path, default_branch, is_worktree) VALUES (?1, ?2, ?3, ?4, 0)",
                params![project.id, project.name, project.repo_path, project.default_branch],
            )
            .map_err(|error| format!("project insert failed: {error}"))?;

        registered.push(project);
    }

    Ok(ProjectRegistrationResult {
        registered,
        skipped,
    })
}

#[tauri::command]
pub fn delete_project(project_id: String) -> Result<(), String> {
    let (connection, _) = open_database()?;
    connection
        .execute("DELETE FROM projects WHERE id = ?1", [project_id])
        .map_err(|error| format!("project delete failed: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_available_ssh_hosts() -> Result<Vec<String>, String> {
    Ok(parse_ssh_hosts()?
        .into_iter()
        .map(|host| host.host)
        .collect())
}

#[tauri::command]
pub fn list_git_branches(project_path: String) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .args([
            "-C",
            &project_path,
            "branch",
            "-a",
            "--format=%(refname:short)",
        ])
        .output()
        .map_err(|error| format!("git branch command failed to start: {error}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
        .collect())
}

fn parse_ssh_hosts() -> Result<Vec<SshHostConfig>, String> {
    let home_directory = env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    let config_path = PathBuf::from(&home_directory).join(".ssh").join("config");
    let content = match fs::read_to_string(config_path) {
        Ok(content) => content,
        Err(_) => return Ok(Vec::new()),
    };

    let mut hosts = Vec::new();
    let mut current_host: Option<SshHostConfig> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let key = parts.next().unwrap_or_default().to_lowercase();
        let value = parts.collect::<Vec<_>>().join(" ");

        if key == "host" {
            if let Some(host) = current_host.take() {
                if !host.hostname.is_empty() {
                    hosts.push(host);
                }
            }
            current_host = Some(SshHostConfig {
                host: value.clone(),
                hostname: String::new(),
                port: 22,
                username: "root".to_string(),
                private_key_path: format!("{home_directory}/.ssh/id_rsa"),
            });
            continue;
        }

        if let Some(host) = current_host.as_mut() {
            match key.as_str() {
                "hostname" => host.hostname = value,
                "user" => host.username = value,
                "port" => host.port = value.parse().unwrap_or(22),
                "identityfile" => host.private_key_path = value.replace('~', &home_directory),
                _ => {}
            }
        }
    }

    if let Some(host) = current_host {
        if !host.hostname.is_empty() {
            hosts.push(host);
        }
    }

    Ok(hosts)
}
