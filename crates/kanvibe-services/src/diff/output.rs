//! 변경 파일 목록 명령 하나와 그 NUL 구분 출력 해석. 경로에 공백·탭·개행·한글이 있어도 그대로 읽도록 `-z` 출력만 쓴다.

use std::collections::HashMap;

use kanvibe_domain::task_diff_stats::{DiffFile, DiffFileStatus};
use kanvibe_process::host_file_access::quote_shell_argument;

const DIFF_NAME_STATUS_MARKER: &str = "__KANVIBE_DIFF_NAME_STATUS__";
const DIFF_NUMSTAT_MARKER: &str = "__KANVIBE_DIFF_NUMSTAT__";
const DIFF_WORKING_TREE_MARKER: &str = "__KANVIBE_DIFF_WORKING_TREE__";

pub(super) fn build_git_command(worktree_path: &str, arguments: &[&str]) -> String {
    format!(
        "git -C {} {}",
        quote_shell_argument(worktree_path),
        arguments.join(" ")
    )
}

/// HEAD 대비 이름·상태, 줄 수, 추적하지 않는 파일의 줄 수, 작업 트리 상태를 구간 marker와 함께 한 번에 출력한다
pub(super) fn build_git_diff_files_command(worktree_path: &str) -> String {
    let untracked_numstat = [
        "worktreePath=$1; shift",
        "for filePath do",
        "git -C \"$worktreePath\" diff --no-index --numstat -z -- /dev/null \"$filePath\"",
        "result=$?",
        "if [ \"$result\" -gt 1 ]; then exit \"$result\"; fi",
        "done",
    ]
    .join("\n");
    [
        format!(
            "printf '%s\\0' {}",
            quote_shell_argument(DIFF_NAME_STATUS_MARKER)
        ),
        build_git_command(worktree_path, &["diff", "HEAD", "--name-status", "-z"]),
        format!(
            "printf '%s\\0' {}",
            quote_shell_argument(DIFF_NUMSTAT_MARKER)
        ),
        build_git_command(worktree_path, &["diff", "HEAD", "--numstat", "-z"]),
        format!(
            "{} | xargs -0 sh -c {} sh {}",
            build_git_command(
                worktree_path,
                &["ls-files", "--others", "--exclude-standard", "-z"]
            ),
            quote_shell_argument(&untracked_numstat),
            quote_shell_argument(worktree_path)
        ),
        format!(
            "printf '%s\\0' {}",
            quote_shell_argument(DIFF_WORKING_TREE_MARKER)
        ),
        build_git_command(
            worktree_path,
            &["status", "--porcelain", "--untracked-files=all", "-z"],
        ),
    ]
    .join(" && ")
}

#[derive(Debug, Default)]
struct DiffSections {
    name_status: Vec<String>,
    numstat: Vec<String>,
    working_tree: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    NameStatus,
    Numstat,
    WorkingTree,
}

fn section_of_marker(record: &str) -> Option<Section> {
    match record {
        DIFF_NAME_STATUS_MARKER => Some(Section::NameStatus),
        DIFF_NUMSTAT_MARKER => Some(Section::Numstat),
        DIFF_WORKING_TREE_MARKER => Some(Section::WorkingTree),
        _ => None,
    }
}

fn starts_with_rename_or_copy(record: &str) -> bool {
    record.starts_with(['R', 'C'])
}

fn status_pair(record: &str) -> String {
    record.chars().take(2).collect()
}

/// 레코드 하나가 차지하는 NUL 필드 수. rename의 경로 필드는 구간 marker와 같은 이름이어도 경로로 소비해야 한다
fn record_width(section: Section, record: &str) -> usize {
    match section {
        Section::NameStatus if starts_with_rename_or_copy(record) => 3,
        Section::NameStatus => 2,
        Section::Numstat if record.ends_with('\t') => 3,
        Section::Numstat => 1,
        Section::WorkingTree if status_pair(record).contains(['R', 'C']) => 2,
        Section::WorkingTree => 1,
    }
}

fn parse_sections(output: &str) -> DiffSections {
    let records: Vec<&str> = output.split('\0').collect();
    let mut sections = DiffSections::default();
    let mut current: Option<Section> = None;
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if let Some(section) = section_of_marker(record) {
            current = Some(section);
            index += 1;
            continue;
        }
        let Some(section) = current.filter(|_| !record.is_empty()) else {
            index += 1;
            continue;
        };
        let width = record_width(section, record);
        let end = (index + width).min(records.len());
        let target = match section {
            Section::NameStatus => &mut sections.name_status,
            Section::Numstat => &mut sections.numstat,
            Section::WorkingTree => &mut sections.working_tree,
        };
        target.extend(records[index..end].iter().map(|field| field.to_string()));
        index += width;
    }
    sections
}

