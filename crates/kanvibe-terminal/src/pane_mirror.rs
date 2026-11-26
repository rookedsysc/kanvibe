//! 모바일 클라이언트에 pane 하나를 그대로 비추기 위한 명령 빌더와 출력 파서(TS `src/lib/paneMirror.ts`).
//!
//! `terminal_tabs`가 다루는 탭 축은 tmux window와 zellij tab이고, 이 모듈은 그 아래 pane 축만 다룬다.
//! 두 축을 갈라 둔 이유는 조작 대상이 다르기 때문이다. 탭 축은 사용자가 만들고 지우고 이름을 바꾸는 대상이지만,
//! pane 축은 읽어서 비추기만 하는 대상이다.
//!
//! 읽기 명령은 포커스를 바꾸지 않는다. 모바일 viewport 명령만 연결 수명 동안 크기를 바꾸고 원래 배치를 복원한다.
//! tmux grouped session은 window 객체를 공유해서 `select-pane`이나 `resize-pane -Z`가 데스크탑 화면까지 움직이고,
//! zellij도 pane id를 주지 않으면 포커스된 pane을 대상으로 삼는다. 그래서 모든 명령이 대상을 명시적으로 받는다.
//!
//! 모든 함수는 부수효과가 없다. 실제 실행은 모바일 브리지 서비스가 맡는다.

use std::sync::LazyLock;

use kanvibe_ai::javascript::{
    is_js_whitespace, js_string, js_trim, js_trim_end, js_trim_start, number_to_string,
};
use kanvibe_ai::usage::shared::js_number;
use kanvibe_process::worktree::quote_for_posix_shell;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::herdr::build_herdr_command;
pub use crate::remote_terminal_protocol::{
    HerdrObserveRecord, TerminalCursorPosition, build_screen_redraw, parse_herdr_observe_record,
};
use crate::remote_terminal_protocol::{
    MAX_MIRROR_LINES, MouseReportEncoding, ScrollDirection, js_integer,
};

#[cfg(test)]
mod tests;

/// 비출 pane 하나. tmux·zellij·Herdr pane을 같은 모양으로 맞춘다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorPane {
    /// 멀티플렉서가 주는 안정적인 pane 식별자. tmux는 `%17`, zellij는 `terminal_1`, Herdr는 `w1:p1`
    pub id: String,
    /// 이 pane이 속한 탭(tmux window / zellij tab / Herdr tab)의 식별자
    pub tab_id: String,
    pub tab_name: String,
    /// pane에서 돌고 있는 명령. 알 수 없으면 빈 문자열
    pub command: String,
    /// 탭 안에서의 좌표와 크기. 태블릿이 window 하나를 통째로 조립할 때 쓴다
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
}

/// 이름을 마지막 필드에 두고 탭 문자로 구분한다.
/// tmux는 format 출력에서 이름의 제어문자를 이스케이프하지만, 파서는 남는 조각을 되붙여 그 가정이 깨져도 이름이 잘리지 않게 한다.
const TMUX_MIRROR_PANE_FORMAT: &str = "#{pane_id}\t#{window_id}\t#{pane_current_command}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}\t#{window_name}";

const TMUX_MIRROR_PANE_FIELD_COUNT: usize = 8;

/// 세션 하나의 모든 pane을 window 순서대로 조회하는 명령.
/// 에이전트 탐지용 pane 조회는 서버 전체를 훑고 좌표를 담지 않으므로 여기서는 쓰지 않는다.
pub fn build_tmux_list_mirror_panes_command(session_name: &str) -> String {
    let target = quote_for_posix_shell(session_name);
    format!(
        "tmux list-panes -s -t {target} -F {}",
        quote_for_posix_shell(TMUX_MIRROR_PANE_FORMAT)
    )
}

/// 모바일 viewport로 바꾸기 전 tmux window의 크기·배치·크기 정책·확대 여부
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxViewportState {
    pub width: i64,
    pub height: i64,
    pub layout: String,
    pub window_size: Option<String>,
    pub is_zoomed: bool,
}

pub fn build_tmux_read_viewport_command(window_id: &str) -> String {
    let target = quote_for_posix_shell(window_id);
    format!(
        "tmux display-message -p -t {target} '#{{window_width}}\t#{{window_height}}\t#{{window_layout}}\t#{{window_zoomed_flag}}' && tmux show-options -wqv -t {target} window-size"
    )
}

pub fn parse_tmux_viewport_state(output: &str) -> Result<TmuxViewportState, String> {
    let mut output_lines = js_trim_end(output).split('\n');
    let geometry = output_lines.next().unwrap_or_default();
    let window_size = output_lines.next().filter(|size| !size.is_empty());
    let mut fields = geometry.split('\t');
    let width = parse_positive_integer(fields.next());
    let height = parse_positive_integer(fields.next());
    let layout = fields.next().filter(|layout| !layout.is_empty());
    let is_zoomed = fields.next() == Some("1");
    let (Some(width), Some(height), Some(layout)) = (width, height, layout) else {
        return Err("tmux 창 크기를 읽지 못했습니다.".to_string());
    };
    Ok(TmuxViewportState {
        width,
        height,
        layout: layout.to_string(),
        window_size: window_size.map(str::to_string),
        is_zoomed,
    })
}

