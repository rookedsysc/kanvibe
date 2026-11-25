//! pane 입력을 소유한 CLI 프로세스 찾기. `ps` 출력에서 pane 셸의 foreground 그룹 아래 에이전트 CLI를 고른다.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::javascript::{JS_WHITESPACE_CLASS, js_trim};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatProvider {
    Claude,
    Codex,
    Opencode,
    Gemini,
}

impl PaneChatProvider {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::Opencode),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatProcess {
    pub pid: i64,
    pub process_start_time: String,
    pub provider: PaneChatProvider,
    /// 명시된 Unix app-server 주소. `--remote`가 없으면 필드 자체가 없고, 다른 종류의 주소이면 `Some(None)`(null)이며 기본 소켓으로 대체하지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_server_socket: Option<Option<String>>,
}

struct ProcessRow {
    pid: i64,
    parent_pid: i64,
    group: i64,
    foreground_group: i64,
    tty: String,
    process_start_time: String,
    command: String,
    args: String,
}

fn compile(pattern: &str) -> Regex {
    Regex::new(
        &pattern
            .replace("\\S", &format!("[^{JS_WHITESPACE_CLASS}]"))
            .replace("\\s", &format!("[{JS_WHITESPACE_CLASS}]")),
    )
    .expect("프로세스 패턴")
}

static INTERPRETERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:node|nodejs|bun|python(?:[0-9]+(?:\.[0-9]+)?)?|ruby|bash|sh|zsh)$")
        .expect("인터프리터 패턴")
});
static ARGUMENT_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| compile(r#""(?:[^"\\]|\\[^\n\r\x{2028}\x{2029}])*"|'[^']*'|\S+"#));
static TOKEN_QUOTES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^["']|["']$"#).expect("따옴표 패턴"));
static EVAL_OPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:-[epc][^\n\r\x{2028}\x{2029}]*|--(?:eval|print|command)(?:=[^\n\r\x{2028}\x{2029}]*)?)$").expect("eval 옵션 패턴")
});
static VALUE_OPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:-r|--require|--import|--loader|--experimental-loader|--max-old-space-size|--max_old_space_size)$").expect("값 옵션 패턴")
});
static FIRST_TOKEN: LazyLock<Regex> = LazyLock::new(|| compile(r"^\S+"));
static VERSION_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$").expect("버전 이름 패턴"));
static CLAUDE_EXECUTABLE_PATH: LazyLock<Regex> =
    LazyLock::new(|| compile(r"^/(?:[^/\s]+/)*claude$"));
static CLAUDE_CODE_SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/@anthropic-ai/claude-code/[^\n\r\x{2028}\x{2029}]*(?:cli|index)\.js$")
        .expect("Claude 스크립트 패턴")
});
static GEMINI_SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/@google/gemini-cli/(?:[^\n\r\x{2028}\x{2029}]*/)?(?:cli|index|gemini)\.js$")
        .expect("Gemini 스크립트 패턴")
});
static CODEX_SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/@openai/codex/[^\n\r\x{2028}\x{2029}]*codex\.js$").expect("Codex 스크립트 패턴")
});
static PS_FIELD_GAP: LazyLock<Regex> = LazyLock::new(|| compile(r"\s+"));
static SIGNED_INTEGER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^-?[0-9]+$").expect("정수 패턴"));
static NODE_COMMAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:node|nodejs)$").expect("node 패턴"));
static NODE_COMMAND_IGNORING_CASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:node|nodejs)$").expect("node 패턴"));
static MAX_OLD_SPACE_OPTION: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?:^|\s)--max-old-space-size(?:=|\s)"));
static KNOWN_CODEX_WRAPPER: LazyLock<Regex> =
    LazyLock::new(|| compile(r"/@openai/codex/(?:bin/)?codex\.js(?:\s|$)"));
static KNOWN_CODEX_NATIVE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?:^|\s)\S*/@openai/codex(?:-[^/]+)?/\S*/codex(?:\.exe)?(?:\s|$)"));
static CODEX_REMOTE_OPTION: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?:^|\s)--remote(?:=|\s+)(\S+)"));
static EXE_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.exe$").expect("exe 패턴"));

