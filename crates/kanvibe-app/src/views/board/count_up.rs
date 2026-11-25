//! 숫자를 이전 값에서 새 값으로 굴린다(TS `CountUpNumber`). 스스로 갱신되는 카드에서 방금 바뀐 자리를 움직임으로 가리킨다.
//! 첫 값은 굴리지 않고, 움직임을 줄이도록 설정한 사용자(TS `prefers-reduced-motion: reduce`)에게는 새 값으로 바로 건너뛴다.

use std::time::Duration;

use gpui_kit::{
    App, AppContext, Context, Global, IntoElement, ParentElement, Render, SharedString, Styled,
    Task, Window, div,
};

use crate::qa::targets::{QaTargetBehavior, QaTargetDescription, qa_target_layer_with};

/// 한 번 굴러가는 데 걸리는 시간. 폴링 주기보다 훨씬 짧아 다음 값이 오기 전에 항상 멈춘다
const COUNT_UP_DURATION: Duration = Duration::from_millis(420);

/// 화면 한 프레임(60Hz)마다 값을 다시 그린다
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// 운영체제의 "움직임 줄이기" 설정. 한 번 읽어 앱 전체가 같이 쓴다
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionPreference {
    pub is_reduced: bool,
}

impl Global for MotionPreference {}

impl MotionPreference {
    /// 아직 읽지 않았으면 운영체제 설정을 백그라운드에서 읽어 둔다. QA 재생은 TS 기준 캡처(Chromium 기본값)처럼 줄이지 않는다
    pub(super) fn ensure_loaded(cx: &mut App) {
        if cx.has_global::<Self>() {
            return;
        }
        cx.set_global(Self::default());
        if cx.has_global::<crate::qa::QaRuntime>() {
            return;
        }
        let query = cx.background_spawn(async { read_os_reduced_motion() });
        cx.spawn(async move |cx| {
            let is_reduced = query.await;
            cx.update(|cx| cx.set_global(Self { is_reduced }));
        })
        .detach();
    }

    fn current(cx: &App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|preference| preference.is_reduced)
    }
}

/// GNOME `enable-animations`(Chromium이 Linux에서 `prefers-reduced-motion`으로 쓰는 GTK 설정)와
/// macOS 손쉬운 사용 "동작 줄이기". 읽지 못하면 줄이지 않는다
fn read_os_reduced_motion() -> bool {
    let (program, arguments, reduced_answer): (&str, &[&str], &str) = if cfg!(target_os = "macos") {
        (
            "defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
            "1",
        )
    } else {
        (
            "gsettings",
            &["get", "org.gnome.desktop.interface", "enable-animations"],
            "false",
        )
    };
    // 설정 조회에 필요한 값만 넘긴다(앱 실행 환경을 통째로 물려주지 않는다)
    let mut command = std::process::Command::new(program);
    command.args(arguments).env_clear();
    for key in [
        "PATH",
        "HOME",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| String::from_utf8_lossy(&output.stdout).trim() == reduced_answer)
}

/// 굴러가는 숫자 하나. `test_id`는 TS `data-testid`이고 QA 대상 이름이 된다
pub struct CountUpNumber {
    displayed: i64,
    test_id: &'static str,
    _animation: Task<()>,
}

impl CountUpNumber {
    pub fn new(value: i64, test_id: &'static str) -> Self {
        Self {
            displayed: value,
            test_id,
            _animation: Task::ready(()),
        }
    }

    /// 새 값을 받는다. 같은 값이면 아무것도 하지 않고, 굴러가는 중이면 지금 보이는 값에서 다시 출발한다
    pub fn set_value(&mut self, value: i64, cx: &mut Context<Self>) {
        let start = self.displayed;
        if start == value {
            return;
        }
        if MotionPreference::current(cx) {
            self.displayed = value;
            self._animation = Task::ready(());
            cx.notify();
            return;
        }
        let executor = cx.background_executor().clone();
        let started_at = executor.now();
        self._animation = cx.spawn(async move |this, cx| {
            loop {
                executor.timer(FRAME_INTERVAL).await;
                let elapsed = executor.now().saturating_duration_since(started_at);
                let progress = (elapsed.as_secs_f64() / COUNT_UP_DURATION.as_secs_f64()).min(1.0);
                let next = count_up_frame_value(start, value, progress);
                let Ok(()) = this.update(cx, |this, cx| {
                    this.displayed = next;
                    cx.notify();
                }) else {
                    return;
                };
                if progress >= 1.0 {
                    return;
                }
            }
        });
    }
}

/// ease-out cubic(`1 - (1 - t)^3`)으로 굴린 값
fn count_up_frame_value(start: i64, end: i64, progress: f64) -> i64 {
    let eased = 1.0 - (1.0 - progress).powi(3);
    (start as f64 + (end - start) as f64 * eased).round() as i64
}

impl Render for CountUpNumber {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let text: SharedString = self.displayed.to_string().into();
        div()
            .relative()
            .child(text.clone())
            .child(qa_target_layer_with(
                QaTargetDescription {
                    role: "status",
                    name: self.test_id.into(),
                    text,
                },
                QaTargetBehavior {
                    selector: Some(format!("[data-testid='{}']", self.test_id).into()),
                    ..QaTargetBehavior::default()
                },
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::count_up_frame_value;

    #[test]
    fn frames_start_at_the_old_value_and_end_at_the_new_value() {
        assert_eq!(count_up_frame_value(340, 348, 0.0), 340);
        assert_eq!(count_up_frame_value(340, 348, 1.0), 348);
        assert!(count_up_frame_value(340, 348, 0.3) > 340);
    }
}
