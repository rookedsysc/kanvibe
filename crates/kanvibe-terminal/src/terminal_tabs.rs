//! tmux window와 zellij tab을 KanVibe 탭 바가 쓰는 모양으로 바꾸는 명령 빌더와 출력 파서(TS `src/lib/terminalTabs.ts`).
//!
//! 세션 생성·정리는 worktree 쪽이 담당하고 이 파일은 탭 축만 다룬다.
//! 두 축은 변경 출처가 달라서(세션 수명 vs 탭 조작) 같은 파일에 두면 함께 커진다.
//!
//! 명령 빌더와 파서는 부수효과가 없다. 실제 실행은 터미널 탭 서비스가 맡고,
//! 버전 판정만 예외로 [`ZellijIdTargetingSupportCache`]가 실행기를 인자로 받아 답을 캐시한다.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use kanvibe_ai::javascript::{js_string, js_trim, js_trim_start};
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_process::exec::{ExecGitOptions, ShellCommandRunner};
use kanvibe_process::worktree::quote_for_posix_shell;
use regex::Regex;
use serde_json::Value;

#[cfg(test)]
mod tests;

/// 탭 이름을 마지막 필드에 두고 탭 문자로 구분한다.
/// tmux는 format 출력에서 이름의 제어문자를 이스케이프하므로 이름 안에 실제 탭 문자는 들어오지 않지만,
/// 파서는 남는 조각을 이름으로 되붙여 그 가정이 깨져도 이름이 잘리지 않게 한다.
const TMUX_WINDOW_LIST_FORMAT: &str =
    "#{window_id}\t#{window_index}\t#{window_active}\t#{window_name}";

const TMUX_WINDOW_LIST_FIELD_COUNT: usize = 4;

/// tmux 서버 전체의 pane을 한 번에 조회하는 형식.
/// 세션을 지정하지 않는 이유는, 어떤 worktree에서 어떤 에이전트가 도는지 알아내려면
/// 태스크별로 세션을 하나씩 물어보는 대신 서버 전체를 한 번만 훑는 편이 싸기 때문이다.
const TMUX_PANE_LIST_FORMAT: &str = "#{session_name}\t#{window_id}\t#{window_index}\t#{pane_current_command}\t#{pane_current_path}\t#{window_name}";

const TMUX_PANE_LIST_FIELD_COUNT: usize = 6;

/// 탭 id·이름·활성 여부를 한 번에 주는 `list-tabs --json`은 zellij 0.44.0에서 추가됐다.
/// 그 이전 버전은 이름 목록과 레이아웃 덤프를 합쳐 같은 정보를 만든다.
const ZELLIJ_TAB_ID_MINIMUM_VERSION: [f64; 3] = [0.0, 44.0, 0.0];

/// JS `\d`는 ASCII 숫자만 뜻하므로 `[0-9]`로 쓴다
static ZELLIJ_VERSION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([0-9]+)\.([0-9]+)\.([0-9]+)").expect("zellij 버전 패턴"));

/// JS 정규식의 `.`는 줄 끝 문자(`\n`, `\r`, U+2028, U+2029)를 받지 않고, `\b`는 ASCII 단어 경계다
static ZELLIJ_TAB_NODE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*tab(?-u:\b)([^\n\r\u{2028}\u{2029}]*)$").expect("zellij tab 노드 패턴")
});

static ZELLIJ_FOCUS_ATTRIBUTE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)focus=true(?-u:\b)").expect("zellij focus 속성 패턴"));

static ZELLIJ_NAME_ATTRIBUTE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?-u:\b)name="((?:[^"\\]|\\[^\n\r\u{2028}\u{2029}])*)""#)
        .expect("zellij name 속성 패턴")
});

static ZELLIJ_ESCAPED_CHARACTER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\([^\n\r\u{2028}\u{2029}])").expect("zellij 이스케이프 패턴"));

/// tmux pane 하나가 어느 세션·window에서 무슨 명령을 어떤 경로로 돌리는지
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPaneSnapshot {
    pub session_name: String,
    pub window_id: String,
    pub window_index: i64,
    pub window_name: String,
    pub command: String,
    pub current_path: String,
}

/// tmux window 목록을 조회하는 명령
pub fn build_tmux_list_windows_command(session_name: &str) -> String {
    [
        "tmux list-windows -t".to_string(),
        quote_for_posix_shell(session_name),
        "-F".to_string(),
        quote_for_posix_shell(TMUX_WINDOW_LIST_FORMAT),
    ]
    .join(" ")
}

