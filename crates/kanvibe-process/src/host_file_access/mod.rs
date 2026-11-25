//! 로컬·원격 공통 파일 접근(TS `hostFileAccess.ts`).
//! 로컬은 파일 시스템을 직접 쓰고, 원격은 `exec_git`으로 셸 명령(`cat`, `test`, `find`, `stat`)을 보낸다.
//! 경로는 원격 POSIX 경로도 담아야 하므로 `&str`로 받는다.

mod file_update;
mod local_listing;
mod remote_commands;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};

#[derive(Debug, thiserror::Error)]
pub enum HostFileError {
    #[error(transparent)]
    Exec(#[from] ExecGitError),
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// TS가 `new Error(...)`로 던지던 한국어 메시지. 문자열을 바이트 단위로 같게 유지한다
    #[error("{0}")]
    Message(String),
}

impl HostFileError {
    pub fn io(path: &str, source: std::io::Error) -> Self {
        Self::Io {
            path: path.to_string(),
            source,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFileReadResult {
    pub exists: bool,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModifiedFile {
    pub file_path: String,
    pub mtime_ms: f64,
}

/// 파일 갱신 callback. 지금 내용을 받아 새 내용을 돌려준다
pub type TextFileUpdate<'a> = dyn FnMut(&str) -> Result<String, HostFileError> + 'a;

/// AI 기록 JSONL은 일반 명령 상한 10 MiB를 넘을 수 있어, 원격 읽기는 64 MiB까지 받는다
pub const REMOTE_TEXT_FILE_MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;

pub fn quote_shell_argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// 실행기와 로컬 HOME, 원격 HOME 캐시를 묶은 파일 접근 문맥. 앱에서 하나를 만들어 공유한다
pub struct HostFileAccess {
    runner: Arc<dyn ShellCommandRunner>,
    local_home_directory: String,
    remote_home_directories: Mutex<HashMap<String, String>>,
}

fn remote_host(ssh_host: Option<&str>) -> Option<&str> {
    ssh_host.filter(|host| !host.is_empty())
}

/// TS의 `Pick<ExecGitOptions, "strictHostKeyChecking">`. 호출자가 준 옵션 가운데 호스트 키 확인만 SSH 경계로 넘긴다
fn strict_only(options: &ExecGitOptions) -> ExecGitOptions {
    ExecGitOptions {
        should_check_host_key_strictly: options.should_check_host_key_strictly,
        ..ExecGitOptions::default()
    }
}

/// Node `readFile(path, "utf-8")`처럼 잘못된 UTF-8 바이트는 대체 문자로 읽는다
fn read_local_text(target_path: &str) -> std::io::Result<String> {
    let bytes = std::fs::read(target_path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_not_found(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
}

impl HostFileAccess {
    pub fn new(
        runner: Arc<dyn ShellCommandRunner>,
        local_home_directory: impl Into<String>,
    ) -> Self {
        Self {
            runner,
            local_home_directory: local_home_directory.into(),
            remote_home_directories: Mutex::new(HashMap::new()),
        }
    }

    pub fn runner(&self) -> &dyn ShellCommandRunner {
        self.runner.as_ref()
    }

    pub fn shared_runner(&self) -> Arc<dyn ShellCommandRunner> {
        Arc::clone(&self.runner)
    }

    pub fn local_home_directory(&self) -> &str {
        &self.local_home_directory
    }

    fn exec_remote(
        &self,
        command: &str,
        host: &str,
        options: &ExecGitOptions,
    ) -> Result<String, HostFileError> {
        Ok(self.runner.exec_git(command, Some(host), options)?)
    }

    pub fn home_directory(&self, ssh_host: Option<&str>) -> Result<String, HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            return Ok(self.local_home_directory.clone());
        };
        if let Some(cached) = self.remote_home_directories.lock().unwrap().get(host) {
            return Ok(cached.clone());
        }
        let output = self.exec_remote("printf '%s' \"$HOME\"", host, &ExecGitOptions::default())?;
        let home_directory = output.trim().to_string();
        if home_directory.is_empty() {
            return Err(HostFileError::Message(format!(
                "{host} 원격 HOME 디렉토리를 확인할 수 없습니다."
            )));
        }
        self.remote_home_directories
            .lock()
            .unwrap()
            .insert(host.to_string(), home_directory.clone());
        Ok(home_directory)
    }

    pub fn path_exists(
        &self,
        target_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            return Ok(std::fs::metadata(target_path).is_ok());
        };
        let command = format!(
            "test -e {} && printf '1' || true",
            quote_shell_argument(target_path)
        );
        let output = self.exec_remote(&command, host, &ExecGitOptions::default())?;
        Ok(output.trim() == "1")
    }

    /// 경로가 정말 사라졌는지 본다. 접근이 거절된 상위 디렉터리나 끊어진 링크, SSH 실패는 삭제로 보지 않는다
    pub fn is_path_missing(
        &self,
        target_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            return match std::fs::symlink_metadata(target_path) {
                Ok(_) => Ok(false),
                Err(error) if is_not_found(&error) => Ok(true),
                Err(error) => Err(HostFileError::io(target_path, error)),
            };
        };
        let command = remote_commands::build_path_presence_command(target_path, host);
        let output = self.exec_remote(&command, host, &ExecGitOptions::default())?;
        match output.trim() {
            "missing" => Ok(true),
            "present" => Ok(false),
            _ => Err(HostFileError::Message(format!(
                "{target_path} 경로를 확인하지 못했습니다."
            ))),
        }
    }

    /// 경로 가운데 심볼릭 링크인 첫 경로. 없는 경로는 링크가 아니다
    pub fn find_symbolic_link_path(
        &self,
        target_paths: &[String],
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<Option<String>, HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            let symbolic_link_path = target_paths.iter().find(|target_path| {
                std::fs::symlink_metadata(target_path)
                    .map(|metadata| metadata.file_type().is_symlink())
                    .unwrap_or(false)
            });
            return Ok(symbolic_link_path.cloned());
        };
        let quoted_paths: Vec<String> = target_paths
            .iter()
            .map(|target_path| quote_shell_argument(target_path))
            .collect();
        let command = format!(
            "for __kanvibe_path in {}; do if test -L \"$__kanvibe_path\"; then printf '%s' \"$__kanvibe_path\"; break; fi; done",
            quoted_paths.join(" ")
        );
        let output = self.exec_remote(&command, host, &strict_only(options))?;
        let symbolic_link_path = output.trim();
        Ok((!symbolic_link_path.is_empty()).then(|| symbolic_link_path.to_string()))
    }

    /// 없거나 읽지 못한 로컬 파일은 빈 문자열이다. 원격 읽기 실패는 빈 파일로 숨기지 않는다
    pub fn read_text_file(
        &self,
        target_path: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            return Ok(read_local_text(target_path).unwrap_or_default());
        };
        let quoted_path = quote_shell_argument(target_path);
        let command = format!("test -f {quoted_path} && cat {quoted_path} || true");
        let remote_options = ExecGitOptions {
            max_buffer_bytes: Some(REMOTE_TEXT_FILE_MAX_BUFFER_BYTES),
            ..strict_only(options)
        };
        self.exec_remote(&command, host, &remote_options)
    }

