//! 작업 상세 화면인지 경로로 판정한다. 같은 키가 화면에 따라 다른 명령이 되므로 모든 입력 경로가 이 판정을 공유한다.

/// `#/<locale>/task/<id>` 뒤에 끝, `?`, `#` 중 하나가 오면 작업 상세 경로다
pub fn is_task_detail_route_url(url: &str) -> bool {
    url.match_indices("#/")
        .any(|(start, _)| is_task_detail_route_at(&url[start + 2..]))
}

fn is_task_detail_route_at(rest: &str) -> bool {
    let Some(locale_end) = rest.find('/') else {
        return false;
    };
    if locale_end == 0 {
        return false;
    }
    let Some(task_part) = rest[locale_end..].strip_prefix("/task/") else {
        return false;
    };
    let task_id_end = task_part.find(['/', '?', '#']).unwrap_or(task_part.len());
    let is_followed_by_boundary = task_part[task_id_end..]
        .chars()
        .next()
        .is_none_or(|boundary| boundary == '?' || boundary == '#');
    task_id_end > 0 && is_followed_by_boundary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_task_detail_routes_in_hash_urls() {
        assert!(is_task_detail_route_url(
            "file:///app/index.html#/ko/task/task-1"
        ));
        assert!(is_task_detail_route_url("#/en/task/task-1?tab=diff"));
        assert!(!is_task_detail_route_url("#/ko/task/task-1/diff"));
        assert!(!is_task_detail_route_url("#/ko/settings"));
        assert!(!is_task_detail_route_url("#//task/task-1"));
        assert!(!is_task_detail_route_url(""));
    }
}
