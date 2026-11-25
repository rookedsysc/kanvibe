//! 성능 하네스(`parity/tools/perf/run_perf.py`)가 읽는 표식. `--qa --perf-markers`일 때만 켠다.
//!
//! 표식은 stdout 한 줄 `KANVIBE_PERF <이름> <유닉스 epoch ns>`다. TS 쪽 표식(`parity/tools/perf/ts_perf_preload.cjs`)과
//! 같은 시점을 가리키도록, 화면 변화가 그려진 프레임의 "다음 프레임 시작"에서 쓴다. GPUI는 프레임 시작에서 next-frame
//! 콜백을 부른 뒤 그 프레임을 그리므로, 콜백에서 보는 QA 대상 목록은 바로 앞 프레임이 그린 모습이다.
//!
//! - `first_board_paint`: Todo 열 카드가 처음 그려졌다(재생 드라이버가 쓰는 카드 선택자와 같은 대상)
//! - `first_terminal_rows`: 터미널 줄 글이 처음 그려졌다
//! - `key_received`: 창이 키 입력을 받았다(다른 처리기보다 먼저)
//! - `key_frame`: 키 입력 뒤 첫 프레임이 그려졌다(TS의 requestAnimationFrame 두 번과 같은 뜻)
//! - `key_text_change`: 키 입력 뒤 터미널 줄 글이 바뀐 프레임이 그려졌다(에코된 글자)
//! - `startup:<단계>`: 시작 단계(창 열기, 첫 프레임 시작, 첫 보드 조회, hook 서버 등록 등)가 끝난 시각. 콜드 스타트를
//!   단계별로 나눠 보는 용도이고 하네스 지표에는 쓰지 않는다
//!
//! 프레임마다 확인할 때는 프레임만 요청하고 창을 다시 그리게 하지 않는다. 다시 그리게 하면 측정 대상인 그리기 비용이 늘어난다.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui_kit::{AnyWindowHandle, App, Window};

use crate::qa::options::LaunchOptions;
use crate::qa::targets::{DescribedTarget, QaTargetRegistry, TargetQuery};

pub const PERF_MARKERS_SWITCH: &str = "--perf-markers";
const BOARD_CARD_SELECTOR: &str = "[data-kanban-task-card='true'][data-kanban-status='todo']";
const TERMINAL_ROW_ROLE: &str = "row";
const FIRST_PAINT_TIMEOUT: Duration = Duration::from_secs(120);
const TERMINAL_ECHO_TIMEOUT: Duration = Duration::from_secs(3);

/// 시작 단계 표식(`startup:<단계>`)을 쓸지. `--perf-markers`일 때만 켜고, 이 모듈 밖에서는 읽기만 한다
static IS_STARTUP_TIMELINE_ENABLED: AtomicBool = AtomicBool::new(false);

/// 시작 단계 표식을 켠다. 표식이 QA 실행에서만 켜지도록 [`require_qa_for_perf_markers`]를 통과한 뒤 부른다
pub fn enable_startup_timeline() {
    IS_STARTUP_TIMELINE_ENABLED.store(true, Ordering::Relaxed);
}

/// 시작 단계가 끝난 시각을 `KANVIBE_PERF startup:<단계> <ns>`로 남긴다. 표식이 꺼져 있으면 아무것도 하지 않는다
pub fn mark_startup_phase(phase: &str) {
    if IS_STARTUP_TIMELINE_ENABLED.load(Ordering::Relaxed) {
        print_marker(&format!("startup:{phase}"));
    }
}

/// `--perf-markers`를 떼어 내고 켜졌는지와 나머지 인자를 돌려준다
pub fn split_perf_markers_switch(arguments: Vec<String>) -> (bool, Vec<String>) {
    let is_enabled = arguments
        .iter()
        .any(|argument| argument == PERF_MARKERS_SWITCH);
    let remaining = arguments
        .into_iter()
        .filter(|argument| argument != PERF_MARKERS_SWITCH)
        .collect();
    (is_enabled, remaining)
}

/// 표식은 QA 실행에서만 쓴다. 일반 실행에 섞이면 사용자 앱 stdout에 표식이 쌓인다
pub fn require_qa_for_perf_markers(
    launch: LaunchOptions,
    is_perf_markers_enabled: bool,
) -> Result<LaunchOptions, String> {
    match is_perf_markers_enabled && launch.qa.is_none() {
        true => Err(format!(
            "{PERF_MARKERS_SWITCH}는 --qa와 함께만 쓸 수 있습니다"
        )),
        false => Ok(launch),
    }
}

/// 첫 창에 첫 화면 표식을 걸고, 모든 창의 키 입력에 키 표식을 건다
pub fn install_perf_markers(window_handle: AnyWindowHandle, cx: &mut App) -> anyhow::Result<()> {
    window_handle.update(cx, |_, window, _| watch_first_paint(window))?;
    cx.intercept_keystrokes(|_, window, cx| mark_keystroke(window, cx))
        .detach();
    Ok(())
}