    /// 없는 파일은 `exists: false`, 그 밖의 읽기 실패는 오류다. 원격은 한 번의 왕복으로 모두 읽는다
    pub fn read_text_files(
        &self,
        target_paths: &[String],
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<BTreeMap<String, TextFileReadResult>, HostFileError> {
        if target_paths.is_empty() {
            return Ok(BTreeMap::new());
        }
        let Some(host) = remote_host(ssh_host) else {
            return read_local_text_files(target_paths);
        };
        let command = remote_commands::build_read_text_files_command(target_paths);
        let output = self.exec_remote(&command, host, &strict_only(options))?;
        remote_commands::parse_remote_text_files(&output, target_paths)
    }

    pub fn write_text_file(
        &self,
        target_path: &str,
        content: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<(), HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            create_parent_directory(target_path)?;
            return std::fs::write(target_path, content)
                .map_err(|error| HostFileError::io(target_path, error));
        };
        let command = remote_commands::build_write_text_file_command(target_path, content);
        self.exec_remote(&command, host, &strict_only(options))?;
        Ok(())
    }

    /// 저장소 측 잠금(`<file>.lock` 디렉터리)으로 여러 프로세스와 SSH client의 읽기·수정·교체를 직렬화한다.
    /// `mode`를 주면 내용이 담기는 임시 파일부터 그 권한으로 만들어, 교체 전후 어느 순간에도 넓은 권한으로 열리지 않는다.
    pub fn update_text_file(
        &self,
        target_path: &str,
        ssh_host: Option<&str>,
        mode: Option<u32>,
        options: &ExecGitOptions,
        update: &mut TextFileUpdate<'_>,
    ) -> Result<(), HostFileError> {
        let _in_process_guard = file_update::InProcessFileLock::acquire(ssh_host, target_path);
        match remote_host(ssh_host) {
            None => file_update::update_local_text_file(target_path, mode, update),
            Some(host) => file_update::update_remote_text_file(
                self.runner(),
                target_path,
                host,
                mode,
                &strict_only(options),
                update,
            ),
        }
    }

