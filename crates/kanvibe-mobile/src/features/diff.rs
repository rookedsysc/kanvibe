//! 모바일 변경 보기와 파일 편집(TS `mobileDiffService`). 편집할 수 있는 파일은 worktree 안의 상대 경로이면서
//! 지금 변경 목록에 있는 파일뿐이고, 경로 중간에 심볼릭 링크가 있으면 worktree 밖으로 나갈 수 있어 거절한다.

use std::collections::BTreeMap;

use kanvibe_domain::task_diff_stats::{DiffFile, TaskDiffStats};
use kanvibe_services::kanban::TaskWithProject;
use serde::Serialize;
use serde_json::Value;

use super::MobileFeatures;
use super::validation::{DEFAULT_TEXT_MAX_LENGTH, read_mobile_object, read_mobile_text};
use crate::bridge::normalize_posix_path;
use crate::error::{MobileError, MobileResult};

const MAX_EDIT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MobileDiffFileContent {
    pub original: String,
    pub content: String,
}

/// 검증을 통과한 변경 파일. 이름이 바뀐 파일은 원본을 이전 경로에서 읽는다
struct ValidatedDiffPath {
    file_path: String,
    original_path: String,
}

fn is_unsafe_relative_path(file_path: &str) -> bool {
    file_path.starts_with('/')
        || file_path.contains('\\')
        || file_path
            .split('/')
            .any(|part| matches!(part, "" | "." | ".." | ".git"))
}

/// worktree 루트부터 파일까지 지나는 모든 경로
fn build_ancestor_paths(root_path: &str, file_path: &str) -> Vec<String> {
    let components: Vec<&str> = file_path.split('/').collect();
    let mut paths = vec![root_path.to_string()];
    for index in 0..components.len() {
        let prefix = components[..=index].join("/");
        paths.push(normalize_posix_path(&format!("{root_path}/{prefix}")));
    }
    paths
}

fn exceeds_edit_limit(text: &str) -> bool {
    text.len() > MAX_EDIT_BYTES
}

impl MobileFeatures {
    fn require_mobile_diff_task(&self, task_id: &str) -> MobileResult<TaskWithProject> {
        let task = self.require_mobile_task(task_id)?;
        let has_worktree = task
            .task
            .worktree_path
            .as_deref()
            .is_some_and(|p| !p.is_empty())
            && task
                .task
                .branch_name
                .as_deref()
                .is_some_and(|b| !b.is_empty());
        if !has_worktree {
            return Err(MobileError::feature(409, "변경을 볼 worktree가 없습니다"));
        }
        Ok(task)
    }

    pub fn get_mobile_diff(&self, task_id: &str) -> MobileResult<Vec<DiffFile>> {
        self.require_mobile_diff_task(task_id)?;
        Ok(self.diff.get_git_diff_files(task_id))
    }

    /// 진행 중 태스크의 변경 집계. 저장된 집계를 먼저 돌려주고 갱신은 뒤에서 돈다
    pub fn get_mobile_diff_stats(&self) -> MobileResult<BTreeMap<String, TaskDiffStats>> {
        let board = self.tasks.get_tasks_by_status()?;
        let progress_task_ids: Vec<String> = board
            .tasks
            .progress
            .iter()
            .map(|task| task.id.clone())
            .collect();
        Ok(self.diff.get_task_diff_stats(&progress_task_ids))
    }

    fn validate_mobile_diff_path(
        &self,
        task_id: &str,
        submitted_path: Option<&str>,
    ) -> MobileResult<ValidatedDiffPath> {
        let path_value = submitted_path.map(|path| Value::String(path.to_string()));
        let file_path =
            read_mobile_text(path_value.as_ref(), "파일 경로", DEFAULT_TEXT_MAX_LENGTH)?;
        if is_unsafe_relative_path(&file_path) {
            return Err(MobileError::feature(
                400,
                "worktree 안의 상대 파일 경로를 선택해 주세요",
            ));
        }
        let task = self.require_mobile_diff_task(task_id)?;
        let files = self.diff.get_git_diff_files(task_id);
        let Some(file) = files.iter().find(|entry| entry.path == file_path) else {
            return Err(MobileError::feature(404, "변경 목록에 없는 파일입니다"));
        };
        let root_path = task.task.worktree_path.clone().unwrap_or_default();
        let paths = build_ancestor_paths(&root_path, &file_path);
        let symbolic_link = self
            .diff
            .find_symbolic_link_path(&paths, task.task.ssh_host.as_deref())
            .map_err(MobileError::Internal)?;
        if symbolic_link.is_some() {
            return Err(MobileError::feature(
                400,
                "심볼릭 링크 파일은 열 수 없습니다",
            ));
        }
        Ok(ValidatedDiffPath {
            original_path: file
                .original_path
                .clone()
                .unwrap_or_else(|| file_path.clone()),
            file_path,
        })
    }

    pub fn get_mobile_diff_file(
        &self,
        task_id: &str,
        submitted_path: Option<&str>,
    ) -> MobileResult<MobileDiffFileContent> {
        let validated = self.validate_mobile_diff_path(task_id, submitted_path)?;
        let original = self
            .diff
            .get_original_file_content(task_id, &validated.original_path);
        let content = self.diff.get_file_content(task_id, &validated.file_path);
        if exceeds_edit_limit(&original) || exceeds_edit_limit(&content) {
            return Err(MobileError::feature(
                409,
                "모바일에서 열 수 있는 파일 크기를 넘었습니다",
            ));
        }
        if original.contains('\0') || content.contains('\0') {
            return Err(MobileError::feature(
                409,
                "바이너리 파일은 편집할 수 없습니다",
            ));
        }
        Ok(MobileDiffFileContent { original, content })
    }

    pub fn save_mobile_diff_file(
        &self,
        task_id: &str,
        submitted_path: Option<&str>,
        input: &Value,
    ) -> MobileResult<()> {
        let body = read_mobile_object(input)?;
        let Some(content) = body
            .get("content")
            .and_then(Value::as_str)
            .filter(|content| !content.contains('\0') && !exceeds_edit_limit(content))
        else {
            return Err(MobileError::feature(
                400,
                "저장할 파일 내용이 올바르지 않거나 너무 큽니다",
            ));
        };
        let validated = self.validate_mobile_diff_path(task_id, submitted_path)?;
        let saved = self
            .diff
            .save_file_content(task_id, &validated.file_path, content);
        if !saved.success {
            return Err(MobileError::feature(409, "파일을 저장하지 못했습니다"));
        }
        Ok(())
    }
}