fn watch_first_paint(window: &mut Window) {
    let deadline = Instant::now() + FIRST_PAINT_TIMEOUT;
    let board_card = TargetQuery::Described(DescribedTarget {
        selector: Some(BOARD_CARD_SELECTOR.to_string()),
        ..DescribedTarget::default()
    });
    let mut is_first_frame = true;
    poll_each_frame(window, deadline, move |window, cx| {
        // 창을 열 때 GPUI가 미리 그린 장면은 첫 프레임에서야 화면에 나온다. 그래서 첫 프레임 시작에서 보이는 QA 대상은
        // 아직 그려지지 않은 것이고, 둘째 프레임 시작부터 "바로 앞 프레임이 그린 모습"이다
        if std::mem::take(&mut is_first_frame) {
            mark_startup_phase("first_frame_start");
            return false;
        }
        let window_id = window.window_handle().window_id();
        let Some(registry) = cx.try_global::<QaTargetRegistry>() else {
            return true;
        };
        if registry.find_hit(window_id, &board_card).is_some() {
            print_marker("first_board_paint");
            return true;
        }
        let has_terminal_rows = registry
            .painted_texts(window_id, TERMINAL_ROW_ROLE)
            .next()
            .is_some();
        if has_terminal_rows {
            print_marker("first_terminal_rows");
        }
        has_terminal_rows
    });
}

fn mark_keystroke(window: &mut Window, cx: &mut App) {
    print_marker("key_received");
    // 키를 처리한 뒤 첫 프레임 시작 → 그 프레임을 그림 → 다음 프레임 시작에서 표식을 쓴다
    window.on_next_frame(|window, _| window.on_next_frame(|_, _| print_marker("key_frame")));

    // 터미널이 그려져 있을 때만 에코를 기다린다. 보드에서는 프레임을 더 요청하지 않는다
    let Some(rows_before) = terminal_rows_digest(window, cx) else {
        return;
    };
    let deadline = Instant::now() + TERMINAL_ECHO_TIMEOUT;
    poll_each_frame(window, deadline, move |window, cx| {
        let has_changed = terminal_rows_digest(window, cx) != Some(rows_before);
        if has_changed {
            print_marker("key_text_change");
        }
        has_changed
    });
}

/// 마지막으로 그린 터미널 줄 글의 요약값. 터미널 줄이 하나도 없으면 `None`이다
fn terminal_rows_digest(window: &Window, cx: &App) -> Option<u64> {
    let registry = cx.try_global::<QaTargetRegistry>()?;
    let window_id = window.window_handle().window_id();
    let mut hasher = DefaultHasher::new();
    let mut row_count = 0usize;
    for text in registry.painted_texts(window_id, TERMINAL_ROW_ROLE) {
        text.hash(&mut hasher);
        row_count += 1;
    }
    (row_count > 0).then(|| hasher.finish())
}

/// `is_done`이 참을 돌려주거나 기한이 지날 때까지 프레임 시작마다 부른다
fn poll_each_frame(
    window: &mut Window,
    deadline: Instant,
    mut is_done: impl FnMut(&mut Window, &mut App) -> bool + 'static,
) {
    window.on_next_frame(move |window, cx| {
        if is_done(window, cx) || Instant::now() >= deadline {
            return;
        }
        poll_each_frame(window, deadline, is_done);
    });
}

fn print_marker(name: &str) {
    let epoch_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    println!("KANVIBE_PERF {name} {epoch_ns}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qa::options::parse_launch_options;

    fn arguments(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn perf_markers_switch_is_removed_from_the_remaining_arguments() {
        let (is_enabled, remaining) =
            split_perf_markers_switch(arguments("--qa --perf-markers --route /ko"));

        assert!(is_enabled);
        assert_eq!(remaining, arguments("--qa --route /ko"));
    }

    #[test]
    fn perf_markers_are_refused_outside_qa_mode() {
        let normal_launch = parse_launch_options(Vec::new()).unwrap();
        let qa_launch =
            parse_launch_options(arguments("--qa --fixed-now 2026-01-15T03:00:00Z")).unwrap();

        assert!(require_qa_for_perf_markers(normal_launch.clone(), true).is_err());
        assert!(require_qa_for_perf_markers(normal_launch, false).is_ok());
        assert!(require_qa_for_perf_markers(qa_launch, true).is_ok());
    }

    #[test]
    fn perf_markers_stay_off_without_the_switch() {
        let (is_enabled, remaining) = split_perf_markers_switch(arguments("--qa --route /ko"));

        assert!(!is_enabled);
        assert_eq!(remaining, arguments("--qa --route /ko"));
    }
}