/// [`build_tmux_list_windows_command`] 출력을 탭 목록으로 바꾼다
pub fn parse_tmux_window_list(list_windows_output: &str) -> Vec<TerminalTab> {
    split_output_lines(list_windows_output)
        .filter_map(parse_tmux_window_line)
        .collect()
}

fn parse_tmux_window_line(line: &str) -> Option<TerminalTab> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() < TMUX_WINDOW_LIST_FIELD_COUNT {
        return None;
    }

    let window_id = fields[0];
    let parsed_index = parse_js_integer(fields[1])?;
    if window_id.is_empty() {
        return None;
    }

    Some(TerminalTab {
        id: window_id.to_string(),
        native_index: parsed_index,
        name: fields[3..].join("\t"),
        is_active: fields[2] == "1",
    })
}

/// tmux 서버의 모든 pane을 조회하는 명령. 세션이 하나도 없으면 tmux가 실패하므로 호출자가 빈 출력으로 다뤄야 한다
pub fn build_tmux_list_panes_command() -> String {
    format!(
        "tmux list-panes -a -F {}",
        quote_for_posix_shell(TMUX_PANE_LIST_FORMAT)
    )
}

/// [`build_tmux_list_panes_command`] 출력을 pane 목록으로 바꾼다
pub fn parse_tmux_pane_list(list_panes_output: &str) -> Vec<TmuxPaneSnapshot> {
    split_output_lines(list_panes_output)
        .filter_map(parse_tmux_pane_line)
        .collect()
}

fn parse_tmux_pane_line(line: &str) -> Option<TmuxPaneSnapshot> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() < TMUX_PANE_LIST_FIELD_COUNT {
        return None;
    }

    let window_id = fields[1];
    let current_path = fields[4];
    let parsed_index = parse_js_integer(fields[2])?;
    if window_id.is_empty() || current_path.is_empty() {
        return None;
    }

    Some(TmuxPaneSnapshot {
        session_name: fields[0].to_string(),
        window_id: window_id.to_string(),
        window_index: parsed_index,
        window_name: fields[5..].join("\t"),
        command: fields[3].to_string(),
        current_path: current_path.to_string(),
    })
}

/// 줄마다 끝의 `\r` 하나를 떼고 빈 줄은 건너뛴다(JS `split("\n")` 뒤 `replace(/\r$/, "")`)
fn split_output_lines(output: &str) -> impl Iterator<Item = &str> {
    output
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty())
}

/// JS `Number.parseInt(text, 10)`이 정수를 낼 때만 값을 준다. 앞 공백과 부호를 받고 숫자가 아닌 글자에서 멈춘다
fn parse_js_integer(text: &str) -> Option<i64> {
    let trimmed = js_trim_start(text);
    let (sign, unsigned) = match trimmed.as_bytes().first() {
        Some(b'-') => (-1, &trimmed[1..]),
        Some(b'+') => (1, &trimmed[1..]),
        _ => (1, trimmed),
    };
    let digit_count = unsigned.bytes().take_while(u8::is_ascii_digit).count();
    let magnitude: i64 = unsigned[..digit_count].parse().ok()?;
    Some(sign * magnitude)
}

/// tmux window id는 서버 전역에서 유일하므로 세션 접두사 없이 그대로 대상으로 쓴다.
/// 인덱스를 쓰면 다른 window가 닫힐 때 번호가 밀려 엉뚱한 탭을 조작하게 된다.
pub fn build_tmux_select_window_command(window_id: &str) -> String {
    format!("tmux select-window -t {}", quote_for_posix_shell(window_id))
}

/// 세션 뒤의 콜론은 "이 세션의 새 window"를 뜻한다
pub fn build_tmux_new_window_command(session_name: &str, working_dir: Option<&str>) -> String {
    let target = format!("{}:", quote_for_posix_shell(session_name));
    let create_argument = format!(
        "tmux new-window -t {target} -P -F {}",
        quote_for_posix_shell("#{window_id}")
    );
    match working_dir.filter(|directory| !directory.is_empty()) {
        Some(directory) => format!("{create_argument} -c {}", quote_for_posix_shell(directory)),
        None => create_argument,
    }
}

pub fn build_tmux_kill_window_command(window_id: &str) -> String {
    format!("tmux kill-window -t {}", quote_for_posix_shell(window_id))
}

pub fn build_tmux_rename_window_command(window_id: &str, name: &str) -> String {
    format!(
        "tmux rename-window -t {} -- {}",
        quote_for_posix_shell(window_id),
        quote_for_posix_shell(name)
    )
}

