//! 세션 기록이 있는 호스트(로컬 또는 SSH 원격)의 파일과 명령. TS `hostFileAccess`에서 AI 리더가 쓰는 부분만 옮긴다.
//!
//! 원격 명령 실행(SSH 연결 풀, ControlMaster)은 `kanvibe-process`의 몫이라 여기서는 [`CommandRunner`] trait으로만 받는다.
//! 홈 디렉터리와 provider 환경 변수(`CODEX_HOME` 등)도 프로세스 환경에서 직접 읽지 않고 [`SessionHost`]가 들고 다닌다.

use std::collections::{BTreeMap, HashMap};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use crate::error::{AiError, AiResult};
use crate::node_path;

/// 명령 하나에 거는 제한. TS `ExecGitOptions`의 `maxBufferBytes`, `timeoutMs`에 해당한다
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandOptions {
    pub max_buffer_bytes: Option<usize>,
    pub timeout_ms: Option<u64>,
}

/// TS `execGit(command, sshHost, options)`. `ssh_host`가 없으면 로컬 셸에서 돌린다.
/// 로컬 실행은 `createLocalShellEnvironment()`처럼 좁힌 환경으로 돌려야 한다(CLAUDE.md 환경 규칙)
pub trait CommandRunner: Send + Sync {
    fn run(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: CommandOptions,
    ) -> AiResult<String>;
}

/// 명령을 돌릴 수 없는 호스트. 파일만 읽는 로컬 조회에서 명령이 필요해지면 실패로 알린다
pub struct UnavailableCommandRunner;

impl CommandRunner for UnavailableCommandRunner {
    fn run(
        &self,
        command: &str,
        _ssh_host: Option<&str>,
        _options: CommandOptions,
    ) -> AiResult<String> {
        Err(AiError::Command {
            message: format!("명령을 실행할 수 없습니다: {command}"),
            is_connection_failure: false,
        })
    }
}

/// AI history JSONL은 일반 명령 상한 10 MiB를 넘을 수 있어 64 MiB까지 받는다
pub const REMOTE_TEXT_FILE_MAX_BUFFER_BYTES: usize = 64 * 1024 * 1024;

static REMOTE_HOME_DIRECTORIES: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 세션 기록을 읽을 호스트. 로컬이면 주입한 홈 디렉터리와 환경 변수 재정의를, 원격이면 원격 `$HOME`을 쓴다
#[derive(Clone)]
pub struct SessionHost {
    ssh_host: Option<String>,
    runner: Arc<dyn CommandRunner>,
    local_home: String,
    local_variables: Arc<BTreeMap<String, String>>,
}

impl SessionHost {
    /// 로컬 호스트. `variables`에는 provider 경로 재정의(`CODEX_HOME`, `XDG_DATA_HOME` 등)만 담는다
    pub fn local(
        home_directory: impl Into<String>,
        variables: BTreeMap<String, String>,
        runner: Arc<dyn CommandRunner>,
    ) -> Self {
        Self {
            ssh_host: None,
            runner,
            local_home: home_directory.into(),
            local_variables: Arc::new(variables),
        }
    }

    /// 같은 실행기로 원격 호스트를 본다. 빈 문자열은 로컬로 본다(TS의 falsy `sshHost`)
    pub fn with_ssh_host(&self, ssh_host: Option<&str>) -> Self {
        Self {
            ssh_host: ssh_host.filter(|host| !host.is_empty()).map(str::to_string),
            ..self.clone()
        }
    }

    pub fn ssh_host(&self) -> Option<&str> {
        self.ssh_host.as_deref()
    }

    pub fn is_remote(&self) -> bool {
        self.ssh_host.is_some()
    }

    pub fn runner(&self) -> &Arc<dyn CommandRunner> {
        &self.runner
    }

    /// 로컬에서만 적용하는 환경 변수 재정의. 원격은 항상 원격 `$HOME` 기준이라 `None`이다
    pub fn variable(&self, name: &str) -> Option<&str> {
        if self.is_remote() {
            return None;
        }
        self.local_variables.get(name).map(String::as_str)
    }

    /// 이 호스트에서 명령을 돌린다
    pub fn run(&self, command: &str, options: CommandOptions) -> AiResult<String> {
        self.runner.run(command, self.ssh_host(), options)
    }

