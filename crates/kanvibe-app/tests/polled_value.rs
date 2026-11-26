//! 공용 주기 조회(TS `usePolledValue`). 보드 변경 집계·실행중 pane·세션 패널, 작업 상세 세션·호출 그래프·변경 파일이
//! 모두 이 조회 하나로 돈다. 조회 대상이 바뀌면 화면이 조회를 새로 만든다(TS의 `read` 교체).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::gpui;
use gpui_kit::{
    App, Context, Entity, IntoElement, Render, Task, TestAppContext, VisualTestContext, Window, div,
};
use kanvibe_app::polled_value::{PollingSubscription, start_polling};

/// 조회 한 번의 응답. 바로 끝나거나, 시험이 나중에 끝낸다
enum Response {
    Ready(Result<String, ()>),
    Deferred(oneshot::Receiver<Result<String, ()>>),
}

/// 미리 정한 응답을 차례로 돌려주고 불린 횟수를 센다
#[derive(Clone, Default)]
struct ScriptedRead {
    calls: Rc<Cell<usize>>,
    responses: Rc<RefCell<VecDeque<Response>>>,
}

impl ScriptedRead {
    fn ready(values: &[Result<&str, ()>]) -> Self {
        let script = Self::default();
        for value in values {
            script.push(Response::Ready(value.map(str::to_string)));
        }
        script
    }

    fn deferred() -> (Self, oneshot::Sender<Result<String, ()>>) {
        let script = Self::default();
        let (sender, receiver) = oneshot::channel();
        script.push(Response::Deferred(receiver));
        (script, sender)
    }

    fn push(&self, response: Response) {
        self.responses.borrow_mut().push_back(response);
    }

    fn read(&self, cx: &mut App) -> Task<Result<String, ()>> {
        self.calls.set(self.calls.get() + 1);
        let response = self.responses.borrow_mut().pop_front();
        cx.foreground_executor().spawn(async move {
            match response {
                Some(Response::Ready(result)) => result,
                Some(Response::Deferred(receiver)) => receiver.await.unwrap_or(Err(())),
                None => Err(()),
            }
        })
    }
}

/// 조회를 쥐는 화면
struct Host {
    polled: Option<PollingSubscription<String>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct Harness {
    host: Entity<Host>,
    cx: VisualTestContext,
}

fn open_host(cx: &mut TestAppContext, is_window_active: bool) -> Harness {
    let (host, visual) = cx.add_window_view(|_, _| Host { polled: None });
    let mut visual = visual.clone();
    if is_window_active {
        visual.update(|window, _| window.activate_window());
    } else {
        visual.deactivate_window();
    }
    visual.run_until_parked();
    Harness { host, cx: visual }
}

impl Harness {
    /// 조회 대상을 (다시) 정한다. 이전 조회는 버려진다
    fn poll(&mut self, script: &ScriptedRead, interval: Duration) {
        let host = self.host.clone();
        let script = script.clone();
        self.cx.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.polled = Some(start_polling(
                    interval,
                    move |cx| script.read(cx),
                    |_: &mut Host, _, cx| cx.notify(),
                    window,
                    cx,
                ));
            })
        });
        self.cx.run_until_parked();
    }

    fn value(&mut self) -> Option<String> {
        let host = self.host.clone();
        self.cx.update(|_, cx| {
            host.read(cx)
                .polled
                .as_ref()
                .and_then(|polled| polled.value(cx).cloned())
        })
    }

    fn set_window_active(&mut self, is_active: bool) {
        if is_active {
            self.cx.update(|window, _| window.activate_window());
        } else {
            self.cx.deactivate_window();
        }
        self.cx.run_until_parked();
    }

    fn advance(&mut self, duration: Duration) {
        self.cx.executor().advance_clock(duration);
        self.cx.run_until_parked();
    }
}

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);

#[gpui::test]
fn the_previous_query_is_hidden_while_the_next_is_pending_or_rejected(cx: &mut TestAppContext) {
    let mut harness = open_host(cx, true);
    harness.poll(&ScriptedRead::ready(&[Ok("A")]), SECOND);
    assert_eq!(harness.value().as_deref(), Some("A"));
    let (next, sender) = ScriptedRead::deferred();
    harness.poll(&next, SECOND);
    assert_eq!(harness.value(), None);
    sender.send(Err(())).unwrap();
    harness.cx.run_until_parked();
    assert_eq!(harness.value(), None);
}

#[gpui::test]
fn a_failed_later_poll_keeps_the_same_querys_last_success(cx: &mut TestAppContext) {
    let mut harness = open_host(cx, true);
    let script = ScriptedRead::ready(&[Ok("A"), Err(())]);
    harness.poll(&script, SECOND);
    assert_eq!(harness.value().as_deref(), Some("A"));
    harness.advance(SECOND);
    assert_eq!(script.calls.get(), 2);
    assert_eq!(harness.value().as_deref(), Some("A"));
}

#[gpui::test]
fn a_cancelled_query_response_never_covers_the_current_query(cx: &mut TestAppContext) {
    let mut harness = open_host(cx, true);
    let (old, old_sender) = ScriptedRead::deferred();
    harness.poll(&old, SECOND);
    harness.poll(&ScriptedRead::ready(&[Ok("B")]), SECOND);
    assert_eq!(harness.value().as_deref(), Some("B"));
    // 버려진 조회는 이미 취소되어 응답을 받을 쪽이 없다
    let _ = old_sender.send(Ok("A".into()));
    harness.cx.run_until_parked();
    assert_eq!(harness.value().as_deref(), Some("B"));
}

#[gpui::test]
fn hiding_a_read_window_does_not_reread_and_returning_catches_up_once(cx: &mut TestAppContext) {
    let mut harness = open_host(cx, true);
    let script = ScriptedRead::ready(&[Ok("A"), Ok("B")]);
    harness.poll(&script, MINUTE);
    assert_eq!(harness.value().as_deref(), Some("A"));
    harness.set_window_active(false);
    assert_eq!(script.calls.get(), 1);
    assert_eq!(harness.value().as_deref(), Some("A"));
    harness.set_window_active(true);
    assert_eq!(harness.value().as_deref(), Some("B"));
    assert_eq!(script.calls.get(), 2);
}

#[gpui::test]
fn a_window_opened_in_the_background_still_reads_once(cx: &mut TestAppContext) {
    let mut harness = open_host(cx, false);
    let script = ScriptedRead::ready(&[Ok("A"), Ok("B")]);
    harness.poll(&script, SECOND);
    assert_eq!(harness.value().as_deref(), Some("A"));
    // 가려진 동안에는 주기 조회를 돌지 않는다
    harness.advance(SECOND * 5);
    assert_eq!(script.calls.get(), 1);
}

#[gpui::test]
fn toggling_activation_during_a_slow_read_neither_overlaps_nor_drops_it(cx: &mut TestAppContext) {
    let mut harness = open_host(cx, true);
    let (script, sender) = ScriptedRead::deferred();
    harness.poll(&script, MINUTE);
    assert_eq!(script.calls.get(), 1);
    harness.set_window_active(false);
    harness.set_window_active(true);
    assert_eq!(script.calls.get(), 1);
    sender.send(Ok("A".into())).unwrap();
    harness.cx.run_until_parked();
    assert_eq!(harness.value().as_deref(), Some("A"));
}
