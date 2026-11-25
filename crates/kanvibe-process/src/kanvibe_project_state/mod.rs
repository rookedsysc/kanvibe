//! 워크트리 안 `.kanvibe/` 상태 파일(TS `kanvibeProjectState.ts`).
//! hook이 쓰는 status.json, KanVibe client가 쓰는 project.json·task.json, hook 알림 대상 targets.json을
//! 로컬과 원격 저장소에서 같은 방식으로 읽고 쓴다. 갱신은 저장소 측 잠금을 거쳐 동시 기록이 서로를 지우지 않는다.

#[cfg(test)]
mod concurrency_tests;
mod content;
#[cfg(test)]
mod tests;

pub use content::*;

use kanvibe_domain::task::{TaskPriority, TaskStatus};

use crate::exec::ExecGitOptions;
use crate::host_file_access::{HostFileAccess, HostFileError};

pub const KANVIBE_DIR_NAME: &str = ".kanvibe";
pub const TASK_STATE_FILE_NAME: &str = "status.json";
pub const PROJECT_STATE_FILE_NAME: &str = "project.json";
pub const TARGETS_FILE_NAME: &str = "targets.json";
pub const TASK_DESCRIPTION_FILE_NAME: &str = "task.json";

/// targets.json은 task CLI 자격 증명을 담으므로 소유자만 읽게 한다
const TARGETS_FILE_MODE: u32 = 0o600;

/// Node `path.posix.normalize`. Linux 로컬 경로도 같은 규칙이므로 로컬·원격 모두 이것으로 잇는다
fn normalize_posix_path(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let is_absolute = path.starts_with('/');
    let has_trailing_separator = path.ends_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." if segments.last().is_some_and(|last| *last != "..") => {
                segments.pop();
            }
            ".." if is_absolute => {}
            _ => segments.push(segment),
        }
    }
    let mut normalized = segments.join("/");
    if normalized.is_empty() && !is_absolute {
        normalized.push('.');
    }
    if has_trailing_separator && !segments.is_empty() {
        normalized.push('/');
    }
    if is_absolute {
        normalized.insert(0, '/');
    }
    normalized
}

/// Node `path.join`. 빈 조각은 건너뛰고 이은 뒤 정규화한다
pub(crate) fn join_posix_path(parts: &[&str]) -> String {
    let joined: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|part| !part.is_empty())
        .collect();
    normalize_posix_path(&joined.join("/"))
}

fn state_file_path(repo_path: &str, file_name: &str) -> String {
    join_posix_path(&[repo_path, KANVIBE_DIR_NAME, file_name])
}

pub fn get_kanvibe_task_state_path(repo_path: &str) -> String {
    state_file_path(repo_path, TASK_STATE_FILE_NAME)
}

pub fn get_kanvibe_project_state_path(repo_path: &str) -> String {
    state_file_path(repo_path, PROJECT_STATE_FILE_NAME)
}

pub fn get_kanvibe_targets_path(repo_path: &str) -> String {
    state_file_path(repo_path, TARGETS_FILE_NAME)
}

pub fn get_kanvibe_task_description_path(repo_path: &str) -> String {
    state_file_path(repo_path, TASK_DESCRIPTION_FILE_NAME)
}

/// 다른 기기의 상태를 한 번에 반영하기 위해 함께 읽는 값들.
/// `priority`·`project_priority`의 바깥 `None`은 파일에 기록이 없다는 뜻이다
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KanvibeTaskSyncState {
    pub status: Option<TaskStatus>,
    pub description: Option<KanvibeTaskDescription>,
    pub priority: Option<Option<TaskPriority>>,
    pub project_priority: Option<Option<TaskPriority>>,
    pub session: Option<KanvibeTaskSession>,
}

/// task.json에 쓸 값. 바깥 `None`인 값은 기록하지 않고, `Some(None)`은 null로 기록한다
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KanvibeTaskMetadataUpdate {
    pub description: Option<Option<String>>,
    pub priority: Option<Option<TaskPriority>>,
}

fn default_options() -> ExecGitOptions {
    ExecGitOptions::default()
}

fn strict_options() -> ExecGitOptions {
    ExecGitOptions {
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    }
}

