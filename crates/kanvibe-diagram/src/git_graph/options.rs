//! gitGraph 설정(`config.gitGraph`). 앞머리나 `init` 지시문으로 바꾼 값만 기본값을 덮는다.

use serde_json::{Map, Value};

use crate::directive::DiagramConfig;

#[derive(Clone, Debug, PartialEq)]
pub struct GitGraphOptions {
    pub show_branches: bool,
    pub show_commit_label: bool,
    pub rotate_commit_label: bool,
    pub parallel_commits: bool,
    pub main_branch_name: String,
    pub main_branch_order: f64,
}

impl Default for GitGraphOptions {
    /// Mermaid defaultConfig.gitGraph
    fn default() -> Self {
        Self {
            show_branches: true,
            show_commit_label: true,
            rotate_commit_label: true,
            parallel_commits: false,
            main_branch_name: "main".to_string(),
            main_branch_order: 0.0,
        }
    }
}

impl GitGraphOptions {
    /// 형이 맞지 않는 값은 무시하고 기본값을 둔다
    pub fn from_config(config: &DiagramConfig) -> Self {
        let mut options = Self::default();
        let Some(section) = config.section("gitGraph") else {
            return options;
        };
        let flag = |key: &str| section.get(key).and_then(Value::as_bool);
        options.show_branches = flag("showBranches").unwrap_or(options.show_branches);
        options.show_commit_label = flag("showCommitLabel").unwrap_or(options.show_commit_label);
        options.rotate_commit_label =
            flag("rotateCommitLabel").unwrap_or(options.rotate_commit_label);
        options.parallel_commits = flag("parallelCommits").unwrap_or(options.parallel_commits);
        if let Some(name) = text_value(section, "mainBranchName") {
            options.main_branch_name = name;
        }
        if let Some(order) = section.get("mainBranchOrder").and_then(Value::as_f64) {
            options.main_branch_order = order;
        }
        options
    }
}

fn text_value(section: &Map<String, Value>, key: &str) -> Option<String> {
    section
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::read_config;

    #[test]
    fn 지시문_값만_기본값을_덮는다() {
        let config = read_config(
            "%%{init: {'gitGraph': {'showBranches': false, 'mainBranchName': 'trunk', 'parallelCommits': 'yes'}}}%%\ngitGraph",
        );
        let options = GitGraphOptions::from_config(&config);
        assert!(!options.show_branches);
        assert!(options.show_commit_label);
        assert!(!options.parallel_commits);
        assert_eq!(options.main_branch_name, "trunk");
    }
}
