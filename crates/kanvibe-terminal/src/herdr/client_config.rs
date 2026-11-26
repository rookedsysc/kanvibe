//! Herdr client 설정의 탭 바 위치(`[ui] tab_bar_position`). pane 화면을 투영할 때 탭 바 줄을 어느 쪽에서 뺄지 정한다.

use std::sync::LazyLock;

use kanvibe_ai::javascript::js_trim;
use kanvibe_process::exec::ExecGitOptions;
use regex::Regex;

use super::{HERDR_COMMAND_TIMEOUT, HerdrService};
pub use crate::terminal_registry::HerdrTabBarPosition;

/// 설정 경로 계산과 내용 읽기를 한 명령으로 묶는다. 원격에서는 명령 하나가 SSH 왕복 하나라, 나누면 터미널을 열 때마다 왕복이 하나 더 든다.
/// 상대 경로는 어느 디렉터리 기준인지 알 수 없으므로 읽지 않고 실패시킨다.
const HERDR_CLIENT_CONFIG_READ_COMMAND: &str = concat!(
    r#"__kanvibe_herdr_config="${HERDR_CONFIG_PATH:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr/config.toml}""#,
    "; ",
    r#"case "$__kanvibe_herdr_config" in /*) ;; *) exit 64 ;; esac"#,
    "; ",
    r#"if test -e "$__kanvibe_herdr_config"; then cat "$__kanvibe_herdr_config"; fi"#,
);

static TABLE_HEADER_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([^\]]+)\]\s*(?:#.*)?$").expect("유효한 정규식"));
static INLINE_UI_TABLE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(?:ui|"ui"|'ui')\s*="#).expect("유효한 정규식"));
static QUOTED_ESCAPED_KEY_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^["'][^=]*\\"#).expect("유효한 정규식"));
static UI_SECTION_KEY_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:tab_bar_position|"tab_bar_position"|'tab_bar_position')\s*=\s*(.*)$"#)
        .expect("유효한 정규식")
});
static DOTTED_ROOT_KEY_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:ui|"ui"|'ui')\s*\.\s*(?:tab_bar_position|"tab_bar_position"|'tab_bar_position')\s*=\s*(.*)$"#)
        .expect("유효한 정규식")
});
static POSITION_VALUE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:"(top|bottom)"|'(top|bottom)')\s*(?:#.*)?$"#).expect("유효한 정규식")
});

/// JS `replace(/^(["'])(.*)\1$/, "$2")`. 같은 따옴표로 감싼 표 이름의 따옴표를 벗긴다
fn unquote_table_name(name: &str) -> &str {
    let is_quoted = name.len() >= 2
        && ['"', '\'']
            .iter()
            .any(|quote| name.starts_with(*quote) && name.ends_with(*quote));
    match is_quoted {
        true => &name[1..name.len() - 1],
        false => name,
    }
}

/// 한 줄을 읽은 결과
enum ConfigLine {
    Skip,
    Section(String),
    Position(HerdrTabBarPosition),
    /// 해석할 수 없는 TOML에서 위치를 추측하면 pane 밖을 덮을 수 있어 판단을 포기한다
    Unparsable,
}

fn read_config_line(line: &str, section: &str) -> ConfigLine {
    if line.is_empty() || line.starts_with('#') {
        return ConfigLine::Skip;
    }
    if line.contains("\"\"\"") || line.contains("'''") {
        return ConfigLine::Unparsable;
    }
    if line.starts_with('[') {
        let table = TABLE_HEADER_PATTERN.captures(line);
        let name = table
            .as_ref()
            .and_then(|table| table.get(1))
            .map(|name| name.as_str());
        if name.is_some_and(|name| name.contains('\\')) {
            return ConfigLine::Unparsable;
        }
        return ConfigLine::Section(
            name.map_or("", |name| unquote_table_name(js_trim(name)))
                .to_string(),
        );
    }
    if section.is_empty() && INLINE_UI_TABLE_PATTERN.is_match(line) {
        return ConfigLine::Unparsable;
    }
    if section == "ui" && QUOTED_ESCAPED_KEY_PATTERN.is_match(line) {
        return ConfigLine::Unparsable;
    }
    let key = match section {
        "ui" => UI_SECTION_KEY_PATTERN.captures(line),
        "" => DOTTED_ROOT_KEY_PATTERN.captures(line),
        _ => None,
    };
    let Some(value) = key.and_then(|key| key.get(1)) else {
        return ConfigLine::Skip;
    };
    let Some(position) = POSITION_VALUE_PATTERN.captures(value.as_str()) else {
        return ConfigLine::Unparsable;
    };
    match position
        .get(1)
        .or_else(|| position.get(2))
        .map(|value| value.as_str())
    {
        Some("bottom") => ConfigLine::Position(HerdrTabBarPosition::Bottom),
        _ => ConfigLine::Position(HerdrTabBarPosition::Top),
    }
}

/// 설정 내용에서 탭 바 위치를 읽는다. 값이 없으면 Herdr 기본값 top, 해석할 수 없거나 두 번 정했으면 `None`이다
pub(super) fn parse_tab_bar_position(content: &str) -> Option<HerdrTabBarPosition> {
    let mut section = String::new();
    let mut position = HerdrTabBarPosition::Top;
    let mut has_found_position = false;
    for original in content.split('\n') {
        let line = js_trim(original.strip_suffix('\r').unwrap_or(original));
        match read_config_line(line, &section) {
            ConfigLine::Skip => {}
            ConfigLine::Section(name) => section = name,
            ConfigLine::Unparsable => return None,
            ConfigLine::Position(_) if has_found_position => return None,
            ConfigLine::Position(found) => {
                position = found;
                has_found_position = true;
            }
        }
    }
    Some(position)
}

impl HerdrService {
    /// TS `readHerdrClientTabBarPosition`. 설정을 읽지 못하면 `None`이다
    pub fn read_herdr_client_tab_bar_position(
        &self,
        ssh_host: Option<&str>,
    ) -> Option<HerdrTabBarPosition> {
        let options = ExecGitOptions {
            timeout: Some(HERDR_COMMAND_TIMEOUT),
            ..ExecGitOptions::default()
        };
        let content = self
            .runner
            .exec_git(HERDR_CLIENT_CONFIG_READ_COMMAND, ssh_host, &options)
            .ok()?;
        parse_tab_bar_position(&content)
    }
}