    /// TS `getHomeDirectory`. 원격 HOME은 호스트별로 한 번만 묻는다
    pub fn home_directory(&self) -> AiResult<String> {
        let Some(ssh_host) = self.ssh_host() else {
            return Ok(self.local_home.clone());
        };
        if let Some(cached) = REMOTE_HOME_DIRECTORIES
            .lock()
            .expect("홈 캐시")
            .get(ssh_host)
        {
            return Ok(cached.clone());
        }
        let output = self.run("printf '%s' \"$HOME\"", CommandOptions::default())?;
        let home_directory = crate::javascript::js_trim(&output).to_string();
        if home_directory.is_empty() {
            return Err(AiError::message(format!(
                "{ssh_host} 원격 HOME 디렉토리를 확인할 수 없습니다."
            )));
        }
        REMOTE_HOME_DIRECTORIES
            .lock()
            .expect("홈 캐시")
            .insert(ssh_host.to_string(), home_directory.clone());
        Ok(home_directory)
    }

    /// TS `pathExists`
    pub fn path_exists(&self, target_path: &str) -> AiResult<bool> {
        if !self.is_remote() {
            return Ok(Path::new(target_path).exists());
        }
        let output = self.run(
            &format!(
                "test -e {} && printf '1' || true",
                quote_shell_argument(target_path)
            ),
            CommandOptions::default(),
        )?;
        Ok(crate::javascript::js_trim(&output) == "1")
    }