/// JS `Number(text)`가 1 이상의 정수일 때 그 값. 필드가 없으면(`undefined`) 읽지 못한 것이다
fn parse_positive_integer(text: Option<&str>) -> Option<i64> {
    let number = js_number(text?);
    let is_positive_integer = number.is_finite() && number.fract() == 0.0 && number >= 1.0;
    is_positive_integer.then_some(number as i64)
}

pub fn build_tmux_read_pane_geometry_command(window_id: &str, pane_id: &str) -> String {
    let window = quote_for_posix_shell(window_id);
    let unzoom = quote_for_posix_shell(&format!("resize-pane -Z -t {window}"));
    format!(
        "tmux if-shell -F -t {window} '#{{window_zoomed_flag}}' {unzoom} && tmux display-message -p -t {} '#{{window_width}} #{{window_height}} #{{pane_width}} #{{pane_height}}'",
        quote_for_posix_shell(pane_id)
    )
}

/// 읽어 둔 window·pane 크기에서 pane만 `cols`×`rows`가 되도록 window 크기를 정해 함께 바꾼다
pub fn build_tmux_resize_viewport_command(
    pane_id: &str,
    geometry: &str,
    cols: i64,
    rows: i64,
) -> Result<String, String> {
    let sizes: Vec<Option<i64>> = js_trim(geometry)
        .split(is_js_whitespace)
        .filter(|field| !field.is_empty())
        .take(4)
        .map(|field| parse_positive_integer(Some(field)))
        .collect();
    let [
        Some(window_width),
        Some(window_height),
        Some(pane_width),
        Some(pane_height),
    ] = sizes[..]
    else {
        return Err("tmux pane 크기를 읽지 못했습니다.".to_string());
    };
    let target = quote_for_posix_shell(pane_id);
    let width = window_width - pane_width + cols;
    let height = window_height - pane_height + rows;
    Ok(format!(
        "tmux resize-window -t {target} -x {width} -y {height} && tmux resize-pane -t {target} -x {cols} -y {rows}"
    ))
}

pub fn build_tmux_restore_viewport_command(window_id: &str, state: &TmuxViewportState) -> String {
    let target = quote_for_posix_shell(window_id);
    let policy = match &state.window_size {
        None => format!("tmux set-option -w -u -t {target} window-size"),
        Some(window_size) => format!(
            "tmux set-option -w -t {target} window-size {}",
            quote_for_posix_shell(window_size)
        ),
    };
    let zoom = if state.is_zoomed {
        format!(
            "tmux if-shell -F -t {target} '#{{!:#{{window_zoomed_flag}}}}' {} || exit $?; ",
            quote_for_posix_shell(&format!("resize-pane -Z -t {target}"))
        )
    } else {
        String::new()
    };
    // PC에서 pane을 추가하면 저장한 layout을 적용할 수 없어도 임시 manual 정책은 반드시 풀어야 한다.
    format!(
        "kanvibe_restore_status=0; tmux resize-window -t {target} -x {} -y {} && tmux select-layout -t {target} {} || kanvibe_restore_status=$?; {policy} || exit $?; {zoom}exit \"$kanvibe_restore_status\"",
        state.width,
        state.height,
        quote_for_posix_shell(&state.layout)
    )
}

/// `build_tmux_list_mirror_panes_command` 출력을 pane 목록으로 바꾼다
pub fn parse_tmux_mirror_pane_list(list_panes_output: &str) -> Vec<MirrorPane> {
    list_panes_output
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty())
        .filter_map(parse_tmux_mirror_pane_line)
        .collect()
}

fn parse_tmux_mirror_pane_line(line: &str) -> Option<MirrorPane> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() < TMUX_MIRROR_PANE_FIELD_COUNT {
        return None;
    }
    let [
        pane_id,
        window_id,
        command,
        left,
        top,
        width,
        height,
        name_fields @ ..,
    ] = fields.as_slice()
    else {
        return None;
    };
    let geometry = parse_geometry([*left, *top, *width, *height])?;
    if pane_id.is_empty() || window_id.is_empty() {
        return None;
    }
    Some(geometry.into_mirror_pane(
        pane_id.to_string(),
        window_id.to_string(),
        name_fields.join("\t"),
        command.to_string(),
    ))
}

/// 탭 안 pane 좌표와 크기
struct PaneGeometry {
    left: i64,
    top: i64,
    width: i64,
    height: i64,
}

impl PaneGeometry {
    fn into_mirror_pane(
        self,
        id: String,
        tab_id: String,
        tab_name: String,
        command: String,
    ) -> MirrorPane {
        MirrorPane {
            id,
            tab_id,
            tab_name,
            command,
            left: self.left,
            top: self.top,
            width: self.width,
            height: self.height,
        }
    }
}