#[derive(Debug, Clone, Copy, Default)]
struct LineCounts {
    additions: i64,
    deletions: i64,
}

/// 바이너리 파일은 `-`로 나와 0으로 센다. JS `parseInt`처럼 앞쪽 숫자만 읽는다
fn parse_count(value: &str) -> i64 {
    if value == "-" {
        return 0;
    }
    let digits: String = value
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

fn line_counts_by_path(numstat: &[String]) -> HashMap<String, LineCounts> {
    let mut counts = HashMap::new();
    let mut index = 0;
    while index < numstat.len() {
        let mut fields = numstat[index].split('\t');
        let added = fields.next().unwrap_or_default();
        let deleted = fields.next().unwrap_or_default();
        let mut file_path = fields.collect::<Vec<_>>().join("\t");
        if file_path.is_empty() {
            file_path = numstat.get(index + 2).cloned().unwrap_or_default();
            index += 2;
        }
        let line_counts = LineCounts {
            additions: parse_count(added),
            deletions: parse_count(deleted),
        };
        counts.insert(file_path, line_counts);
        index += 1;
    }
    counts
}

fn parse_git_status(status_letter: &str) -> DiffFileStatus {
    match status_letter {
        "A" => DiffFileStatus::Added,
        "D" => DiffFileStatus::Deleted,
        "M" => DiffFileStatus::Modified,
        _ if status_letter.starts_with('R') => DiffFileStatus::Renamed,
        _ => DiffFileStatus::Modified,
    }
}

/// JS `Map`처럼 같은 경로를 다시 넣으면 처음 자리에서 값만 바꾼다
#[derive(Default)]
struct OrderedDiffFiles {
    files: Vec<DiffFile>,
    index_by_path: HashMap<String, usize>,
}

impl OrderedDiffFiles {
    fn insert(&mut self, file: DiffFile) {
        match self.index_by_path.get(&file.path) {
            Some(&index) => self.files[index] = file,
            None => {
                self.index_by_path
                    .insert(file.path.clone(), self.files.len());
                self.files.push(file);
            }
        }
    }
}

fn counts_for(counts: &HashMap<String, LineCounts>, file_path: &str) -> LineCounts {
    counts.get(file_path).copied().unwrap_or_default()
}

/// HEAD와 작업 트리의 차이로 staged·unstaged 변경을 함께 구성한다
fn add_head_changes(
    files: &mut OrderedDiffFiles,
    name_status: &[String],
    counts: &HashMap<String, LineCounts>,
) {
    let mut fields = name_status.iter();
    while let Some(status_letter) = fields.next() {
        let original_path = fields.next().cloned().unwrap_or_default();
        let is_rename_or_copy = starts_with_rename_or_copy(status_letter);
        let file_path = if is_rename_or_copy {
            fields.next().cloned().unwrap_or_default()
        } else {
            original_path.clone()
        };
        let line_counts = counts_for(counts, &file_path);
        files.insert(DiffFile {
            path: file_path,
            original_path: is_rename_or_copy.then_some(original_path),
            status: parse_git_status(status_letter),
            additions: line_counts.additions,
            deletions: line_counts.deletions,
        });
    }
}

/// 추적되지 않은 파일은 HEAD diff에 없으므로 별도로 포함한다
fn add_untracked_files(
    files: &mut OrderedDiffFiles,
    working_tree: &[String],
    counts: &HashMap<String, LineCounts>,
) {
    let mut index = 0;
    while index < working_tree.len() {
        let record = &working_tree[index];
        let pair = status_pair(record);
        if pair.contains(['R', 'C']) {
            index += 1;
        }
        index += 1;
        if pair != "??" {
            continue;
        }
        let file_path: String = record.chars().skip(3).collect();
        let line_counts = counts_for(counts, &file_path);
        files.insert(DiffFile {
            path: file_path,
            original_path: None,
            status: DiffFileStatus::Added,
            additions: line_counts.additions,
            deletions: line_counts.deletions,
        });
    }
}

pub(super) fn parse_git_diff_files(output: &str) -> Vec<DiffFile> {
    let sections = parse_sections(output);
    let counts = line_counts_by_path(&sections.numstat);
    let mut files = OrderedDiffFiles::default();
    add_head_changes(&mut files, &sections.name_status, &counts);
    add_untracked_files(&mut files, &sections.working_tree, &counts);
    files.files
}
