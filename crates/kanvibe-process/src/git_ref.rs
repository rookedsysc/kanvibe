//! 네트워크에서 받은 브랜치 이름으로 받아들일 모양(TS `lib/gitRef.ts`).
//!
//! 이 값은 worktree 생성 명령의 셸 문자열과 worktree 경로에 그대로 들어간다. 큰따옴표 안이라도 셸은 `$(...)`와 백틱을
//! 펼치므로, 모바일·CLI처럼 네트워크에서 오는 값은 여기서 막지 않으면 요청자가 이 머신에서 임의의 명령을 돌리고
//! 프로젝트 밖 경로를 짚을 수 있다. git이 받아 주는 이름 중 셸과 경로에 뜻이 없는 문자만 통과시킨다.

use std::sync::LazyLock;

use regex::Regex;

static SAFE_GIT_REF_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[A-Za-z0-9][A-Za-z0-9._/-]*$").expect("유효한 정규식"));

pub fn is_safe_git_ref(value: &str) -> bool {
    SAFE_GIT_REF_PATTERN.is_match(value) && !value.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_branch_names_and_rejects_shell_or_path_tricks() {
        assert!(is_safe_git_ref("feat/login-page_2.1"));
        assert!(!is_safe_git_ref("-delete"));
        assert!(!is_safe_git_ref("feat/$(id)"));
        assert!(!is_safe_git_ref("feat/`id`"));
        assert!(!is_safe_git_ref("../escape"));
        assert!(!is_safe_git_ref("feat/a..b"));
        assert!(!is_safe_git_ref(""));
    }
}
