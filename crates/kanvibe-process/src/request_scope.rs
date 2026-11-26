//! 네트워크 요청이 어느 task를 건드릴 수 있는지 가르는 순수 판정.
//! TS에서는 `mobilePairing.parseBearerToken`과 `hookTunnelService.isTaskReachableFromTunnel`에 있었다.
//! task CLI(이 crate), hook 상태 서비스(kanvibe-services), 서버 crate가 모두 이 함수를 쓰므로 가장 아래 crate인 여기에 둔다.

use std::collections::HashSet;

use kanvibe_domain::project::Project;
use kanvibe_domain::task::KanbanTask;

/// `Authorization: Bearer <token>`의 토큰. 스킴은 대소문자를 가리지 않고, 토큰은 공백 없는 한 덩어리여야 한다
pub fn parse_bearer_token(authorization_header: Option<&str>) -> Option<&str> {
    let header = authorization_header?;
    let scheme_length = "Bearer".len();
    let scheme = header.get(..scheme_length)?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    let rest = &header[scheme_length..];
    let token = rest.trim_start_matches(char::is_whitespace);
    let has_separator = token.len() < rest.len();
    let is_single_token = !token.is_empty() && !token.contains(char::is_whitespace);
    (has_separator && is_single_token).then_some(token)
}

/// 터널로 들어온 요청이면 task(없으면 프로젝트)의 호스트가 그 터널이 맡은 호스트일 때만 받는다.
/// 터널이 아닌 요청(`None`)은 모든 task에 닿는다
pub fn is_task_reachable_from_tunnel(
    task: &KanbanTask,
    project: Option<&Project>,
    source_ssh_hosts: Option<&HashSet<String>>,
) -> bool {
    let Some(source_ssh_hosts) = source_ssh_hosts else {
        return true;
    };
    let task_host = task
        .ssh_host
        .as_ref()
        .or_else(|| project.and_then(|project| project.ssh_host.as_ref()));
    task_host.is_some_and(|host| source_ssh_hosts.contains(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_tokens_need_the_scheme_a_separator_and_one_token() {
        assert_eq!(parse_bearer_token(Some("Bearer abc")), Some("abc"));
        assert_eq!(parse_bearer_token(Some("bearer   abc")), Some("abc"));
        assert_eq!(parse_bearer_token(Some("Bearerabc")), None);
        assert_eq!(parse_bearer_token(Some("Bearer abc def")), None);
        assert_eq!(parse_bearer_token(Some("abc")), None);
        assert_eq!(parse_bearer_token(None), None);
    }
}
