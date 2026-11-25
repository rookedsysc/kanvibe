//! worktree 경계 판정에 쓰는 POSIX 경로 계산(Node `path.posix.resolve`·`relative`와 같은 결과).

/// `.`과 `..`을 풀고 중복 구분자를 없앤 절대 경로
fn normalize_absolute(path: &str) -> String {
    let mut components: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            name => components.push(name),
        }
    }
    format!("/{}", components.join("/"))
}

pub(super) fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
}

pub(super) fn resolve(base: &str, path: &str) -> String {
    if is_absolute(path) {
        return normalize_absolute(path);
    }
    normalize_absolute(&format!("{base}/{path}"))
}

/// `from`에서 `to`로 가는 상대 경로. 같은 경로면 빈 문자열이다
pub(super) fn relative(from: &str, to: &str) -> String {
    let from = normalize_absolute(from);
    let to = normalize_absolute(to);
    let from_parts: Vec<&str> = from.split('/').filter(|part| !part.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').filter(|part| !part.is_empty()).collect();
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<&str> = vec![".."; from_parts.len() - common];
    parts.extend(&to_parts[common..]);
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_node_posix_resolve_and_relative() {
        assert_eq!(resolve("/repo", "a/../b/./c"), "/repo/b/c");
        assert_eq!(resolve("/repo", "/etc/passwd"), "/etc/passwd");
        assert_eq!(relative("/repo", "/repo/docs/plan.md"), "docs/plan.md");
        assert_eq!(relative("/repo", "/repo"), "");
        assert_eq!(relative("/repo", "/repo-sibling/x"), "../repo-sibling/x");
        assert_eq!(relative("/repo/a", "/"), "../..");
    }
}
