//! 조회 결과를 주기적으로 다시 읽는 값 하나(TS `usePolledValue` + `useIsWindowActive`).
//!
//! 앞선 조회가 아직 끝나지 않았으면 이번 주기를 건너뛰어, 원격처럼 느린 호출에서 요청이 쌓이지 않게 한다.
//! 창이 가려져 있으면 주기 조회를 멈추되, 아직 한 번도 읽지 않았으면 첫 조회는 한다. 그래야 창이 다시 앞으로 왔을 때
//! 빈 화면부터 보이지 않는다. 이미 읽은 창이 가려질 때는 다시 읽지 않고, 다시 앞으로 오면 밀린 변화를 한 번 읽은 뒤
//! 주기를 다시 시작한다. 창 활성 여부만 바뀐 사이에 끝난 조회는 그대로 반영한다.
//! 실패한 조회는 화면을 오류로 덮지 않고 지난 값을 둔 채 다음 주기에 다시 시도한다.
//!
//! 조회 대상(작업 id 등)이 바뀌거나 조회를 끄면 띄운 화면이 이 값을 새로 만들거나 버린다. 버려진 값의 조회는 함께
//! 취소되므로 늦게 끝난 이전 대상의 응답이 지금 대상의 값 위에 덮이지 않고, 새 값은 첫 응답 전까지 비어 있다.

use std::time::Duration;

use gpui_kit::{App, AppContext, Context, Entity, Subscription, Task, Window};

type PolledRead<T> = Box<dyn Fn(&mut App) -> Task<Option<T>>>;

pub struct PolledValue<T> {
    value: Option<T>,
    read: PolledRead<T>,
    interval: Duration,
    is_window_active: bool,
    has_started_read: bool,
    is_reading: bool,
    _read: Task<()>,
    _ticker: Task<()>,
    _window_activation: Subscription,
}

impl<T: 'static> PolledValue<T> {
    /// 만들자마자 한 번 읽고, 창이 활성인 동안 `interval`마다 다시 읽는다
    pub fn new<E: 'static>(
        interval: Duration,
        read: impl Fn(&mut App) -> Task<Result<T, E>> + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let read: PolledRead<T> = Box::new(move |cx| {
            let request = read(cx);
            cx.foreground_executor()
                .spawn(async move { request.await.ok() })
        });
        let window_activation = cx.observe_window_activation(window, |this, window, cx| {
            this.set_window_active(window.is_window_active(), cx)
        });
        let mut polled = Self {
            value: None,
            read,
            interval,
            is_window_active: window.is_window_active(),
            has_started_read: false,
            is_reading: false,
            _read: Task::ready(()),
            _ticker: Task::ready(()),
            _window_activation: window_activation,
        };
        polled.restart(cx);
        polled
    }

    /// 마지막으로 성공한 조회 결과. 첫 응답 전에는 없다
    pub fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }

    fn set_window_active(&mut self, is_window_active: bool, cx: &mut Context<Self>) {
        if self.is_window_active == is_window_active {
            return;
        }
        self.is_window_active = is_window_active;
        self.restart(cx);
    }

    /// 창 활성 여부가 정해질 때마다 TS 효과처럼 다시 건다: 활성이거나 아직 안 읽었으면 바로 읽고, 활성일 때만 주기를 돈다
    fn restart(&mut self, cx: &mut Context<Self>) {
        if self.is_window_active || !self.has_started_read {
            self.read_once(cx);
        }
        if !self.is_window_active {
            self._ticker = Task::ready(());
            return;
        }
        let interval = self.interval;
        self._ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(interval).await;
                if this.update(cx, |this, cx| this.read_once(cx)).is_err() {
                    return;
                }
            }
        });
    }

    fn read_once(&mut self, cx: &mut Context<Self>) {
        if self.is_reading {
            return;
        }
        self.is_reading = true;
        self.has_started_read = true;
        let request = (self.read)(cx);
        self._read = cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                this.is_reading = false;
                if let Some(value) = result {
                    this.value = Some(value);
                    cx.notify();
                }
            });
        });
    }
}

/// 화면이 쥐는 주기 조회 하나. 버리면 조회도 함께 멈춘다
pub struct PollingSubscription<T> {
    polled: Entity<PolledValue<T>>,
    _observe: Subscription,
}

impl<T: 'static> PollingSubscription<T> {
    /// 마지막으로 성공한 조회 결과. 첫 응답 전에는 없다
    pub fn value<'a>(&self, cx: &'a App) -> Option<&'a T> {
        self.polled.read(cx).value()
    }
}

/// 조회를 걸고, 조회가 성공할 때마다 그 값으로 `on_value`를 부른다
pub fn start_polling<V: 'static, T: Clone + 'static, E: 'static>(
    interval: Duration,
    read: impl Fn(&mut App) -> Task<Result<T, E>> + 'static,
    on_value: impl Fn(&mut V, T, &mut Context<V>) + 'static,
    window: &mut Window,
    cx: &mut Context<V>,
) -> PollingSubscription<T> {
    let polled = cx.new(|cx| PolledValue::new(interval, read, window, cx));
    let observe = cx.observe(&polled, move |view, polled, cx| {
        if let Some(value) = polled.read(cx).value().cloned() {
            on_value(view, value, cx);
        }
    });
    PollingSubscription {
        polled,
        _observe: observe,
    }
}
