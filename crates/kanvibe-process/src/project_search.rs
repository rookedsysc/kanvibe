//! 프로젝트 퍼지 검색(TS `utils/fuzzySearch.ts`, `utils/projectSearch.ts`).

use kanvibe_domain::project::Project;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub path: String,
    pub score: i64,
    /// 매칭된 문자의 위치(소문자로 바꾼 대상의 문자 단위)
    pub matched_indices: Vec<usize>,
}

/// 연속 매칭 보너스와 마지막 `/` 뒤(폴더명) 매칭 가중치를 더한 점수
fn score_matches(target: &str, matched_indices: &[usize]) -> i64 {
    let last_slash = target
        .chars()
        .enumerate()
        .filter(|(_, character)| *character == '/')
        .map(|(index, _)| index)
        .last();
    let mut score = 0;
    for (order, &index) in matched_indices.iter().enumerate() {
        score += match last_slash {
            Some(slash_index) if index <= slash_index => 1,
            _ => 2,
        };
        if order > 0 && index == matched_indices[order - 1] + 1 {
            score += 3;
        }
    }
    score
}

/// fzf 스타일 subsequence fuzzy matching. 검색어의 문자가 순서대로 모두 나와야 일치한다
pub fn fuzzy_match(query: &str, target: &str) -> Option<FuzzyMatch> {
    let lower_query: Vec<char> = query.to_lowercase().chars().collect();
    let mut matched_indices = Vec::new();
    for (index, character) in target.to_lowercase().chars().enumerate() {
        if matched_indices.len() == lower_query.len() {
            break;
        }
        if character == lower_query[matched_indices.len()] {
            matched_indices.push(index);
        }
    }
    if matched_indices.len() != lower_query.len() {
        return None;
    }
    Some(FuzzyMatch {
        path: target.to_string(),
        score: score_matches(target, &matched_indices),
        matched_indices,
    })
}

/// 프로젝트 이름·경로·SSH host 중 하나라도 검색어의 문자 순서와 일치하는지 판별한다
pub fn matches_project_search(project: &Project, query: &str) -> bool {
    let normalized_query = query.trim().to_lowercase();
    if normalized_query.is_empty() {
        return true;
    }
    let ssh_host = project.ssh_host.as_deref().unwrap_or_default();
    [project.name.as_str(), project.repo_path.as_str(), ssh_host]
        .iter()
        .any(|value| fuzzy_match(&normalized_query, value).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(ssh_host: Option<&str>) -> Project {
        Project {
            id: "project-1".into(),
            name: "kanvibe".into(),
            repo_path: "/code/my-project".into(),
            default_branch: "main".into(),
            ssh_host: ssh_host.map(str::to_string),
            is_worktree: false,
            color: None,
            icon_data_url: None,
            created_at: String::new(),
        }
    }

    // parity: TS-utils-projectSearch-e106f2
    #[test]
    fn matches_subsequences_in_name_path_or_host() {
        for query in ["kvb", "cd/mpr", "dvbx", " KVB ", "", "   "] {
            assert!(
                matches_project_search(&project(Some("devbox")), query),
                "{query:?}"
            );
        }
    }

    // parity: TS-utils-projectSearch-2d1398
    #[test]
    fn rejects_out_of_order_or_cross_field_queries() {
        for query in ["bvk", "zzz", "kanvibee", "kvbdvbx"] {
            assert!(
                !matches_project_search(&project(Some("devbox")), query),
                "{query:?}"
            );
        }
    }

    // parity: TS-utils-projectSearch-33155b
    #[test]
    fn local_projects_have_no_host_to_match() {
        assert!(!matches_project_search(&project(None), "dvbx"));
    }

    #[test]
    fn folder_name_and_consecutive_matches_score_higher() {
        let matched = fuzzy_match("ab", "x/ab").unwrap();

        assert_eq!(matched.matched_indices, [2, 3]);
        assert_eq!(matched.score, 2 + 2 + 3);
        assert_eq!(fuzzy_match("ab", "a/b").unwrap().score, 1 + 2);
    }
}