/// 경로의 마지막 조각(JS `split("/").at(-1)`)
fn last_path_part(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or_default()
}

/// 인터프리터 명령줄에서 실행하는 스크립트. eval로 넘긴 코드나 옵션 값은 스크립트로 보지 않는다
fn interpreter_script(args: &str) -> String {
    let tokens: Vec<String> = ARGUMENT_TOKEN
        .find_iter(args)
        .map(|token| TOKEN_QUOTES.replace_all(token.as_str(), "").into_owned())
        .collect();
    let mut index = 1;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "--" {
            return tokens.get(index + 1).cloned().unwrap_or_default();
        }
        if EVAL_OPTION.is_match(token) {
            return String::new();
        }
        if VALUE_OPTION.is_match(token) {
            index += 2;
            continue;
        }
        if !token.starts_with('-') {
            return token.clone();
        }
        index += 1;
    }
    String::new()
}

fn detect_provider(command: &str, args: &str) -> Option<PaneChatProvider> {
    let executable = EXE_SUFFIX
        .replace(last_path_part(command), "")
        .to_lowercase();
    if let Some(provider) = PaneChatProvider::parse(&executable) {
        return Some(provider);
    }
    let native_executable = FIRST_TOKEN.find(args).map_or("", |found| found.as_str());
    // 공식 native 설치는 ucomm에 버전 파일명이 드러나므로 argv의 실행 경로까지 확인한다
    let is_native_claude = VERSION_NAME.is_match(&executable)
        && (native_executable == "claude"
            || CLAUDE_EXECUTABLE_PATH.is_match(native_executable)
            || native_executable.ends_with(&format!("/.local/share/claude/versions/{executable}")));
    if is_native_claude {
        return Some(PaneChatProvider::Claude);
    }
    if !INTERPRETERS.is_match(&executable) {
        return None;
    }
    let script = interpreter_script(args);
    if let Some(provider) = PaneChatProvider::parse(last_path_part(&script)) {
        return Some(provider);
    }
    if CLAUDE_CODE_SCRIPT.is_match(&script) {
        Some(PaneChatProvider::Claude)
    } else if GEMINI_SCRIPT.is_match(&script) {
        Some(PaneChatProvider::Gemini)
    } else if CODEX_SCRIPT.is_match(&script) {
        Some(PaneChatProvider::Codex)
    } else {
        None
    }
}

pub fn is_pane_chat_provider_process(
    command: &str,
    args: &str,
    provider: PaneChatProvider,
) -> bool {
    detect_provider(command, args) == Some(provider)
}

fn parse_process_rows(output: &str) -> Vec<ProcessRow> {
    output
        .split('\n')
        .filter_map(|line| {
            let fields: Vec<&str> = PS_FIELD_GAP.split(js_trim(line)).collect();
            let has_numeric_head = fields.len() >= 11
                && fields[..4]
                    .iter()
                    .all(|field| SIGNED_INTEGER.is_match(field));
            has_numeric_head.then(|| ProcessRow {
                pid: parse_integer(fields[0]),
                parent_pid: parse_integer(fields[1]),
                group: parse_integer(fields[2]),
                foreground_group: parse_integer(fields[3]),
                tty: fields[4].to_string(),
                process_start_time: fields[5..10].join(" "),
                command: fields[10].to_string(),
                args: fields[11..].join(" "),
            })
        })
        .collect()
}

fn parse_integer(field: &str) -> i64 {
    field.parse().unwrap_or(0)
}