    /// 대상 파일이 아직 없을 때만 기록한다. 이미 있는 파일은 다른 주체가 확정한 값이라 덮지 않는다.
    /// 그래서 로컬은 생성 전용 열기로, 원격은 no-clobber redirection으로 생성 시 경합을 막는다.
    pub fn write_text_file_if_absent(
        &self,
        target_path: &str,
        content: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            return write_local_text_file_if_absent(target_path, content);
        };
        let command =
            remote_commands::build_write_text_file_if_absent_command(target_path, content);
        self.exec_remote(&command, host, &ExecGitOptions::default())?;
        Ok(())
    }

    pub fn read_directory_files_by_suffix(
        &self,
        directory_path: &str,
        suffix: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<String>, HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            return Ok(local_listing::read_directory_files_by_suffix(
                directory_path,
                suffix,
            ));
        };
        let quoted_directory = quote_shell_argument(directory_path);
        let command = format!(
            "test -d {quoted_directory} && find {quoted_directory} -maxdepth 1 -type f -name {} | sort || true",
            quote_shell_argument(&format!("*{suffix}"))
        );
        let output = self.exec_remote(&command, host, &ExecGitOptions::default())?;
        Ok(non_empty_lines(&output))
    }

    pub fn list_files_recursively_by_suffix(
        &self,
        root_path: &str,
        suffix: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<String>, HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            return Ok(local_listing::list_files_recursively_by_suffix(
                root_path, suffix,
            ));
        };
        let quoted_root = quote_shell_argument(root_path);
        let command = format!(
            "test -d {quoted_root} && find {quoted_root} -type f -name {} | sort || true",
            quote_shell_argument(&format!("*{suffix}"))
        );
        let output = self.exec_remote(&command, host, &ExecGitOptions::default())?;
        Ok(non_empty_lines(&output))
    }

    /// 최근에 수정된 파일만 골라 경로와 수정 시각을 함께 돌려준다.
    /// 원격에서 파일마다 mtime을 물으면 파일 수만큼 SSH 왕복이 생기므로 `find`가 필터링과 시각 출력을 한 번에 끝낸다.
    pub fn list_files_modified_within(
        &self,
        root_path: &str,
        suffix: &str,
        within: Duration,
        ssh_host: Option<&str>,
    ) -> Result<Vec<ModifiedFile>, HostFileError> {
        let oldest_allowed_ms = local_listing::now_ms() - within.as_millis() as f64;
        let Some(host) = remote_host(ssh_host) else {
            let modified_files =
                local_listing::list_files_with_mtime(root_path, suffix, oldest_allowed_ms);
            return Ok(modified_files);
        };
        let command =
            remote_commands::build_list_files_modified_within_command(root_path, suffix, within);
        let output = self.exec_remote(&command, host, &ExecGitOptions::default())?;
        Ok(remote_commands::parse_modified_files(
            &output,
            oldest_allowed_ms,
        ))
    }

    pub fn file_mtime_ms(
        &self,
        file_path: &str,
        ssh_host: Option<&str>,
    ) -> Result<Option<f64>, HostFileError> {
        let Some(host) = remote_host(ssh_host) else {
            return Ok(local_listing::file_mtime_ms(file_path));
        };
        let quoted_path = quote_shell_argument(file_path);
        let command = format!(
            "test -e {quoted_path} && (stat -c %Y {quoted_path} 2>/dev/null || stat -f %m {quoted_path} 2>/dev/null) || true"
        );
        let output = self.exec_remote(&command, host, &ExecGitOptions::default())?;
        Ok(remote_commands::parse_leading_integer(output.trim())
            .map(|seconds| seconds as f64 * 1000.0))
    }
}

fn non_empty_lines(output: &str) -> Vec<String> {
    output
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn read_local_text_files(
    target_paths: &[String],
) -> Result<BTreeMap<String, TextFileReadResult>, HostFileError> {
    let mut files = BTreeMap::new();
    for target_path in target_paths {
        let result = match read_local_text(target_path) {
            Ok(content) => TextFileReadResult {
                exists: true,
                content,
            },
            Err(error) if is_not_found(&error) => TextFileReadResult {
                exists: false,
                content: String::new(),
            },
            Err(error) => return Err(HostFileError::io(target_path, error)),
        };
        files.insert(target_path.clone(), result);
    }
    Ok(files)
}

fn write_local_text_file_if_absent(target_path: &str, content: &str) -> Result<(), HostFileError> {
    use std::io::Write;
    create_parent_directory(target_path)?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target_path);
    match file {
        Ok(mut file) => file
            .write_all(content.as_bytes())
            .map_err(|error| HostFileError::io(target_path, error)),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(HostFileError::io(target_path, error)),
    }
}

fn create_parent_directory(target_path: &str) -> Result<(), HostFileError> {
    match std::path::Path::new(target_path).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            std::fs::create_dir_all(parent).map_err(|error| HostFileError::io(target_path, error))
        }
        _ => Ok(()),
    }
}