/// window를 목표 인덱스로 옮긴다.
///
/// `-k`는 쓰지 않는다. tmux에서 `-k`는 자리를 비켜 주는 옵션이 아니라 목표 인덱스에 있던 window를
/// 죽이는 옵션이라, 탭을 끌어다 놓을 때마다 놓인 자리의 탭과 그 안에서 돌던 프로세스가 사라진다.
/// 대신 목표 window의 앞(`-b`)이나 뒤(`-a`)에 끼워 넣어 나머지를 밀어낸다.
///
/// 인덱스는 배열 위치가 아니라 `TerminalTab::native_index`, 즉 tmux가 매긴 window_index를 받는다.
/// 끼워 넣은 뒤 남는 번호 구멍은 `-r`로 다시 채워 탭 순서와 인덱스를 일치시킨다.
pub fn build_tmux_move_window_commands(
    session_name: &str,
    window_id: &str,
    current_window_index: i64,
    target_window_index: i64,
) -> Vec<String> {
    if current_window_index == target_window_index {
        return Vec::new();
    }

    let quoted_session_name = quote_for_posix_shell(session_name);
    let insert_side = if target_window_index > current_window_index {
        "-a"
    } else {
        "-b"
    };

    vec![
        format!(
            "tmux move-window {insert_side} -s {} -t {quoted_session_name}:{target_window_index}",
            quote_for_posix_shell(window_id)
        ),
        format!("tmux move-window -r -t {quoted_session_name}"),
    ]
}

/// zellij는 CLI 이벤트 스트림이 없어 조회도 명령 실행으로만 가능하다.
/// 세션 지정은 `zellij --session <name> action ...` 형태다.
fn build_zellij_action_command(session_name: &str, action_arguments: &str) -> String {
    format!(
        "zellij --session {} action {action_arguments}",
        quote_for_posix_shell(session_name)
    )
}

pub fn build_zellij_version_command() -> String {
    "zellij --version".to_string()
}

/// `zellij --version` 출력이 탭 id CLI를 지원하는 버전인지 판정한다. 판정 불가면 구버전으로 본다
pub fn supports_zellij_tab_id_commands(version_output: &str) -> bool {
    let Some(version_match) = ZELLIJ_VERSION_PATTERN.captures(version_output) else {
        return false;
    };

    // JS `parseInt`처럼 아주 큰 자릿수도 크기 비교가 되도록 실수로 읽는다
    let parsed_version: Vec<f64> = (1..=3)
        .map(|group| version_match[group].parse().unwrap_or(f64::INFINITY))
        .collect();

    for (parsed, minimum) in parsed_version.iter().zip(ZELLIJ_TAB_ID_MINIMUM_VERSION) {
        if *parsed != minimum {
            return *parsed > minimum;
        }
    }

    true
}

pub fn build_zellij_list_tabs_command(session_name: &str) -> String {
    build_zellij_action_command(session_name, "list-tabs --json")
}

/// `list-tabs --json` 출력을 탭 목록으로 바꾼다.
///
/// `position`은 화면 순서라 탭을 닫으면 다시 매겨지지만 `tab_id`는 고정이다.
/// `*-by-id` 명령이 받는 값도 `tab_id`이므로 식별자는 반드시 이쪽을 쓴다.
/// position을 id로 쓰면 가운데 탭을 닫은 뒤 엉뚱한 탭을 조작하게 된다.
pub fn parse_zellij_tab_list(list_tabs_json_output: &str) -> Vec<TerminalTab> {
    let Ok(Value::Array(parsed_entries)) = serde_json::from_str::<Value>(list_tabs_json_output)
    else {
        return Vec::new();
    };

    parsed_entries
        .iter()
        .filter_map(parse_zellij_tab_entry)
        .collect()
}

fn parse_zellij_tab_entry(entry: &Value) -> Option<TerminalTab> {
    let position = json_integer(entry.get("position"))?;
    let tab_id = entry.get("tab_id")?;
    json_integer(Some(tab_id))?;
    let name = entry.get("name")?.as_str()?;

    Some(TerminalTab {
        id: js_string(tab_id),
        native_index: position as i64,
        name: name.to_string(),
        is_active: entry.get("active") == Some(&Value::Bool(true)),
    })
}

/// `typeof value === "number" && Number.isInteger(value)`인 값
fn json_integer(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite() && number.fract() == 0.0)
}

pub fn build_zellij_query_tab_names_command(session_name: &str) -> String {
    build_zellij_action_command(session_name, "query-tab-names")
}

pub fn build_zellij_dump_layout_command(session_name: &str) -> String {
    build_zellij_action_command(session_name, "dump-layout")
}

