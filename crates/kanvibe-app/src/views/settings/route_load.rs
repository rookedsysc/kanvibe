//! 라우트 조회 상태(TS `useRouteLoadStatus`). 늦어진 조회를 빈 화면이나 기본값으로 바꾸지 않고,
//! 5초가 지나면 지연 안내를, 실패하면 사유와 재시도를 보인다. 데이터는 실제 응답으로만 채운다.

use std::time::Duration;

use gpui_kit::{Context, Task};

/// TS `INITIAL_DESKTOP_LOAD_TIMEOUT_MS`
pub const INITIAL_LOAD_TIMEOUT: Duration = Duration::from_millis(5000);

#[derive(Default)]
pub struct RouteLoadStatus {
    pub is_load_delayed: bool,
    pub failure_reason: Option<String>,
    /// 지금 조회의 번호. 늦게 도착한 옛 조회 결과를 버리는 데 쓴다
    attempt: u64,
    _delay_timer: Option<Task<()>>,
}

impl RouteLoadStatus {
    /// 새 조회를 시작하고 그 번호를 돌려준다. `status_of`는 뷰에서 이 상태를 꺼내는 함수다
    pub fn begin<V: 'static>(
        &mut self,
        cx: &mut Context<V>,
        status_of: fn(&mut V) -> &mut RouteLoadStatus,
    ) -> u64 {
        self.attempt += 1;
        let attempt = self.attempt;
        let timer = cx.background_executor().timer(INITIAL_LOAD_TIMEOUT);
        self._delay_timer = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |view, cx| {
                let status = status_of(view);
                if status.attempt == attempt {
                    status.is_load_delayed = true;
                    cx.notify();
                }
            });
        }));
        attempt
    }

    pub fn is_current(&self, attempt: u64) -> bool {
        self.attempt == attempt
    }

    pub fn succeed(&mut self) {
        self._delay_timer = None;
        self.is_load_delayed = false;
        self.failure_reason = None;
    }

    pub fn fail(&mut self, reason: String) {
        self._delay_timer = None;
        self.is_load_delayed = false;
        self.failure_reason = Some(reason);
    }

    /// 재시도 직전. 실패 사유를 지운다
    pub fn clear_failure(&mut self) {
        self.failure_reason = None;
    }
}
