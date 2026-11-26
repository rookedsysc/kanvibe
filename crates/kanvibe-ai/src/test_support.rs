//! 테스트 전용 호스트. 실제 홈 디렉터리를 읽지 않도록 임시 디렉터리를 홈으로 주입하고,
//! 원격 호스트는 TS 테스트의 `setupRemoteFileSystem`처럼 명령 문자열을 보고 메모리 파일로 답한다.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use regex::Regex;

use crate::error::{AiError, AiResult};
use crate::host::{CommandOptions, CommandRunner, SessionHost, UnavailableCommandRunner};
use crate::node_path;

/// 주입한 홈과 환경 변수만 보는 로컬 호스트
pub fn local_host(home_directory: &str) -> SessionHost {
    local_host_with(home_directory, &[])
}

pub fn local_host_with(home_directory: &str, variables: &[(&str, &str)]) -> SessionHost {
    let variables = variables
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    SessionHost::local(
        home_directory,
        variables,
        Arc::new(UnavailableCommandRunner),
    )
}

/// 임시 홈 디렉터리. 테스트가 끝나면 지운다
pub fn temp_home() -> (tempfile::TempDir, String) {
    let directory = tempfile::tempdir().expect("임시 디렉터리");
    let path = directory
        .path()
        .canonicalize()
        .expect("임시 경로")
        .to_string_lossy()
        .into_owned();
    (directory, path)
}

pub fn write_file(file_path: &str, content: &str) {
    std::fs::create_dir_all(node_path::dirname(file_path)).expect("디렉터리 만들기");
    std::fs::write(file_path, content).expect("파일 쓰기");
}

/// TS 테스트의 `jsonLines(values)`
pub fn json_lines(values: &[serde_json::Value]) -> String {
    let lines: Vec<String> = values.iter().map(crate::javascript::stringify).collect();
    format!("{}\n", lines.join("\n"))
}

/// 받은 명령 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedCommand {
    pub command: String,
    pub ssh_host: Option<String>,
    pub options: CommandOptions,
}

type CommandResponder = dyn Fn(&str, Option<&str>) -> AiResult<String> + Send + Sync;

/// 명령을 기록하고 주어진 함수로 답하는 실행기
pub struct ScriptedRunner {
    responder: Box<CommandResponder>,
    pub calls: Mutex<Vec<RecordedCommand>>,
}

impl ScriptedRunner {
    pub fn new(
        responder: impl Fn(&str, Option<&str>) -> AiResult<String> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            responder: Box::new(responder),
            calls: Mutex::new(Vec::new()),
        })
    }

    pub fn commands(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("명령 기록")
            .iter()
            .map(|call| call.command.clone())
            .collect()
    }
}

impl CommandRunner for ScriptedRunner {
    fn run(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: CommandOptions,
    ) -> AiResult<String> {
        self.calls.lock().expect("명령 기록").push(RecordedCommand {
            command: command.to_string(),
            ssh_host: ssh_host.map(str::to_string),
            options,
        });
        (self.responder)(command, ssh_host)
    }
}

pub fn remote_host(runner: Arc<ScriptedRunner>, ssh_host: &str) -> SessionHost {
    SessionHost::local("/unused-local-home", BTreeMap::new(), runner).with_ssh_host(Some(ssh_host))
}

/// TS `setupRemoteFileSystem`. 원격 HOME은 `/remote/home`, 파일 수정 시각은 모두 1700000000초다
pub fn remote_file_system(files: &[(&str, &str)]) -> Arc<ScriptedRunner> {
    let file_map: BTreeMap<String, String> = files
        .iter()
        .map(|(path, content)| (path.to_string(), content.to_string()))
        .collect();
    let mut directories = BTreeSet::new();
    for file_path in file_map.keys() {
        let mut current = node_path::dirname(file_path);
        while !current.is_empty() && current != node_path::dirname(&current) {
            directories.insert(current.clone());
            current = node_path::dirname(&current);
        }
    }
    ScriptedRunner::new(move |command, _| answer_remote_command(command, &file_map, &directories))
}

fn answer_remote_command(
    command: &str,
    files: &BTreeMap<String, String>,
    directories: &BTreeSet<String>,
) -> AiResult<String> {
    let exists = |path: &str| files.contains_key(path) || directories.contains(path);
    if command == "printf '%s' \"$HOME\"" {
        return Ok("/remote/home".to_string());
    }
    let find = Regex::new(
        r"test -d '([^']+)' && find '([^']+)' (?:-maxdepth 1 )?-type f -name '\*([^']+)'",
    )
    .unwrap();
    if let Some(found) = find.captures(command) {
        let is_recursive = !command.contains(" -maxdepth 1 ");
        return Ok(list_remote_files(files, &found[1], &found[3], is_recursive));
    }
    let cat = Regex::new(r"test -f '([^']+)' && cat '([^']+)' \|\| true").unwrap();
    if let Some(found) = cat.captures(command) {
        return Ok(files.get(&found[1]).cloned().unwrap_or_default());
    }
    let head = Regex::new(r"head -n (\d+) '([^']+)'").unwrap();
    if let Some(found) = head.captures(command) {
        let count: usize = found[1].parse().unwrap();
        let content = files.get(&found[2]).cloned().unwrap_or_default();
        return Ok(content
            .split('\n')
            .take(count)
            .collect::<Vec<_>>()
            .join("\n"));
    }
    let tail = Regex::new(r"tail -n (\d+) '([^']+)'").unwrap();
    if let Some(found) = tail.captures(command) {
        let count: usize = found[1].parse().unwrap();
        let content = files.get(&found[2]).cloned().unwrap_or_default();
        let lines: Vec<&str> = content.split('\n').collect();
        let start = lines.len().saturating_sub(count + 1);
        return Ok(lines[start..].join("\n"));
    }
    let stat = Regex::new(r"test -e '([^']+)' && \(stat -c %Y '([^']+)'").unwrap();
    if let Some(found) = stat.captures(command) {
        return Ok(if exists(&found[1]) { "1700000000" } else { "" }.to_string());
    }
    let exists_pattern = Regex::new(r"test -e '([^']+)' && printf '1' \|\| true").unwrap();
    if let Some(found) = exists_pattern.captures(command) {
        return Ok(if exists(&found[1]) { "1" } else { "" }.to_string());
    }
    Err(AiError::message(format!(
        "unexpected remote command: {command}"
    )))
}

fn list_remote_files(
    files: &BTreeMap<String, String>,
    root: &str,
    suffix: &str,
    is_recursive: bool,
) -> String {
    files
        .keys()
        .filter(|path| path.ends_with(suffix))
        .filter(|path| {
            if is_recursive {
                path.starts_with(&format!("{root}/"))
            } else {
                node_path::dirname(path) == root
            }
        })
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}