/// KDL 레이아웃에서 `focus=true`가 붙은 tab 노드의 이름을 찾는다. 없으면 `None`
pub fn parse_zellij_focused_tab_name(dump_layout_output: &str) -> Option<String> {
    for line in dump_layout_output.split('\n') {
        let Some(tab_node_match) = ZELLIJ_TAB_NODE_PATTERN.captures(line) else {
            continue;
        };

        let tab_attributes = tab_node_match.get(1).map_or("", |group| group.as_str());
        if !ZELLIJ_FOCUS_ATTRIBUTE_PATTERN.is_match(tab_attributes) {
            continue;
        }

        if let Some(name_match) = ZELLIJ_NAME_ATTRIBUTE_PATTERN.captures(tab_attributes) {
            return Some(
                ZELLIJ_ESCAPED_CHARACTER_PATTERN
                    .replace_all(&name_match[1], "$1")
                    .into_owned(),
            );
        }
    }

    None
}

/// 구버전 zellij용 폴백. 이름 목록에 레이아웃 덤프의 활성 탭을 얹는다.
/// 레이아웃 덤프가 비어도 목록 자체는 쓸 수 있어야 하므로 그때는 첫 탭을 활성으로 본다.
///
/// 이름이 같은 탭은 구분하지 못한다. 이 경로에는 이름 말고 식별자가 없어서 활성 판정도,
/// 이름으로 대상을 겨냥하는 `go-to-tab-name`·`close-tab`·`rename-tab`도 모두 앞의 탭을 집는다.
/// zellij 0.44.0 이상은 `tab_id`를 주는 [`parse_zellij_tab_list`] 경로를 타므로 이 한계가 없다.
pub fn parse_zellij_tab_names_with_focus(
    query_tab_names_output: &str,
    dump_layout_output: &str,
) -> Vec<TerminalTab> {
    let tab_names: Vec<&str> = query_tab_names_output
        .split('\n')
        .map(|line| js_trim(line.strip_suffix('\r').unwrap_or(line)))
        .filter(|name| !name.is_empty())
        .collect();

    if tab_names.is_empty() {
        return Vec::new();
    }

    let focused_tab_name = parse_zellij_focused_tab_name(dump_layout_output);
    let active_index = focused_tab_name
        .and_then(|focused| tab_names.iter().position(|name| *name == focused))
        .unwrap_or(0);

    tab_names
        .iter()
        .enumerate()
        .map(|(index, name)| TerminalTab {
            id: index.to_string(),
            native_index: index as i64,
            name: name.to_string(),
            is_active: index == active_index,
        })
        .collect()
}

pub fn build_zellij_go_to_tab_by_id_command(session_name: &str, tab_id: &str) -> String {
    build_zellij_action_command(
        session_name,
        &format!("go-to-tab-by-id {}", quote_for_posix_shell(tab_id)),
    )
}

/// 구버전 폴백은 이름으로 이동한다.
/// `go-to-tab`의 인덱스가 1부터인지 0부터인지는 버전에 따라 달라서, 이름이 더 안전하다.
pub fn build_zellij_go_to_tab_by_name_command(session_name: &str, tab_name: &str) -> String {
    build_zellij_action_command(
        session_name,
        &format!("go-to-tab-name {}", quote_for_posix_shell(tab_name)),
    )
}

pub fn build_zellij_new_tab_command(session_name: &str, working_dir: Option<&str>) -> String {
    let new_tab_arguments = match working_dir.filter(|directory| !directory.is_empty()) {
        Some(directory) => format!("new-tab --cwd {}", quote_for_posix_shell(directory)),
        None => "new-tab".to_string(),
    };
    build_zellij_action_command(session_name, &new_tab_arguments)
}

pub fn build_zellij_close_tab_by_id_command(session_name: &str, tab_id: &str) -> String {
    build_zellij_action_command(
        session_name,
        &format!("close-tab-by-id {}", quote_for_posix_shell(tab_id)),
    )
}

/// 구버전 폴백. `close-tab`은 활성 탭만 닫으므로 먼저 대상 탭으로 이동한다
pub fn build_zellij_close_focused_tab_commands(session_name: &str, tab_name: &str) -> Vec<String> {
    vec![
        build_zellij_go_to_tab_by_name_command(session_name, tab_name),
        build_zellij_action_command(session_name, "close-tab"),
    ]
}

pub fn build_zellij_rename_tab_by_id_command(
    session_name: &str,
    tab_id: &str,
    name: &str,
) -> String {
    build_zellij_action_command(
        session_name,
        &format!(
            "rename-tab-by-id {} {}",
            quote_for_posix_shell(tab_id),
            quote_for_posix_shell(name)
        ),
    )
}