/// `ps -o pid=,ppid=,pgid=,tpgid=,tty=,lstart=,ucomm=,args=` 출력에서 pane 셸(`root_pid`)의 입력을 받는 CLI를 고른다.
/// 후보가 모호하면 `None`이다
pub fn find_pane_chat_process(output: &str, root_pid: i64) -> Option<PaneChatProcess> {
    let rows = parse_process_rows(output);
    let by_pid: HashMap<i64, &ProcessRow> = rows.iter().map(|row| (row.pid, row)).collect();
    let root = by_pid.get(&root_pid)?;
    if root.tty == "??" || root.tty == "?" || root.foreground_group < 1 {
        return None;
    }
    let candidates: Vec<&ProcessRow> = rows
        .iter()
        .filter(|row| {
            row.tty == root.tty
                && row.group == root.foreground_group
                && is_descendant(&by_pid, row.pid, root_pid)
                && detect_provider(&row.command, &row.args).is_some()
        })
        .collect();
    let providers: HashSet<Option<PaneChatProvider>> = candidates
        .iter()
        .map(|row| detect_provider(&row.command, &row.args))
        .collect();
    if providers.len() != 1 {
        return None;
    }
    let row = match candidates.as_slice() {
        [single] => *single,
        _ => return pick_wrapped_process(&candidates),
    };
    Some(to_pane_chat_process(row))
}

fn is_descendant(by_pid: &HashMap<i64, &ProcessRow>, pid: i64, ancestor: i64) -> bool {
    let mut visited = HashSet::new();
    let mut current = pid;
    while current > 0 && visited.insert(current) {
        if current == ancestor {
            return true;
        }
        current = by_pid.get(&current).map_or(0, |row| row.parent_pid);
    }
    false
}

/// 후보가 둘 이상이면 공식 wrapper와 그 직접 자식인 실제 CLI 한 쌍만 받아들인다.
/// npm wrapper만 stdin을 native 자식에게 잇는다. 에이전트가 실행한 동종 CLI는 입력 소유자가 아니다
fn pick_wrapped_process(candidates: &[&ProcessRow]) -> Option<PaneChatProcess> {
    let is_node = |row: &ProcessRow| NODE_COMMAND.is_match(last_path_part(&row.command));
    let gemini_wrapper = candidates.iter().find(|wrapper| {
        is_node(wrapper)
            && candidates.iter().any(|child| {
                child.parent_pid == wrapper.pid
                    && detect_provider(&child.command, &child.args)
                        == Some(PaneChatProvider::Gemini)
                    && MAX_OLD_SPACE_OPTION.is_match(&child.args)
                    && interpreter_script(&child.args) == interpreter_script(&wrapper.args)
            })
    });
    if let Some(wrapper) = gemini_wrapper.filter(|wrapper| {
        candidates.len() == 2
            && detect_provider(&wrapper.command, &wrapper.args) == Some(PaneChatProvider::Gemini)
    }) {
        let child = candidates
            .iter()
            .find(|candidate| candidate.parent_pid == wrapper.pid)?;
        return Some(PaneChatProcess {
            pid: child.pid,
            process_start_time: child.process_start_time.clone(),
            provider: PaneChatProvider::Gemini,
            codex_server_socket: None,
        });
    }
    let wrapper = candidates.iter().find(|candidate| {
        NODE_COMMAND_IGNORING_CASE.is_match(last_path_part(&candidate.command))
    })?;
    let native = candidates.iter().find(|candidate| {
        candidate.parent_pid == wrapper.pid && last_path_part(&candidate.command) == "codex"
    })?;
    let is_known_pair =
        KNOWN_CODEX_WRAPPER.is_match(&wrapper.args) || KNOWN_CODEX_NATIVE.is_match(&native.args);
    (candidates.len() == 2 && is_known_pair).then(|| to_pane_chat_process(native))
}

fn to_pane_chat_process(row: &ProcessRow) -> PaneChatProcess {
    let provider = detect_provider(&row.command, &row.args).expect("후보는 provider가 있다");
    let remote = (provider == PaneChatProvider::Codex)
        .then(|| {
            CODEX_REMOTE_OPTION
                .captures(&row.args)
                .map(|found| found[1].to_string())
        })
        .flatten();
    let codex_server_socket = remote.map(|remote| {
        remote
            .strip_prefix("unix://")
            .filter(|_| remote.starts_with("unix:///"))
            .map(str::to_string)
    });
    PaneChatProcess {
        pid: row.pid,
        process_start_time: row.process_start_time.clone(),
        provider,
        codex_server_socket,
    }
}

#[cfg(test)]
mod tests;
