//! 세션 이름과 관리 worktree 경로 규칙. 셸 인용과 Node `path.posix` 정규화도 여기 둔다.

use kanvibe_domain::project::{posix_basename, posix_dirname};
use sha2::{Digest, Sha256};

/// zellij 세션 이름을 소켓 경로 108바이트 제한에 맞게 자른다
const ZELLIJ_SESSION_NAME_MAX_LENGTH: usize = 60;
const HERDR_SESSION_NAME_MAX_LENGTH: usize = 40;
const HERDR_SESSION_PREFIX_LENGTH: usize = 23;
const HERDR_SESSION_HASH_LENGTH: usize = 16;

/// branchName을 세션 이름으로 바꾼다. `/`는 `-`로 치환한다
pub fn format_session_name(project_name: &str, branch_name: &str) -> String {
    format!("{project_name}-{branch_name}").replace('/', "-")
}

pub fn format_project_branch_session_name(project_path: &str, branch_name: &str) -> String {
    format_session_name(posix_basename(project_path), branch_name)
}

pub(crate) fn build_managed_worktree_base_path(project_path: &str) -> String {
    let project_name = posix_basename(project_path);
    posix_join(
        posix_dirname(project_path),
        &format!("{project_name}__worktrees"),
    )
}

pub fn build_managed_worktree_path(project_path: &str, branch_name: &str) -> String {
    posix_join(
        &build_managed_worktree_base_path(project_path),
        &branch_name.replace('/', "-"),
    )
}

/// 60자를 넘는 이름은 앞 60자만 남긴다. 길이는 JS 문자열처럼 UTF-16 단위로 센다
pub fn sanitize_zellij_session_name(session_name: &str) -> String {
    let utf16: Vec<u16> = session_name.encode_utf16().collect();
    if utf16.len() <= ZELLIJ_SESSION_NAME_MAX_LENGTH {
        return session_name.to_string();
    }
    String::from_utf16_lossy(&utf16[..ZELLIJ_SESSION_NAME_MAX_LENGTH])
}

fn is_herdr_session_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
}

/// Herdr 세션 이름 규칙을 벗어나면 안전한 앞부분과 sha256 해시 16자로 바꾼다
pub fn sanitize_herdr_session_name(session_name: &str) -> String {
    let is_valid_name = (1..=HERDR_SESSION_NAME_MAX_LENGTH).contains(&session_name.len())
        && session_name.chars().all(is_herdr_session_name_character)
        && session_name != "."
        && session_name != "..";
    if is_valid_name {
        return session_name.to_string();
    }
    let digest = hex::encode(Sha256::digest(session_name.as_bytes()));
    let suffix = &digest[..HERDR_SESSION_HASH_LENGTH];
    // JS `replace(/[^a-zA-Z0-9._-]/g, "-")`는 UTF-16 단위마다 `-` 하나를 넣는다
    let replaced: String = session_name
        .chars()
        .flat_map(
            |character| match is_herdr_session_name_character(character) {
                true => vec![character],
                false => vec!['-'; character.len_utf16()],
            },
        )
        .collect();
    let prefix: String = replaced.chars().take(HERDR_SESSION_PREFIX_LENGTH).collect();
    format!("{prefix}-{suffix}")
}

/// POSIX 셸 작은따옴표 인용. 안의 작은따옴표는 `'"'"'`로 바꾼다
pub fn quote_for_posix_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Node `path.posix.normalize`. `.`·`..`·중복 `/`를 정리하고 끝 `/`는 유지한다
pub(crate) fn posix_normalize(path: &str) -> String {
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
            ".." if !is_absolute => segments.push(".."),
            ".." => {}
            other => segments.push(other),
        }
    }
    let mut normalized = segments.join("/");
    if normalized.is_empty() {
        return match (is_absolute, has_trailing_separator) {
            (true, _) => "/".into(),
            (false, true) => "./".into(),
            (false, false) => ".".into(),
        };
    }
    if has_trailing_separator {
        normalized.push('/');
    }
    if is_absolute {
        normalized.insert(0, '/');
    }
    normalized
}

/// Node `path.posix.join`. 빈 조각은 빼고 이은 뒤 정규화한다
pub(crate) fn posix_join(base: &str, child: &str) -> String {
    let joined: Vec<&str> = [base, child]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    if joined.is_empty() {
        return ".".into();
    }
    posix_normalize(&joined.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_project_and_branch_with_slashes_replaced() {
        assert_eq!(
            format_session_name("kanvibe", "feat/something"),
            "kanvibe-feat-something"
        );
    }

    #[test]
    fn handles_multiple_slashes_in_branch_name() {
        assert_eq!(
            format_session_name("kanvibe", "feat/ui/button"),
            "kanvibe-feat-ui-button"
        );
    }

    #[test]
    fn handles_branch_name_without_slashes() {
        assert_eq!(format_session_name("kanvibe", "main"), "kanvibe-main");
    }

    #[test]
    fn replaces_slashes_in_project_name_as_well() {
        assert_eq!(
            format_session_name("parent/project", "feat/test"),
            "parent-project-feat-test"
        );
    }

    #[test]
    fn derives_session_name_from_project_path_and_branch_name() {
        assert_eq!(
            format_project_branch_session_name("/workspace/kanvibe", "feat/session-cleanup"),
            "kanvibe-feat-session-cleanup"
        );
    }

    #[test]
    fn zellij_name_within_limit_is_unchanged() {
        assert_eq!(sanitize_zellij_session_name("feat-login"), "feat-login");
    }

    #[test]
    fn zellij_name_over_sixty_characters_is_truncated() {
        let result = sanitize_zellij_session_name(&"a".repeat(80));

        assert_eq!(result, "a".repeat(60));
    }

    #[test]
    fn zellij_name_of_exactly_sixty_characters_is_unchanged() {
        let exact_name = "x".repeat(60);

        assert_eq!(sanitize_zellij_session_name(&exact_name), exact_name);
    }

    #[test]
    fn herdr_names_outside_the_rule_get_a_hash_suffix() {
        assert_eq!(
            sanitize_herdr_session_name("project-feature-login"),
            "project-feature-login"
        );
        let sanitized = sanitize_herdr_session_name("a/b c");
        assert!(sanitized.starts_with("a-b-c-"));
        assert_eq!(sanitized.len(), "a-b-c-".len() + 16);
    }

    #[test]
    fn posix_paths_follow_node_semantics() {
        assert_eq!(posix_normalize("/a//b/../c/"), "/a/c/");
        assert_eq!(posix_normalize("a/../.."), "..");
        assert_eq!(
            posix_join("/work", "project__worktrees"),
            "/work/project__worktrees"
        );
        assert_eq!(
            build_managed_worktree_path("/work/project", "feature/login"),
            "/work/project__worktrees/feature-login"
        );
        assert_eq!(quote_for_posix_shell("it's"), "'it'\"'\"'s'");
    }
}
