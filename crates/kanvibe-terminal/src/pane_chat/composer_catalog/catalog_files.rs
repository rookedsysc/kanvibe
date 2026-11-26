//! skill·명령 파일 찾기와 읽기. 로컬은 파일 시스템을 직접 훑고, 원격은 선택한 SSH 호스트에서만 `find`·`head`를 실행한다.

use std::collections::{HashMap, HashSet};
use std::fs::{self, FileType};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use kanvibe_ai::node_path;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use kanvibe_process::worktree::quote_for_posix_shell;

pub(super) const MAX_CATALOG_FILES: usize = 512;
pub(super) const CATALOG_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CATALOG_DEPTH: usize = 8;
const MAX_CATALOG_DIRECTORIES: usize = 2048;
const MAX_SKILL_BYTES: usize = 64 * 1024;
const SKIPPED_DIRECTORY_NAMES: [&str; 3] = [".git", "node_modules", ".venv"];

/// 훑을 폴더 하나. 명령 폴더는 모든 `.md`를, skill 폴더는 `SKILL.md`만 모은다
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CatalogRoot {
    pub directory: String,
    pub is_command_root: bool,
}

impl CatalogRoot {
    fn accepts_file_name(&self, file_name: &str) -> bool {
        if self.is_command_root {
            file_name.ends_with(".md")
        } else {
            file_name == "SKILL.md"
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CatalogFile {
    pub file_path: String,
    pub root: CatalogRoot,
    pub content: String,
}

pub(super) struct CatalogFiles {
    pub files: Vec<CatalogFile>,
    pub is_truncated: bool,
}

fn catalog_options() -> ExecGitOptions {
    ExecGitOptions {
        timeout: Some(CATALOG_TIMEOUT),
        ..ExecGitOptions::default()
    }
}

/// 로컬 폴더를 깊이·개수 상한 안에서 훑는다. 심볼릭 링크 폴더도 따라가되 같은 실제 폴더는 한 번만 본다
#[derive(Default)]
struct LocalCatalogWalk {
    files: Vec<(String, CatalogRoot)>,
    visited: HashSet<PathBuf>,
    is_truncated: bool,
}

/// Node `readdir`(libuv scandir)처럼 이름의 바이트 순서로 정렬한 항목
fn sorted_directory_entries(directory: &str) -> Vec<(String, FileType)> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut named_entries: Vec<(Vec<u8>, String, FileType)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let raw_name = entry.file_name();
            Some((
                raw_name.as_bytes().to_vec(),
                raw_name.to_string_lossy().into_owned(),
                file_type,
            ))
        })
        .collect();
    named_entries.sort_by(|left, right| left.0.cmp(&right.0));
    named_entries
        .into_iter()
        .map(|(_, name, file_type)| (name, file_type))
        .collect()
}

fn is_directory_entry(file_path: &str, file_type: FileType) -> bool {
    file_type.is_dir()
        || (file_type.is_symlink()
            && fs::metadata(file_path).is_ok_and(|metadata| metadata.is_dir()))
}

impl LocalCatalogWalk {
    fn walk(&mut self, directory: &str, root: &CatalogRoot, depth: usize) {
        if depth > MAX_CATALOG_DEPTH
            || self.files.len() >= MAX_CATALOG_FILES
            || self.visited.len() >= MAX_CATALOG_DIRECTORIES
        {
            self.is_truncated = true;
            return;
        }
        let Ok(canonical) = fs::canonicalize(directory) else {
            return;
        };
        if !self.visited.insert(canonical) {
            return;
        }
        for (name, file_type) in sorted_directory_entries(directory) {
            if SKIPPED_DIRECTORY_NAMES.contains(&name.as_str()) {
                continue;
            }
            let file_path = node_path::join(&[directory, &name]);
            if is_directory_entry(&file_path, file_type) {
                self.walk(&file_path, root, depth + 1);
            } else if root.accepts_file_name(&name) {
                self.files.push((file_path, root.clone()));
                if self.files.len() >= MAX_CATALOG_FILES {
                    self.is_truncated = true;
                    return;
                }
            }
        }
    }
}