    /// TS `readTextFile`. 로컬에서 읽지 못하면 빈 문자열이다
    pub fn read_text_file(&self, target_path: &str) -> AiResult<String> {
        if !self.is_remote() {
            return Ok(std::fs::read(target_path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default());
        }
        let quoted = quote_shell_argument(target_path);
        self.run(
            &format!("test -f {quoted} && cat {quoted} || true"),
            CommandOptions {
                max_buffer_bytes: Some(REMOTE_TEXT_FILE_MAX_BUFFER_BYTES),
                timeout_ms: None,
            },
        )
    }

    /// TS `readDirectoryFilesBySuffix`. 로컬은 디렉터리 순서 그대로, 원격은 `sort` 순서다
    pub fn read_directory_files_by_suffix(
        &self,
        directory_path: &str,
        suffix: &str,
    ) -> AiResult<Vec<String>> {
        if !self.is_remote() {
            return Ok(sorted_directory_entries(directory_path)
                .into_iter()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
                .filter(|name| name.ends_with(suffix))
                .map(|name| node_path::join(&[directory_path, &name]))
                .collect());
        }
        let quoted = quote_shell_argument(directory_path);
        let pattern = quote_shell_argument(&format!("*{suffix}"));
        let output = self.run(
            &format!("test -d {quoted} && find {quoted} -maxdepth 1 -type f -name {pattern} | sort || true"),
            CommandOptions::default(),
        )?;
        Ok(non_empty_trimmed_lines(&output))
    }

    /// TS `listFilesRecursivelyBySuffix`. 심볼릭 링크 디렉터리는 따라가지 않는다
    pub fn list_files_recursively_by_suffix(
        &self,
        root_path: &str,
        suffix: &str,
    ) -> AiResult<Vec<String>> {
        if !self.is_remote() {
            let mut files = Vec::new();
            collect_local_files_by_suffix(root_path, suffix, &mut files);
            return Ok(files);
        }
        let quoted = quote_shell_argument(root_path);
        let pattern = quote_shell_argument(&format!("*{suffix}"));
        let output = self.run(
            &format!("test -d {quoted} && find {quoted} -type f -name {pattern} | sort || true"),
            CommandOptions::default(),
        )?;
        Ok(non_empty_trimmed_lines(&output))
    }

    /// TS `listFilesModifiedWithin`. 원격은 `find -mmin`으로 거르고 시각까지 한 번에 받는다
    pub fn list_files_modified_within(
        &self,
        root_path: &str,
        suffix: &str,
        within_ms: f64,
        now_ms: f64,
    ) -> AiResult<Vec<ModifiedFile>> {
        let oldest_allowed_ms = now_ms - within_ms;
        if !self.is_remote() {
            let files = self.list_files_recursively_by_suffix(root_path, suffix)?;
            return Ok(files
                .into_iter()
                .filter_map(|file_path| {
                    let mtime_ms = local_mtime_ms(&file_path)?;
                    (mtime_ms >= oldest_allowed_ms).then_some(ModifiedFile {
                        file_path,
                        mtime_ms,
                    })
                })
                .collect());
        }
        let within_minutes = (within_ms / 60_000.0).ceil().max(1.0) as u64;
        let quoted = quote_shell_argument(root_path);
        let pattern = quote_shell_argument(&format!("*{suffix}"));
        let output = self.run(
            &format!(
                "test -d {quoted} && find {quoted} -type f -name {pattern} -mmin -{within_minutes} -exec sh -c 'for f; do printf \"%s\\t%s\\n\" \"$(stat -c %Y \"$f\" 2>/dev/null || stat -f %m \"$f\" 2>/dev/null)\" \"$f\"; done' sh {{}} + || true"
            ),
            CommandOptions::default(),
        )?;
        Ok(non_empty_trimmed_lines(&output)
            .iter()
            .filter_map(|line| parse_remote_modified_file(line))
            .filter(|record| record.mtime_ms >= oldest_allowed_ms)
            .collect())
    }

    /// TS `getFileMtimeMs`. 로컬은 소수 밀리초까지, 원격은 초 단위다
    pub fn file_mtime_ms(&self, file_path: &str) -> AiResult<Option<f64>> {
        if !self.is_remote() {
            return Ok(local_mtime_ms(file_path));
        }
        let quoted = quote_shell_argument(file_path);
        let output = self.run(
            &format!("test -e {quoted} && (stat -c %Y {quoted} 2>/dev/null || stat -f %m {quoted} 2>/dev/null) || true"),
            CommandOptions::default(),
        )?;
        Ok(parse_js_int(crate::javascript::js_trim(&output)).map(|seconds| seconds * 1000.0))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModifiedFile {
    pub file_path: String,
    pub mtime_ms: f64,
}

/// TS `quoteShellArgument`
pub fn quote_shell_argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Node `stat().mtimeMs`
pub fn local_mtime_ms(file_path: &str) -> Option<f64> {
    let metadata = std::fs::metadata(file_path).ok()?;
    Some(metadata.mtime() as f64 * 1000.0 + metadata.mtime_nsec() as f64 / 1_000_000.0)
}

/// Node `fs.readdir`는 libuv scandir가 이름을 바이트 순으로 정렬해 돌려준다. 같은 순서를 지키도록 정렬한다
fn sorted_directory_entries(directory_path: &str) -> Vec<std::fs::DirEntry> {
    let Ok(entries) = std::fs::read_dir(directory_path) else {
        return Vec::new();
    };
    let mut entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    entries
}

fn collect_local_files_by_suffix(root_path: &str, suffix: &str, files: &mut Vec<String>) {
    for entry in sorted_directory_entries(root_path) {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let entry_path = node_path::join(&[root_path, &name]);
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            collect_local_files_by_suffix(&entry_path, suffix, files);
        } else if name.ends_with(suffix) {
            files.push(entry_path);
        }
    }
}

fn non_empty_trimmed_lines(output: &str) -> Vec<String> {
    output
        .split('\n')
        .map(crate::javascript::js_trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_remote_modified_file(line: &str) -> Option<ModifiedFile> {
    let (seconds_text, file_path) = line.split_once('\t').unwrap_or(("", line));
    let seconds = parse_js_int(seconds_text)?;
    (!file_path.is_empty()).then(|| ModifiedFile {
        file_path: file_path.to_string(),
        mtime_ms: seconds * 1000.0,
    })
}

/// JS `Number.parseInt(value, 10)`. 앞 공백을 건너뛰고 숫자가 이어지는 데까지만 읽는다
pub fn parse_js_int(value: &str) -> Option<f64> {
    let trimmed = crate::javascript::js_trim_start(value);
    let (sign, digits_source) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let digits: String = digits_source
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let number: f64 = digits.parse().ok()?;
    Some(sign * number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_js_int_reads_leading_digits_only() {
        assert_eq!(parse_js_int(" 1700000000\n"), Some(1_700_000_000.0));
        assert_eq!(parse_js_int("12abc"), Some(12.0));
        assert_eq!(parse_js_int("-3"), Some(-3.0));
        assert_eq!(parse_js_int(""), None);
        assert_eq!(parse_js_int("abc"), None);
    }

    #[test]
    fn quotes_single_quotes_for_posix_shells() {
        assert_eq!(
            quote_shell_argument("/tmp/a'b.jsonl"),
            "'/tmp/a'\\''b.jsonl'"
        );
    }
}