/// `[left, top, width, height]` 글자를 JS `Number.parseInt(_, 10)`처럼 읽는다. 크기가 0 이하면 비출 수 없다
fn parse_geometry(raw: [&str; 4]) -> Option<PaneGeometry> {
    let [left, top, width, height] = raw.map(parse_decimal_int_prefix);
    let geometry = PaneGeometry {
        left: left?,
        top: top?,
        width: width?,
        height: height?,
    };
    (geometry.width > 0 && geometry.height > 0).then_some(geometry)
}

/// JS `Number.parseInt(text, 10)`. 앞 공백과 부호 뒤의 숫자만 읽고, 숫자가 없으면 `None`(NaN)이다
fn parse_decimal_int_prefix(text: &str) -> Option<i64> {
    let trimmed = js_trim_start(text);
    let (sign, unsigned) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let digit_count = unsigned.bytes().take_while(u8::is_ascii_digit).count();
    let magnitude: i64 = unsigned[..digit_count].parse().ok()?;
    Some(sign * magnitude)
}

/// 커서와 접속 전에 활성화된 화면·마우스 모드를 함께 읽는다.
pub fn build_tmux_pane_state_command(pane_id: &str) -> String {
    format!(
        "tmux display-message -p -t {} '#{{cursor_x}} #{{cursor_y}} #{{pane_height}} #{{alternate_on}} #{{mouse_standard_flag}} #{{mouse_button_flag}} #{{mouse_all_flag}} #{{mouse_sgr_flag}} #{{mouse_utf8_flag}}'",
        quote_for_posix_shell(pane_id)
    )
}

/// tmux pane의 커서 위치와, 모바일 터미널에 접속 전 화면·마우스 모드를 되살리는 제어 문자열
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPaneState {
    pub cursor: TerminalCursorPosition,
    pub restore_sequences: String,
}

static TMUX_PANE_STATE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([0-9]+) ([0-9]+) ([0-9]+) ([01]) ([01]) ([01]) ([01]) ([01]) ([01])$")
        .expect("tmux pane 상태 패턴")
});

pub fn parse_tmux_pane_state(output: &str) -> Option<TmuxPaneState> {
    let captures = TMUX_PANE_STATE_PATTERN.captures(js_trim(output))?;
    let flag = |index: usize| &captures[index] == "1";
    let mouse_mode = if flag(7) {
        Some(1003)
    } else if flag(6) {
        Some(1002)
    } else if flag(5) {
        Some(1000)
    } else {
        None
    };
    let mouse_encoding = if flag(8) {
        Some(1006)
    } else if flag(9) {
        Some(1005)
    } else {
        None
    };
    let screen_mode = if flag(4) { "h" } else { "l" };
    let mut restore_sequences =
        format!("\x1b[?1049{screen_mode}\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l");
    for mode in [mouse_mode, mouse_encoding].into_iter().flatten() {
        restore_sequences.push_str(&format!("\x1b[?{mode}h"));
    }
    let cursor = TerminalCursorPosition {
        x: captures[1].parse().ok()?,
        y: captures[2].parse().ok()?,
        pane_height: captures[3].parse().ok()?,
    };
    Some(TmuxPaneState {
        cursor,
        restore_sequences,
    })
}

/// tmux 기본 휠 바인딩이 갈림길로 쓰는 pane 상태와, 휠을 넘길 때 맞춰야 할 마우스 보고 인코딩을 읽는다
pub fn build_tmux_wheel_state_command(pane_id: &str) -> String {
    format!(
        "tmux display-message -p -t {} '#{{pane_in_mode}} #{{alternate_on}} #{{mouse_any_flag}} #{{mouse_sgr_flag}} #{{mouse_utf8_flag}}'",
        quote_for_posix_shell(pane_id)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TmuxWheelState {
    pub is_in_copy_mode: bool,
    pub is_alternate_screen: bool,
    pub is_mouse_reporting: bool,
    pub encoding: MouseReportEncoding,
}

static TMUX_WHEEL_STATE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([01]) ([01]) ([01]) ([01]) ([01])$").expect("tmux 휠 상태 패턴")
});

pub fn parse_tmux_wheel_state(output: &str) -> Option<TmuxWheelState> {
    let captures = TMUX_WHEEL_STATE_PATTERN.captures(js_trim(output))?;
    let flag = |index: usize| &captures[index] == "1";
    let encoding = if flag(4) {
        MouseReportEncoding::Sgr
    } else if flag(5) {
        MouseReportEncoding::Utf8
    } else {
        MouseReportEncoding::Normal
    };
    Some(TmuxWheelState {
        is_in_copy_mode: flag(1),
        is_alternate_screen: flag(2),
        is_mouse_reporting: flag(3),
        encoding,
    })
}