/// 구버전 폴백. `rename-tab`도 활성 탭만 바꾸므로 먼저 대상 탭으로 이동한다
pub fn build_zellij_rename_focused_tab_commands(
    session_name: &str,
    tab_name: &str,
    name: &str,
) -> Vec<String> {
    vec![
        build_zellij_go_to_tab_by_name_command(session_name, tab_name),
        build_zellij_action_command(
            session_name,
            &format!("rename-tab {}", quote_for_posix_shell(name)),
        ),
    ]
}

/// 목표 위치까지 몇 칸을 어느 방향으로 옮길지
fn zellij_move_tab_steps(current_index: i64, target_index: i64) -> (usize, &'static str) {
    let step_count = target_index.abs_diff(current_index) as usize;
    let direction = if target_index > current_index {
        "right"
    } else {
        "left"
    };
    (step_count, direction)
}

/// zellij의 `move-tab`은 방향만 받으므로 목표 위치까지 한 칸씩 옮긴다.
/// `-t`로 대상 탭을 직접 겨냥해, 순서만 바꾸려는데 활성 탭까지 바뀌는 일이 없게 한다.
pub fn build_zellij_move_tab_by_id_commands(
    session_name: &str,
    tab_id: &str,
    current_index: i64,
    target_index: i64,
) -> Vec<String> {
    let (step_count, direction) = zellij_move_tab_steps(current_index, target_index);
    let move_command = build_zellij_action_command(
        session_name,
        &format!("move-tab {direction} -t {}", quote_for_posix_shell(tab_id)),
    );

    vec![move_command; step_count]
}

/// 구버전 폴백. `-t`가 없어 대상 탭으로 이동한 뒤 방향 명령을 반복한다
pub fn build_zellij_move_tab_commands(
    session_name: &str,
    tab_name: &str,
    current_index: i64,
    target_index: i64,
) -> Vec<String> {
    let (step_count, direction) = zellij_move_tab_steps(current_index, target_index);
    if step_count == 0 {
        return Vec::new();
    }

    let move_command = build_zellij_action_command(session_name, &format!("move-tab {direction}"));
    let mut commands = vec![build_zellij_go_to_tab_by_name_command(
        session_name,
        tab_name,
    )];
    commands.extend(std::iter::repeat_n(move_command, step_count));
    commands
}

/// 호스트별 zellij id 지정 명령 지원 여부. zellij 버전은 세션이 사는 동안 바뀌지 않으므로 호스트별로 한 번만 확인한다.
///
/// 이름이 축 하나를 고르지 않고 버전 경계를 가리키는 이유는, 부르는 곳이 탭 축과 pane 축 둘 다이기 때문이다.
/// 서비스마다 캐시를 두면 같은 호스트에 `--version`을 두 번 묻고 판정 규칙도 두 곳으로 갈라지므로,
/// 앱이 이 값을 하나만 만들어 두 서비스에 함께 넘긴다.
#[derive(Debug, Default)]
pub struct ZellijIdTargetingSupportCache {
    support_by_host: Mutex<HashMap<String, bool>>,
}

impl ZellijIdTargetingSupportCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 이 호스트의 zellij가 탭·pane을 id로 지정하는 명령을 받는지 판정한다.
    /// 실행 옵션을 인자로 받는 이유는, 호출자마다 타임아웃과 원격 경로가 달라 이 파일이 정할 수 없기 때문이다.
    /// 같은 호스트를 동시에 처음 물으면 TS처럼 둘 다 실행하고 나중 답이 캐시에 남는다.
    pub fn resolve_zellij_id_targeting_support(
        &self,
        ssh_host: Option<&str>,
        runner: &dyn ShellCommandRunner,
        options: &ExecGitOptions,
    ) -> bool {
        let host_key = ssh_host.unwrap_or("local").to_string();
        if let Some(cached_support) = self.lock_support_by_host().get(&host_key) {
            return *cached_support;
        }

        // 버전을 못 읽으면 id 지정 명령이 없는 구버전으로 본다
        let is_supported = runner
            .exec_git(&build_zellij_version_command(), ssh_host, options)
            .is_ok_and(|version_output| supports_zellij_tab_id_commands(&version_output));

        self.lock_support_by_host().insert(host_key, is_supported);
        is_supported
    }

    fn lock_support_by_host(&self) -> std::sync::MutexGuard<'_, HashMap<String, bool>> {
        self.support_by_host
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
