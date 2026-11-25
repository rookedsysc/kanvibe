//! 작업 변경 집계. 집계는 작업이 바뀌지 않아도 갱신되는 파생값이라 `task_diff_stats` 테이블에 따로 둔다.
//! 작업 행에 쓰면 `updated_at`이 밀려 그 값으로 정렬하는 보드가 갱신마다 카드를 재배열한다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDiffStats {
    pub file_count: i64,
    pub additions: i64,
    pub deletions: i64,
}

/// `task_diff_stats` 테이블의 행
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDiffStatsRecord {
    pub task_id: String,
    pub stats: TaskDiffStats,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// diff 화면이 보여 주는 변경 파일 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    pub status: DiffFileStatus,
    pub additions: i64,
    pub deletions: i64,
}

/// 변경 파일 목록을 카드와 패널이 함께 쓰는 집계로 접는다. 두 화면이 같은 숫자를 보여야 해서 셈을 한곳에 둔다.
pub fn summarize_diff_files(files: &[DiffFile]) -> TaskDiffStats {
    files
        .iter()
        .fold(TaskDiffStats::default(), |stats, file| TaskDiffStats {
            file_count: stats.file_count + 1,
            additions: stats.additions + file.additions,
            deletions: stats.deletions + file.deletions,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff_file(path: &str, status: DiffFileStatus, additions: i64, deletions: i64) -> DiffFile {
        DiffFile {
            path: path.into(),
            original_path: None,
            status,
            additions,
            deletions,
        }
    }

    // parity: TS-shared-taskDiffStats-91b280
    #[test]
    fn sums_file_count_additions_and_deletions() {
        let files = [
            diff_file("src/a.ts", DiffFileStatus::Modified, 12, 3),
            diff_file("src/b.ts", DiffFileStatus::Added, 88, 0),
            diff_file("scripts/old.cjs", DiffFileStatus::Deleted, 0, 41),
        ];

        assert_eq!(
            summarize_diff_files(&files),
            TaskDiffStats {
                file_count: 3,
                additions: 100,
                deletions: 44
            }
        );
    }

    // parity: TS-shared-taskDiffStats-b73110
    #[test]
    fn empty_change_list_is_all_zero() {
        assert_eq!(summarize_diff_files(&[]), TaskDiffStats::default());
    }

    // parity: TS-shared-taskDiffStats-3aaf1e
    #[test]
    fn files_without_line_counts_still_count_as_files() {
        let files = [diff_file("assets/logo.png", DiffFileStatus::Added, 0, 0)];

        assert_eq!(
            summarize_diff_files(&files),
            TaskDiffStats {
                file_count: 1,
                additions: 0,
                deletions: 0
            }
        );
    }
}