/// tmux 기본 copy-mode 표의 휠 바인딩(`send-keys -X -N 5 scroll-up`)과 같은 폭으로 움직인다
const TMUX_COPY_MODE_WHEEL_LINES: u32 = 5;

pub fn build_tmux_copy_mode_wheel_command(pane_id: &str, direction: ScrollDirection) -> String {
    format!(
        "tmux send-keys -X -N {TMUX_COPY_MODE_WHEEL_LINES} -t {} scroll-{}",
        quote_for_posix_shell(pane_id),
        direction.as_str()
    )
}

/// 휠로 들어간 copy-mode는 `-e` 덕분에 맨 아래로 내려오면 스스로 끝난다
pub fn build_tmux_enter_wheel_copy_mode_command(pane_id: &str) -> String {
    format!("tmux copy-mode -e -t {}", quote_for_posix_shell(pane_id))
}

/// copy-mode가 얼마나 올라가 있는지 읽는다. copy-mode가 아니면 첫 값이 0이다
pub fn build_tmux_copy_mode_view_command(pane_id: &str) -> String {
    format!(
        "tmux display-message -p -t {} '#{{pane_in_mode}} #{{scroll_position}} #{{pane_height}}'",
        quote_for_posix_shell(pane_id)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TmuxCopyModeView {
    pub scroll_position: i64,
    pub pane_height: i64,
}

static TMUX_COPY_MODE_VIEW_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^1 ([0-9]+) ([0-9]+)$").expect("tmux copy-mode 화면 패턴"));

pub fn parse_tmux_copy_mode_view(output: &str) -> Option<TmuxCopyModeView> {
    let captures = TMUX_COPY_MODE_VIEW_PATTERN.captures(js_trim(output))?;
    Some(TmuxCopyModeView {
        scroll_position: captures[1].parse().ok()?,
        pane_height: captures[2].parse().ok()?,
    })
}

/// copy-mode가 보여 주는 줄 범위를 뜬다. `pipe-pane`은 프로그램 출력만 전하므로 이 화면은 따로 떠서 모바일에 보내야 한다.
/// `capture-pane -M`은 tmux 3.8에서 copy-mode의 스크롤 위치를 반영하지 않아, 올라간 줄 수만큼 범위를 옮겨 뜬다.
pub fn build_tmux_capture_copy_mode_view_command(pane_id: &str, view: &TmuxCopyModeView) -> String {
    let last_line = view.pane_height - 1 - view.scroll_position;
    format!(
        "tmux capture-pane -pe -S -{} -E {last_line} -t {}",
        view.scroll_position,
        quote_for_posix_shell(pane_id)
    )
}

/// pane의 현재 화면을 ANSI를 살린 채로 한 번 떠 온다.
/// 스트림은 구독 시점 이후만 흘러오므로 첫 화면은 이 스냅샷으로 채워야 빈 터미널이 보이지 않는다.
pub fn build_tmux_capture_pane_command(pane_id: &str) -> String {
    format!(
        "tmux capture-pane -pe -S -{MAX_MIRROR_LINES} -t {}",
        quote_for_posix_shell(pane_id)
    )
}

/// 흘려보내기를 멈춘다. 명령 인자를 주지 않으면 tmux가 해당 pane의 파이프를 끊는다
pub fn build_tmux_cancel_pipe_pane_command(pane_id: &str) -> String {
    format!("tmux pipe-pane -t {}", quote_for_posix_shell(pane_id))
}

/// pane의 출력을 계속 흘려보내는 명령. 표준출력으로 원문 바이트가 그대로 나온다.
///
/// tmux의 `pipe-pane`은 자기 자식으로 명령을 띄우고 우리 프로세스의 fd를 물려주지 않으므로,
/// 중간에 임시 파일을 두고 그 파일을 `tail -f`로 따라가는 방식 말고는 출력을 받아올 방법이 없다.
/// 파일을 거치는 대신 얻는 것은, 이 한 줄이 로컬에서도 `ssh` 너머에서도 똑같이 동작한다는 점이다.
///
/// `-o`는 같은 pane에 파이프가 이미 걸려 있으면 끄는 토글이라, 호출자는 pane마다 파이프를 하나만 유지해야 한다.
/// 구독자가 여럿이면 파이프도 하나만 걸고 받은 쪽에서 나눠 보낸다.
///
/// `trap`이 있어야 이 프로세스가 죽을 때 pane에 걸어 둔 파이프와 임시 파일이 남지 않는다.
/// 그런데 `tail`을 마지막 명령으로 두면 셸이 그 자리에서 자신을 `exec`로 치환해 트랩째 사라진다.
/// 그래서 `tail`을 배경으로 돌리고 `wait`로 붙잡는다. 이 형태여야 종료 신호에 트랩이 실제로 실행된다.
pub fn build_tmux_pane_stream_command(pane_id: &str) -> String {
    let target = quote_for_posix_shell(pane_id);
    [
        "sink=$(mktemp -t kanvibe-pane.XXXXXX)".to_string(),
        format!("trap 'tmux pipe-pane -t {target}; rm -f \"$sink\"; kill $tailpid 2>/dev/null' EXIT INT TERM"),
        format!("tmux pipe-pane -o -t {target} \"cat >> '$sink'\""),
        "tail -n +1 -f \"$sink\" & tailpid=$!".to_string(),
        "wait $tailpid".to_string(),
    ]
    .join("; ")
}

/// 모바일에서 누른 키를 pane에 그대로 넣는다.
/// `-l`은 문자열을 키 이름으로 해석하지 않고 글자 그대로 보내라는 뜻이고, `--`가 있어야 대시로 시작하는 입력이 옵션으로 먹히지 않는다.
pub fn build_tmux_send_keys_command(pane_id: &str, input: &str) -> String {
    let target = quote_for_posix_shell(pane_id);
    if !input.contains('\0') {
        return format!(
            "tmux send-keys -t {target} -l -- {}",
            quote_for_posix_shell(input)
        );
    }
    // 프로세스 인자는 NUL을 담을 수 없으므로 그 바이트만 tmux의 이름 있는 키로 전한다.
    let mut commands: Vec<String> = Vec::new();
    for (index, text) in input.split('\0').enumerate() {
        if index > 0 {
            commands.push(format!("tmux send-keys -t {target} -- C-Space"));
        }
        if !text.is_empty() {
            commands.push(format!(
                "tmux send-keys -t {target} -l -- {}",
                quote_for_posix_shell(text)
            ));
        }
    }
    commands.join(" && ")
}

/// zellij pane id는 JSON의 숫자 id에 종류 접두사를 붙인 형태다.
/// plugin pane은 zellij 자신의 UI라 비추지 않으므로 terminal만 다룬다.
fn format_zellij_pane_id(numeric_id: &str) -> String {
    format!("terminal_{numeric_id}")
}

/// JS 템플릿 문자열 `${value}`. 없는 필드는 `undefined`로 찍힌다
fn template_string(value: Option<&Value>) -> String {
    value.map_or_else(|| "undefined".to_string(), js_string)
}

/// zellij 세션의 pane을 JSON으로 조회하는 명령.
/// 표 형식은 필드 구분이 공백 두 칸인데 탭 이름과 명령에 공백이 들어갈 수 있어 일반적으로 파싱할 수 없다. JSON이 아니면 안 된다.
pub fn build_zellij_list_mirror_panes_command(session_name: &str) -> String {
    format!(
        "zellij --session {} action list-panes --json -a",
        quote_for_posix_shell(session_name)
    )
}

/// 클라이언트 화면 안의 칸 좌표(0부터)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPosition {
    pub left: i64,
    pub top: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZellijViewportState {
    pub tab_id: String,
    pub cols: i64,
    pub rows: i64,
    pub pane_cols: i64,
    pub pane_rows: i64,
    pub viewport_cols: i64,
    pub viewport_rows: i64,
    pub display_cols: i64,
    pub display_rows: i64,
    /// 클라이언트 화면에서 pane 글자 영역이 시작하는 칸(0부터). 휠 보고의 좌표를 이 기준으로 옮긴다. 목록이 알려 주지 않으면 비어 있다
    pub content_origin: Option<CellPosition>,
    pub floating: Option<CellPosition>,
}

pub fn build_zellij_read_viewport_command(session_name: &str) -> String {
    format!(
        "printf '[' && {} && printf ',' && zellij --session {} action list-tabs --json -a && printf ']'",
        build_zellij_list_mirror_panes_command(session_name),
        quote_for_posix_shell(session_name)
    )
}

pub fn build_zellij_select_tab_command(session_name: &str, tab_id: &str) -> String {
    format!(
        "zellij --session {} action go-to-tab-by-id {}",
        quote_for_posix_shell(session_name),
        quote_for_posix_shell(tab_id)
    )
}

/// floating pane을 옮길 좌표와 테두리 포함 크기
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingPaneGeometry {
    pub left: i64,
    pub top: i64,
    pub cols: i64,
    pub rows: i64,
}

pub fn build_zellij_resize_floating_pane_command(
    session_name: &str,
    pane_id: &str,
    geometry: &FloatingPaneGeometry,
) -> String {
    format!(
        "zellij --session {} action change-floating-pane-coordinates --pane-id {} --x {} --y {} --width {} --height {}",
        quote_for_posix_shell(session_name),
        quote_for_posix_shell(pane_id),
        geometry.left,
        geometry.top,
        geometry.cols,
        geometry.rows
    )
}

/// `build_zellij_read_viewport_command` 출력에서 `pane_id` pane과 그 탭의 격자 크기를 읽는다. 하나라도 빠지면 `None`이다
pub fn parse_zellij_viewport_state(output: &str, pane_id: &str) -> Option<ZellijViewportState> {
    let parsed: Value = serde_json::from_str(output).ok()?;
    let panes = parsed.get(0)?.as_array()?;
    let tabs = parsed.get(1)?.as_array()?;
    let pane = panes.iter().find(|entry| {
        entry.get("is_plugin") == Some(&Value::Bool(false))
            && format_zellij_pane_id(&template_string(entry.get("id"))) == pane_id
    })?;
    // JS `===`로 찾은 탭이 정수 tab_id를 가져야 하므로, 숫자가 아닌 pane tab_id는 어떤 탭과 맞아도 결과가 없다
    let pane_tab_id = pane.get("tab_id")?.as_f64()?;
    let tab = tabs
        .iter()
        .find(|entry| entry.get("tab_id").and_then(Value::as_f64) == Some(pane_tab_id))?;
    let tab_id = js_integer(tab.get("tab_id")).filter(|tab_id| *tab_id >= 0.0)?;
    let sizes = read_zellij_viewport_sizes(pane, tab)?;
    let content_origin = read_cell_position(pane, "pane_content_x", "pane_content_y");
    let floating = if pane.get("is_floating") == Some(&Value::Bool(true)) {
        Some(read_cell_position(pane, "pane_x", "pane_y")?)
    } else {
        None
    };
    Some(ZellijViewportState {
        tab_id: number_to_string(tab_id),
        cols: sizes[0],
        rows: sizes[1],
        pane_cols: sizes[2],
        pane_rows: sizes[3],
        viewport_cols: sizes[4],
        viewport_rows: sizes[5],
        display_cols: sizes[6],
        display_rows: sizes[7],
        content_origin,
        floating,
    })
}

/// pane 글자 영역, pane 전체, 탭 viewport, 표시 영역 크기를 순서대로 읽는다. 모두 양의 정수여야 한다
fn read_zellij_viewport_sizes(pane: &Value, tab: &Value) -> Option<[i64; 8]> {
    let fields = [
        (pane, "pane_content_columns"),
        (pane, "pane_content_rows"),
        (pane, "pane_columns"),
        (pane, "pane_rows"),
        (tab, "viewport_columns"),
        (tab, "viewport_rows"),
        (tab, "display_area_columns"),
        (tab, "display_area_rows"),
    ];
    let mut sizes = [0_i64; 8];
    for (size, (entry, key)) in sizes.iter_mut().zip(fields) {
        let value = js_integer(entry.get(key)).filter(|value| *value > 0.0)?;
        *size = value as i64;
    }
    Some(sizes)
}

fn read_cell_position(entry: &Value, left_key: &str, top_key: &str) -> Option<CellPosition> {
    let left = js_integer(entry.get(left_key)).filter(|left| *left >= 0.0)?;
    let top = js_integer(entry.get(top_key)).filter(|top| *top >= 0.0)?;
    Some(CellPosition {
        left: left as i64,
        top: top as i64,
    })
}

/// `build_zellij_list_mirror_panes_command` 출력을 pane 목록으로 바꾼다
pub fn parse_zellij_mirror_pane_list(list_panes_json_output: &str) -> Vec<MirrorPane> {
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(list_panes_json_output) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(parse_zellij_mirror_pane_entry)
        .collect()
}

fn parse_zellij_mirror_pane_entry(entry: &Value) -> Option<MirrorPane> {
    if entry.get("is_plugin") != Some(&Value::Bool(false)) {
        return None;
    }
    js_integer(entry.get("id"))?;
    // JS는 `String(entry.pane_x)`를 `parseInt`로 읽으므로 숫자도 글자로 바꾼 뒤 같은 규칙으로 읽는다
    let raw_geometry = ["pane_x", "pane_y", "pane_columns", "pane_rows"]
        .map(|key| template_string(entry.get(key)));
    let geometry = parse_geometry(raw_geometry.each_ref().map(String::as_str))?;
    let tab_id = match entry.get("tab_id") {
        None | Some(Value::Null) => String::new(),
        Some(tab_id) => js_string(tab_id),
    };
    let tab_name = entry
        .get("tab_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(geometry.into_mirror_pane(
        format_zellij_pane_id(&template_string(entry.get("id"))),
        tab_id,
        tab_name.to_string(),
        zellij_pane_command(entry).to_string(),
    ))
}

/// 명령이 비어 있는 pane은 기본 셸이라 zellij가 붙인 제목을 대신 보여 준다
fn zellij_pane_command(entry: &Value) -> &str {
    let terminal_command = entry.get("terminal_command").and_then(Value::as_str);
    match terminal_command.filter(|command| !command.is_empty()) {
        Some(command) => command,
        None => entry
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    }
}

/// zellij pane 하나의 현재 화면을 ANSI를 살린 채로 떠 온다.
/// zellij에는 tmux `pipe-pane`에 해당하는 출력 스트림이 없어서 이 명령을 주기적으로 다시 부르는 방식으로 따라간다.
pub fn build_zellij_dump_pane_command(session_name: &str, pane_id: &str) -> String {
    let session = quote_for_posix_shell(session_name);
    // 원격 전송량과 exec 버퍼를 제한하되 dump 실패를 tail의 성공으로 덮지 않는다.
    format!(
        "kanvibe_mobile_dump=$(zellij --session {session} action dump-screen -p {} -a --full) && printf '%s\\n' \"$kanvibe_mobile_dump\" | tail -n {MAX_MIRROR_LINES}",
        quote_for_posix_shell(pane_id)
    )
}

/// pane에 지금 보이는 화면만 떠 온다. 스크롤백 없이 떠야 Zellij가 휠로 올려 둔 위치가 모바일에 그대로 보인다.
pub fn build_zellij_dump_viewport_command(session_name: &str, pane_id: &str) -> String {
    format!(
        "zellij --session {} action dump-screen -p {} -a",
        quote_for_posix_shell(session_name),
        quote_for_posix_shell(pane_id)
    )
}

/// 모바일에서 누른 키를 zellij pane에 넣는다.
/// `action write`는 바이트를 십진수로 하나씩 받으므로 문자열을 UTF-8 바이트로 펼쳐 넘긴다.
/// `send-keys`가 아니라 `write`를 쓰는 이유는 키 이름 해석 없이 원문 그대로를 전달해야 하기 때문이다.
pub fn build_zellij_write_pane_command(session_name: &str, pane_id: &str, input: &str) -> String {
    let session = quote_for_posix_shell(session_name);
    let bytes: Vec<String> = input.bytes().map(|byte| byte.to_string()).collect();
    format!(
        "zellij --session {session} action write -p {} {}",
        quote_for_posix_shell(pane_id),
        bytes.join(" ")
    )
}

pub fn build_tmux_pane_shortcut_command(pane_id: &str, action: &str) -> Option<String> {
    let target = quote_for_posix_shell(pane_id);
    let command = match action {
        "split-right" => format!("tmux split-window -h -d -t {target} -c '#{{pane_current_path}}'"),
        "split-down" => format!("tmux split-window -v -d -t {target} -c '#{{pane_current_path}}'"),
        "toggle-fullscreen" => format!("tmux resize-pane -Z -t {target}"),
        "copy-mode" => format!("tmux copy-mode -t {target}"),
        "scroll-up" => {
            format!("tmux copy-mode -t {target} && tmux send-keys -X -t {target} page-up")
        }
        "scroll-down" => {
            format!("tmux copy-mode -t {target} && tmux send-keys -X -t {target} page-down")
        }
        "scroll-bottom" => {
            format!("tmux copy-mode -t {target} && tmux send-keys -X -t {target} cancel")
        }
        _ => return None,
    };
    Some(command)
}

/// pane 하나를 종료한다. 단축 동작 빌더에 넣지 않은 것은 그 경로가 마지막 pane 보호 없이 HTTP로도 열려 있기 때문이다.
/// 이 빌더들은 마지막 pane을 막는 pane 종료 경로만 부른다.
pub fn build_tmux_close_pane_command(pane_id: &str) -> String {
    format!("tmux kill-pane -t {}", quote_for_posix_shell(pane_id))
}

pub fn build_zellij_pane_shortcut_command(
    session_name: &str,
    pane_id: &str,
    tab_id: &str,
    action: &str,
) -> Option<String> {
    let prefix = format!(
        "zellij --session {} action",
        quote_for_posix_shell(session_name)
    );
    let command = match action {
        // --pane-id는 in-place 교체 전용이므로 분할은 검증한 탭에 만들고 다른 클라이언트의 포커스를 유지한다.
        "split-right" | "split-down" => {
            let direction = if action == "split-right" {
                "right"
            } else {
                "down"
            };
            return Some(format!(
                "{prefix} new-pane --direction {direction} --tab-id {} --no-focus",
                quote_for_posix_shell(tab_id)
            ));
        }
        "toggle-fullscreen" => "toggle-fullscreen",
        "toggle-floating" => "toggle-pane-embed-or-floating",
        "scroll-up" => "page-scroll-up",
        "scroll-down" => "page-scroll-down",
        "scroll-bottom" => "scroll-to-bottom",
        _ => return None,
    };
    Some(format!(
        "{prefix} {command} --pane-id {}",
        quote_for_posix_shell(pane_id)
    ))
}

pub fn build_zellij_close_pane_command(session_name: &str, pane_id: &str) -> String {
    format!(
        "zellij --session {} action close-pane --pane-id {}",
        quote_for_posix_shell(session_name),
        quote_for_posix_shell(pane_id)
    )
}

/// 지나간 출력까지 담은 Herdr pane 내용을 ANSI를 살린 채로 읽는다.
/// 커서 위치는 주지 않으므로 화면과 커서는 뒤따르는 observe의 전체 프레임이 맞춘다.
pub fn build_herdr_recent_screen_command(
    session_name: &str,
    pane_id: &str,
) -> Result<String, String> {
    let line_count = MAX_MIRROR_LINES.to_string();
    build_herdr_command(
        session_name,
        &[
            "pane",
            "read",
            pane_id,
            "--source",
            "recent",
            "--lines",
            &line_count,
            "--format",
            "ansi",
        ],
    )
}

/// observe는 크기를 주지 않으면 pane과 무관하게 120x40으로 그린다.
/// 이 크기는 관찰자 전용 격자라 pane에서 도는 프로그램의 크기를 바꾸지 않는다.
pub fn build_herdr_observe_pane_command(
    session_name: &str,
    pane_id: &str,
    cols: i64,
    rows: i64,
) -> Result<String, String> {
    build_herdr_terminal_session_command(session_name, "observe", pane_id, cols, rows)
}

/// SSH로 띄우는 observe. observe는 stdin을 읽지 않아 SSH 채널이 닫혀도 pane이 다음에 출력할 때까지 원격에 남는다.
/// 데스크탑은 stdin을 열어 둔 채로 두므로, stdin이 닫혔다는 것은 SSH 채널이 닫혔다는 뜻이다. 그때 observe를 끝낸다
pub fn build_herdr_remote_observe_pane_command(
    session_name: &str,
    pane_id: &str,
    cols: i64,
    rows: i64,
) -> Result<String, String> {
    let observe = build_herdr_observe_pane_command(session_name, pane_id, cols, rows)?;
    Ok(end_when_stdin_closes(&observe))
}

/// `command`를 stdin과 떼어 띄우고, stdin이 닫히면 끝낸다. 출력은 건드리지 않고 종료 코드는 `command`의 것이다.
/// 비대화형 sh는 백그라운드 명령의 stdin을 /dev/null로 바꾸므로, stdin을 fd 3으로 먼저 복제해 지켜본다.
/// `command`가 먼저 끝나면 지켜보던 셸도 끝내 데스크탑이 출력 끝과 종료를 바로 본다.
/// `$!`가 곧 `command`의 pid여야 끝낼 수 있으므로 `command`는 단순 명령 하나여야 한다
fn end_when_stdin_closes(command: &str) -> String {
    format!(
        "exec 3<&0; {command} </dev/null & observer=$!; \
         {{ cat <&3 >/dev/null 2>&1; kill \"$observer\" 2>/dev/null; }} & stdin_watcher=$!; \
         exec 3<&-; wait \"$observer\"; observer_status=$?; kill \"$stdin_watcher\" 2>/dev/null; \
         exit \"$observer_status\""
    )
}

pub fn build_herdr_control_pane_command(
    session_name: &str,
    pane_id: &str,
    cols: i64,
    rows: i64,
) -> Result<String, String> {
    build_herdr_terminal_session_command(session_name, "control", pane_id, cols, rows)
}

fn build_herdr_terminal_session_command(
    session_name: &str,
    mode: &str,
    pane_id: &str,
    cols: i64,
    rows: i64,
) -> Result<String, String> {
    let cols = cols.to_string();
    let rows = rows.to_string();
    build_herdr_command(
        session_name,
        &[
            "terminal", "session", mode, pane_id, "--cols", &cols, "--rows", &rows,
        ],
    )
}

/// 모바일에서 누른 키를 Herdr pane에 넣는다.
/// `send-text`는 제어 문자와 `--help`처럼 `-`로 시작하는 입력도 원문 그대로 전달하지만,
/// 프로세스 인자는 NUL을 담을 수 없어 그 바이트만 이름 있는 키로 전한다.
pub fn build_herdr_write_pane_command(
    session_name: &str,
    pane_id: &str,
    input: &str,
) -> Result<String, String> {
    let mut commands: Vec<String> = Vec::new();
    for (index, text) in input.split('\0').enumerate() {
        if index > 0 {
            commands.push(build_herdr_command(
                session_name,
                &["pane", "send-keys", pane_id, "ctrl+space"],
            )?);
        }
        if !text.is_empty() {
            commands.push(build_herdr_command(
                session_name,
                &["pane", "send-text", pane_id, text],
            )?);
        }
    }
    Ok(commands.join(" && "))
}

/// Herdr에는 스크롤·복사 모드 명령이 없어 분할과 확대만 지원한다. 지난 출력은 모바일 화면을 손가락으로 밀어 본다
pub fn build_herdr_pane_shortcut_command(
    session_name: &str,
    pane_id: &str,
    action: &str,
) -> Result<Option<String>, String> {
    let arguments: &[&str] = match action {
        "split-right" => &[
            "pane",
            "split",
            pane_id,
            "--direction",
            "right",
            "--no-focus",
        ],
        "split-down" => &[
            "pane",
            "split",
            pane_id,
            "--direction",
            "down",
            "--no-focus",
        ],
        "toggle-fullscreen" => &["pane", "zoom", pane_id, "--toggle"],
        _ => return Ok(None),
    };
    build_herdr_command(session_name, arguments).map(Some)
}

pub fn build_herdr_close_pane_command(session_name: &str, pane_id: &str) -> Result<String, String> {
    build_herdr_command(session_name, &["pane", "close", pane_id])
}