pub fn read_kanvibe_task_state(
    files: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
) -> Result<Option<KanvibeTaskState>, HostFileError> {
    let state_path = get_kanvibe_task_state_path(repo_path);
    let content = files.read_text_file(&state_path, ssh_host, &default_options())?;
    Ok(parse_kanvibe_task_state(&content))
}

/// 다른 KanVibe client가 project.json에 기록한 프로젝트 색상을 읽는다
pub fn read_kanvibe_project_color(
    files: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
) -> Result<Option<String>, HostFileError> {
    let project_state_path = get_kanvibe_project_state_path(repo_path);
    let content = files.read_text_file(&project_state_path, ssh_host, &default_options())?;
    Ok(parse_kanvibe_project_state(&content).and_then(|state| state.project_color))
}

/// 다른 기기가 남긴 task 상태와 설명을 함께 읽는다.
/// 두 값은 늘 같은 시점에 필요하므로 원격 저장소에서도 한 번의 왕복으로 가져온다.
pub fn read_kanvibe_task_sync_state(
    files: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
    project_repo_path: Option<&str>,
) -> Result<KanvibeTaskSyncState, HostFileError> {
    let task_state_path = get_kanvibe_task_state_path(repo_path);
    let task_description_path = get_kanvibe_task_description_path(repo_path);
    let project_state_path = get_kanvibe_project_state_path(project_repo_path.unwrap_or(repo_path));
    let state_paths = [
        task_state_path.clone(),
        task_description_path.clone(),
        project_state_path.clone(),
    ];
    let state_files = files.read_text_files(&state_paths, ssh_host, &default_options())?;
    let read_content = |state_path: &String| {
        state_files
            .get(state_path)
            .map(|file| file.content.clone())
            .unwrap_or_default()
    };
    let task_metadata = parse_kanvibe_task_description(&read_content(&task_description_path));
    let project_state = parse_kanvibe_project_state(&read_content(&project_state_path));
    let task_state = parse_kanvibe_task_state(&read_content(&task_state_path));

    Ok(KanvibeTaskSyncState {
        status: task_state.map(|state| state.status),
        priority: task_metadata
            .as_ref()
            .and_then(|metadata| metadata.priority),
        project_priority: project_state.and_then(|state| state.priority),
        session: task_metadata
            .as_ref()
            .and_then(|metadata| metadata.session.clone()),
        description: task_metadata.filter(|metadata| metadata.description.is_some()),
    })
}

/// 상태 기록은 다른 파일을 읽지도 쓰지도 않으므로 색상·설명 갱신과 서로를 되돌릴 수 없다
pub fn write_kanvibe_task_status(
    files: &HostFileAccess,
    repo_path: &str,
    status: TaskStatus,
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    let content = build_kanvibe_task_state_content(status);
    let state_path = get_kanvibe_task_state_path(repo_path);
    files.write_text_file(&state_path, &content, ssh_host, &default_options())
}

/// 세션 정보는 다른 경로가 기록하므로 설명과 priority를 쓰는 동안 그대로 남긴다
pub fn write_kanvibe_task_metadata(
    files: &HostFileAccess,
    repo_path: &str,
    metadata: &KanvibeTaskMetadataUpdate,
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    let description_path = get_kanvibe_task_description_path(repo_path);
    let mut update = |content: &str| {
        let session = parse_kanvibe_task_description(content).and_then(|existing| existing.session);
        Ok(build_kanvibe_task_description_content(
            metadata.description.clone(),
            metadata.priority,
            session,
        ))
    };
    files.update_text_file(
        &description_path,
        ssh_host,
        None,
        &default_options(),
        &mut update,
    )
}

/// task 세션 정보를 task.json에 기록한다. 이미 기록된 설명과 priority는 남긴다
pub fn write_kanvibe_task_session(
    files: &HostFileAccess,
    repo_path: &str,
    session: &KanvibeTaskSession,
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    let description_path = get_kanvibe_task_description_path(repo_path);
    let mut update = |content: &str| {
        let existing = parse_kanvibe_task_description(content);
        let description = existing
            .as_ref()
            .and_then(|metadata| metadata.description.clone());
        let priority = existing.as_ref().and_then(|metadata| metadata.priority);
        Ok(build_kanvibe_task_description_content(
            description,
            priority,
            Some(session.clone()),
        ))
    };
    files.update_text_file(
        &description_path,
        ssh_host,
        None,
        &default_options(),
        &mut update,
    )
}