/// Node `Buffer.toString("utf8")`처럼 앞 64KiB만 읽고 잘못된 바이트는 대체 문자로 바꾼다
fn read_skill_prefix(file_path: &str) -> Option<String> {
    let file = fs::File::open(file_path).ok()?;
    let mut bytes = Vec::with_capacity(MAX_SKILL_BYTES);
    file.take(MAX_SKILL_BYTES as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub(super) fn read_local_catalog_files(roots: &[CatalogRoot]) -> CatalogFiles {
    let mut walk = LocalCatalogWalk::default();
    for root in roots {
        walk.walk(&root.directory, root, 0);
    }
    let files = walk
        .files
        .into_iter()
        .filter_map(|(file_path, root)| {
            let content = read_skill_prefix(&file_path)?;
            Some(CatalogFile {
                file_path,
                root,
                content,
            })
        })
        .collect();
    CatalogFiles {
        files,
        is_truncated: walk.is_truncated,
    }
}

fn remote_find_command(root: &CatalogRoot) -> String {
    let pattern = if root.is_command_root {
        "*.md"
    } else {
        "SKILL.md"
    };
    let directory = quote_for_posix_shell(&root.directory);
    format!(
        "test ! -d {directory} || find -L {directory} -maxdepth {MAX_CATALOG_DEPTH} \\( -name .git -o -name node_modules -o -name .venv \\) -prune -o -type f -name {} -print0 2>/dev/null || true",
        quote_for_posix_shell(pattern)
    )
}

/// 폴더마다 `find`로 경로를 모은다. 상한에 닿으면 남은 폴더는 보지 않는다
fn find_remote_catalog_paths(
    runner: &dyn ShellCommandRunner,
    ssh_host: &str,
    roots: &[CatalogRoot],
) -> Result<Vec<(String, CatalogRoot)>, ExecGitError> {
    let mut found = Vec::new();
    for root in roots {
        let output = runner.exec_git(
            &remote_find_command(root),
            Some(ssh_host),
            &catalog_options(),
        )?;
        let paths = output.split('\0').filter(|file_path| !file_path.is_empty());
        found.extend(paths.map(|file_path| (file_path.to_string(), root.clone())));
        if found.len() >= MAX_CATALOG_FILES {
            break;
        }
    }
    Ok(found)
}

/// 경로와 앞 64KiB의 Base64를 NUL로 번갈아 내보낸다. 읽지 못한 파일은 내용이 비어 빠진다
fn read_remote_contents(
    runner: &dyn ShellCommandRunner,
    ssh_host: &str,
    selected: &[(String, CatalogRoot)],
) -> Result<HashMap<String, String>, ExecGitError> {
    let quoted_paths: Vec<String> = selected
        .iter()
        .map(|(file_path, _)| quote_for_posix_shell(file_path))
        .collect();
    let command = format!(
        "for __kanvibe_skill in {}; do printf '%s\\0' \"$__kanvibe_skill\"; head -c {MAX_SKILL_BYTES} \"$__kanvibe_skill\" 2>/dev/null | base64 | tr -d '\\r\\n'; printf '\\0'; done",
        quoted_paths.join(" ")
    );
    let options = ExecGitOptions {
        max_buffer_bytes: Some(MAX_CATALOG_FILES * MAX_SKILL_BYTES * 2),
        ..catalog_options()
    };
    let output = runner.exec_git(&command, Some(ssh_host), &options)?;
    let parts: Vec<&str> = output.split('\0').collect();
    let mut contents = HashMap::new();
    for pair in parts.as_chunks::<2>().0 {
        if pair[1].is_empty() {
            continue;
        }
        // `base64` 명령이 만든 값이라 표준 Base64다. 풀지 못하면 그 파일은 읽지 못한 것으로 본다
        if let Ok(bytes) = STANDARD.decode(pair[1]) {
            contents.insert(
                pair[0].to_string(),
                String::from_utf8_lossy(&bytes).into_owned(),
            );
        }
    }
    Ok(contents)
}

pub(super) fn read_remote_catalog_files(
    runner: &dyn ShellCommandRunner,
    ssh_host: &str,
    roots: &[CatalogRoot],
) -> Result<CatalogFiles, ExecGitError> {
    let found = find_remote_catalog_paths(runner, ssh_host, roots)?;
    let selected = &found[..found.len().min(MAX_CATALOG_FILES)];
    if selected.is_empty() {
        return Ok(CatalogFiles {
            files: Vec::new(),
            is_truncated: false,
        });
    }
    let contents = read_remote_contents(runner, ssh_host, selected)?;
    let files = selected
        .iter()
        .filter_map(|(file_path, root)| {
            let content = contents.get(file_path).cloned()?;
            Some(CatalogFile {
                file_path: file_path.clone(),
                root: root.clone(),
                content,
            })
        })
        .collect();
    Ok(CatalogFiles {
        files,
        is_truncated: found.len() >= MAX_CATALOG_FILES,
    })
}
