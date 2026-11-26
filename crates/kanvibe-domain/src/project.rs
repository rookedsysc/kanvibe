//! `projects` 테이블의 행과 표시 이름 규칙.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// 등록된 git 저장소. `ssh_host`가 있으면 SSH 너머의 원격 저장소다.
/// 표시 이름은 같은 PC(`ssh_host`) 안에서만 유일하다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    pub default_branch: String,
    pub ssh_host: Option<String>,
    pub is_worktree: bool,
    /// `#rrggbb`
    pub color: Option<String>,
    /// 오프라인에서도 보이도록 data URL로 보관한 GitHub repo/org 아이콘
    pub icon_data_url: Option<String>,
    pub created_at: String,
}

/// 같은 PC 안에서 이름과 상위 폴더까지 모두 겹쳐 표시 이름을 구분할 수 없을 때 알리는 문구
pub const DUPLICATE_PROJECT_NAME_ERROR: &str =
    "같은 PC에 이름과 상위 폴더가 모두 같은 프로젝트가 이미 있습니다.";

/// 같은 PC 안에서 이름이 겹칠 때만 상위 폴더를 덧붙여 표시 이름을 구분한다.
/// 확정한 이름은 `names_on_same_host`에 더하고, 상위 폴더까지 같아 구분할 수 없으면 `None`을 돌려준다.
pub fn resolve_unique_project_name(
    preferred_name: &str,
    repo_path: &str,
    names_on_same_host: &mut HashSet<String>,
) -> Option<String> {
    let base_name = posix_basename(repo_path);
    let parent_name = posix_basename(posix_dirname(repo_path));
    let combined_name = format!("{parent_name}/{base_name}");
    let mut candidates = vec![preferred_name.to_string()];

    if !base_name.is_empty() && base_name != preferred_name {
        candidates.push(base_name.to_string());
    }
    if combined_name != preferred_name {
        candidates.push(combined_name);
    }

    let unused_name = candidates
        .into_iter()
        .find(|candidate| !names_on_same_host.contains(candidate))?;
    names_on_same_host.insert(unused_name.clone());
    Some(unused_name)
}

/// Node `path.posix.basename`과 같게 끝의 `/`를 무시하고 마지막 경로 조각을 돌려준다.
pub fn posix_basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(separator_index) => &trimmed[separator_index + 1..],
        None => trimmed,
    }
}

/// Node `path.posix.dirname`과 같게 마지막 조각을 뺀 부모 경로를 돌려준다.
pub fn posix_dirname(path: &str) -> &str {
    if path.is_empty() {
        return ".";
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    match trimmed.rfind('/') {
        None => ".",
        Some(separator_index) => {
            let parent = trimmed[..separator_index].trim_end_matches('/');
            if parent.is_empty() { "/" } else { parent }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_path_helpers_follow_node_semantics() {
        assert_eq!(posix_basename("/home/tester/kanvibe/"), "kanvibe");
        assert_eq!(posix_basename("/"), "");
        assert_eq!(posix_dirname("/home/tester/kanvibe"), "/home/tester");
        assert_eq!(posix_dirname("/kanvibe"), "/");
        assert_eq!(posix_dirname("kanvibe"), ".");
        assert_eq!(posix_dirname("/"), "/");
    }

    #[test]
    fn duplicate_names_on_the_same_host_get_the_parent_folder() {
        let mut names = HashSet::new();

        let first =
            resolve_unique_project_name("kanvibe", "/home/tester/Documents/kanvibe", &mut names);
        let second =
            resolve_unique_project_name("kanvibe", "/home/tester/work/kanvibe", &mut names);
        let third = resolve_unique_project_name("kanvibe", "/home/other/work/kanvibe", &mut names);

        assert_eq!(first.as_deref(), Some("kanvibe"));
        assert_eq!(second.as_deref(), Some("work/kanvibe"));
        assert_eq!(third, None);
    }
}