/// 형식이 잘못된 색상은 기록하지 않는다. priority는 그대로 남긴다
pub fn write_kanvibe_project_color(
    files: &HostFileAccess,
    repo_path: &str,
    project_color: &str,
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    let Some(normalized_color) = parse_project_color(project_color) else {
        return Ok(());
    };
    let project_state_path = get_kanvibe_project_state_path(repo_path);
    let mut update = |content: &str| {
        let priority = parse_kanvibe_project_state(content).and_then(|state| state.priority);
        Ok(build_kanvibe_project_state_content(
            Some(normalized_color.clone()),
            priority,
        ))
    };
    files.update_text_file(
        &project_state_path,
        ssh_host,
        None,
        &default_options(),
        &mut update,
    )
}

/// 프로젝트 priority를 기록한다. `None`은 null로 남겨 다른 기기에서도 지워지게 한다. 색상은 그대로 남긴다
pub fn write_kanvibe_project_priority(
    files: &HostFileAccess,
    repo_path: &str,
    priority: Option<TaskPriority>,
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    let project_state_path = get_kanvibe_project_state_path(repo_path);
    let mut update = |content: &str| {
        let project_color =
            parse_kanvibe_project_state(content).and_then(|state| state.project_color);
        Ok(build_kanvibe_project_state_content(
            project_color,
            Some(priority),
        ))
    };
    files.update_text_file(
        &project_state_path,
        ssh_host,
        None,
        &default_options(),
        &mut update,
    )
}

/// 아직 색상 파일이 없는 저장소에만 씨앗 색상을 기록한다. 이미 파일이 있으면 그 값이 권위이므로 기록하지 않는다
pub fn write_kanvibe_project_color_if_absent(
    files: &HostFileAccess,
    repo_path: &str,
    project_color: &str,
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    let Some(normalized_color) = parse_project_color(project_color) else {
        return Ok(());
    };
    let content = build_kanvibe_project_state_content(Some(normalized_color), None);
    let project_state_path = get_kanvibe_project_state_path(repo_path);
    files.write_text_file_if_absent(&project_state_path, &content, ssh_host)
}

/// `.kanvibe`나 그 안의 파일이 심볼릭 링크면 기록을 거부한다.
/// 브랜치가 링크를 추적하면 데스크탑이 worktree 밖의 파일을 덮어쓰거나 권한을 바꾸고, 토큰을 그쪽에 남기게 된다.
pub fn assert_kanvibe_state_paths_are_not_symlinks(
    files: &HostFileAccess,
    repo_path: &str,
    file_names: &[&str],
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    let state_directory = join_posix_path(&[repo_path, KANVIBE_DIR_NAME]);
    let mut state_paths = vec![state_directory.clone()];
    state_paths.extend(
        file_names
            .iter()
            .map(|file_name| join_posix_path(&[&state_directory, file_name])),
    );
    let symbolic_link_path =
        files.find_symbolic_link_path(&state_paths, ssh_host, &strict_options())?;
    match symbolic_link_path {
        Some(symbolic_link_path) => Err(HostFileError::Message(format!(
            "KanVibe 상태 경로가 심볼릭 링크라 기록하지 않습니다: {symbolic_link_path}"
        ))),
        None => Ok(()),
    }
}

/// hook 알림 대상을 client 단위로 등록한다. targets.json은 소유자 전용 권한으로 기록한다
pub fn upsert_kanvibe_hook_target(
    files: &HostFileAccess,
    repo_path: &str,
    target: &KanvibeHookTarget,
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    assert_kanvibe_state_paths_are_not_symlinks(files, repo_path, &[], ssh_host)?;
    let targets_path = get_kanvibe_targets_path(repo_path);
    let mut update = |content: &str| {
        let current_targets = parse_kanvibe_targets(content).targets;
        Ok(build_kanvibe_targets_content(&upsert_kanvibe_target(
            current_targets,
            target,
        )))
    };
    files.update_text_file(
        &targets_path,
        ssh_host,
        Some(TARGETS_FILE_MODE),
        &strict_options(),
        &mut update,
    )
}
