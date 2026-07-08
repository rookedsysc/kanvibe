use std::{
    error::Error,
    fmt::{Display, Formatter},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use kanvibe_core::SessionType;

pub const CRATE_PURPOSE: &str = "Git, branch, worktree, and pull request command contracts";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BranchRef {
    name: String,
}

impl BranchRef {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GitError {
    message: String,
}

impl GitError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for GitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GitError {}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DiffFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl DiffFileStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DiffFile {
    pub path: String,
    pub status: DiffFileStatus,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WorktreeSession {
    pub worktree_path: PathBuf,
    pub session_name: String,
    pub session_type: SessionType,
}

pub fn exec_git(repo_path: impl AsRef<Path>, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path.as_ref())
        .args(args)
        .output()
        .map_err(|error| GitError::new(format!("failed to spawn git: {error}")))?;

    if !output.status.success() {
        return Err(GitError::new(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn validate_git_repo(repo_path: impl AsRef<Path>) -> Result<bool, GitError> {
    Ok(exec_git(repo_path, &["rev-parse", "--is-inside-work-tree"])?.trim() == "true")
}

pub fn current_branch(repo_path: impl AsRef<Path>) -> Result<String, GitError> {
    exec_git(repo_path, &["branch", "--show-current"])
}

pub fn list_branches(repo_path: impl AsRef<Path>) -> Result<Vec<String>, GitError> {
    let output = exec_git(repo_path, &["branch", "--format=%(refname:short)"])?;

    Ok(output
        .lines()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

pub fn format_session_name(project_name: &str, branch_name: &str) -> String {
    format!("{project_name}-{branch_name}").replace('/', "-")
}

pub fn build_managed_worktree_path(project_path: impl AsRef<Path>, branch_name: &str) -> PathBuf {
    let project_path = project_path.as_ref();
    let project_name = project_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let worktree_base = project_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{project_name}__worktrees"));

    worktree_base.join(branch_name.replace('/', "-"))
}

pub fn create_worktree_with_session(
    project_path: impl AsRef<Path>,
    branch_name: &str,
    base_branch: &str,
    session_type: SessionType,
) -> Result<WorktreeSession, GitError> {
    let project_path = project_path.as_ref();
    let worktree_path = build_managed_worktree_path(project_path, branch_name);
    let project_name = project_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let session_name = format_session_name(project_name, branch_name);
    let worktree_path_arg = worktree_path.to_string_lossy().to_string();

    exec_git(
        project_path,
        &[
            "worktree",
            "add",
            &worktree_path_arg,
            "-b",
            branch_name,
            base_branch,
        ],
    )?;

    Ok(WorktreeSession {
        worktree_path,
        session_name,
        session_type,
    })
}

pub fn changed_files(
    worktree_path: impl AsRef<Path>,
    base_branch: &str,
    branch_name: &str,
) -> Result<Vec<DiffFile>, GitError> {
    let worktree_path = worktree_path.as_ref();
    let range = format!("{base_branch}...{branch_name}");
    let name_status = exec_git(worktree_path, &["diff", &range, "--name-status"])
        .or_else(|_| Ok::<String, GitError>(String::new()))?;
    let numstat = exec_git(worktree_path, &["diff", &range, "--numstat"])
        .or_else(|_| Ok::<String, GitError>(String::new()))?;
    let status = exec_git(
        worktree_path,
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    let mut files = Vec::<DiffFile>::new();
    let stats = parse_numstat(&numstat);

    for line in name_status
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        let file_path = if parts.len() >= 3 { parts[2] } else { parts[1] };
        let (additions, deletions) = stats
            .iter()
            .find(|(path, _, _)| path == file_path)
            .map(|(_, additions, deletions)| (*additions, *deletions))
            .unwrap_or((0, 0));

        files.push(DiffFile {
            path: file_path.to_owned(),
            status: parse_git_status(parts[0]),
            additions,
            deletions,
        });
    }

    for line in status.lines() {
        if line.len() < 4 {
            continue;
        }
        let file_path = line[3..].to_owned();
        if files.iter().any(|file| file.path == file_path) {
            continue;
        }

        if let Some(status) = parse_working_tree_status(&line[0..2]) {
            files.push(DiffFile {
                path: file_path,
                status,
                additions: 0,
                deletions: 0,
            });
        }
    }

    Ok(files)
}

pub fn original_file_content(
    worktree_path: impl AsRef<Path>,
    base_branch: &str,
    file_path: &str,
) -> Result<String, GitError> {
    validate_relative_path(worktree_path.as_ref(), file_path)?;
    exec_git(
        worktree_path,
        &["show", &format!("{base_branch}:{file_path}")],
    )
    .or_else(|_| Ok(String::new()))
}

pub fn file_content(worktree_path: impl AsRef<Path>, file_path: &str) -> Result<String, GitError> {
    let resolved_path = validate_relative_path(worktree_path.as_ref(), file_path)?;
    fs::read_to_string(&resolved_path).map_err(|error| {
        GitError::new(format!(
            "failed to read {}: {error}",
            resolved_path.display()
        ))
    })
}

pub fn save_file_content(
    worktree_path: impl AsRef<Path>,
    file_path: &str,
    content: &str,
) -> Result<(), GitError> {
    let resolved_path = validate_relative_path(worktree_path.as_ref(), file_path)?;

    fs::write(&resolved_path, content).map_err(|error| {
        GitError::new(format!(
            "failed to write {}: {error}",
            resolved_path.display()
        ))
    })
}

fn validate_relative_path(worktree_path: &Path, file_path: &str) -> Result<PathBuf, GitError> {
    let file_path = Path::new(file_path);
    if file_path.is_absolute() {
        return Err(GitError::new("absolute file paths are not allowed"));
    }

    let worktree_path = fs::canonicalize(worktree_path)
        .map_err(|error| GitError::new(format!("invalid worktree path: {error}")))?;
    let resolved_path = worktree_path.join(file_path);
    let parent = resolved_path
        .parent()
        .ok_or_else(|| GitError::new("file path has no parent"))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| GitError::new(format!("invalid file parent: {error}")))?;

    if !canonical_parent.starts_with(&worktree_path) {
        return Err(GitError::new("file path escapes worktree"));
    }

    Ok(resolved_path)
}

fn parse_numstat(output: &str) -> Vec<(String, u32, u32)> {
    output
        .lines()
        .filter_map(|line| {
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.len() < 3 {
                return None;
            }

            Some((
                parts[2..].join("\t"),
                parts[0].parse::<u32>().unwrap_or(0),
                parts[1].parse::<u32>().unwrap_or(0),
            ))
        })
        .collect()
}

fn parse_git_status(status: &str) -> DiffFileStatus {
    if status.starts_with('R') {
        return DiffFileStatus::Renamed;
    }

    match status.chars().next() {
        Some('A') => DiffFileStatus::Added,
        Some('D') => DiffFileStatus::Deleted,
        Some('M') => DiffFileStatus::Modified,
        _ => DiffFileStatus::Modified,
    }
}

fn parse_working_tree_status(xy: &str) -> Option<DiffFileStatus> {
    let mut chars = xy.chars();
    let index = chars.next()?;
    let worktree = chars.next()?;

    if xy == "??" {
        return Some(DiffFileStatus::Added);
    }
    if index == 'A' || worktree == 'A' {
        return Some(DiffFileStatus::Added);
    }
    if index == 'D' || worktree == 'D' {
        return Some(DiffFileStatus::Deleted);
    }
    if index == 'M' || worktree == 'M' {
        return Some(DiffFileStatus::Modified);
    }
    if index == 'R' || worktree == 'R' {
        return Some(DiffFileStatus::Renamed);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn branch_ref_preserves_branch_name() {
        let branch = BranchRef::new("feature/native-board");

        assert_eq!(branch.as_str(), "feature/native-board");
    }

    #[test]
    fn worktree_path_and_session_name_match_electron_contract() {
        let project_path = Path::new("/tmp/kanvibe");

        assert_eq!(
            build_managed_worktree_path(project_path, "qa/branch-from-task"),
            PathBuf::from("/tmp/kanvibe__worktrees/qa-branch-from-task")
        );
        assert_eq!(
            format_session_name("kanvibe", "qa/branch-from-task"),
            "kanvibe-qa-branch-from-task"
        );
    }

    #[test]
    fn changed_files_include_branch_diff_and_working_tree_entries() {
        let repo = create_temp_git_repo("diff-files");
        write_file(repo.join("src/app.rs"), "fn main() {}\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);
        git(&repo, &["checkout", "-b", "feat/native-diff"]);
        write_file(
            repo.join("src/app.rs"),
            "fn main() {\n    println!(\"native\");\n}\n",
        );
        git(&repo, &["add", "src/app.rs"]);
        git(&repo, &["commit", "-m", "modify app"]);
        write_file(repo.join("src/new.rs"), "pub fn added() {}\n");

        let files = changed_files(&repo, "main", "feat/native-diff").expect("changed files");
        assert!(
            files
                .iter()
                .any(|file| file.path == "src/app.rs" && file.status == DiffFileStatus::Modified)
        );
        assert!(
            files
                .iter()
                .any(|file| file.path == "src/new.rs" && file.status == DiffFileStatus::Added)
        );

        assert_eq!(
            original_file_content(&repo, "main", "src/app.rs").expect("original content"),
            "fn main() {}"
        );
        assert!(
            file_content(&repo, "src/app.rs")
                .expect("current content")
                .contains("native")
        );
        save_file_content(&repo, "src/app.rs", "fn main() { println!(\"saved\"); }\n")
            .expect("save content");
        assert!(
            file_content(&repo, "src/app.rs")
                .expect("saved content")
                .contains("saved")
        );
    }

    #[test]
    fn worktree_creation_matches_branch_from_task_flow() {
        let repo = create_temp_git_repo("worktree-flow");
        write_file(repo.join("README.md"), "# KanVibe\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);

        let session =
            create_worktree_with_session(&repo, "qa/branch-from-task", "main", SessionType::Tmux)
                .expect("worktree should be created");

        assert!(session.worktree_path.exists());
        assert_eq!(session.session_name, "worktree-flow-qa-branch-from-task");
        assert_eq!(
            current_branch(&session.worktree_path).expect("worktree branch"),
            "qa/branch-from-task"
        );
    }

    fn create_temp_git_repo(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kanvibe-git-{name}-{}-{unique}",
            std::process::id()
        ));
        let path = root.join(name);
        fs::create_dir_all(&path).expect("temp repo dir");
        git_raw(&path, &["init", "-b", "main"]);
        git(&path, &["config", "user.email", "qa@kanvibe.test"]);
        git(&path, &["config", "user.name", "KanVibe QA"]);
        fs::create_dir_all(path.join("src")).expect("src dir");
        path
    }

    fn write_file(path: PathBuf, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("file parent");
        }
        fs::write(path, content).expect("write test file");
    }

    fn git(repo: &Path, args: &[&str]) {
        exec_git(repo, args).unwrap_or_else(|error| panic!("git {args:?} failed: {error}"));
    }

    fn git_raw(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
