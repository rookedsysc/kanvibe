//! git·gh·ssh 프로세스를 띄우거나 `Condvar`로 다른 작업을 기다리는 동기 서비스 호출을 GPUI background 풀 밖에서 돌린다.
//! GPUI Linux background 풀은 코어 수만큼의 고정 스레드라, 느린 원격 호출 몇 개가 풀을 다 막으면 앱의 다른 background 작업이 줄을 선다.
//! 그래서 호출마다 전용 스레드를 하나 띄우고, background 작업은 그 결과만 기다린다.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

pub(crate) const BLOCKING_CALL_THREAD_NAME: &str = "kanvibe-blocking-call";

/// `call`을 전용 스레드에서 실행하고 그 결과를 돌려준다. `call`이 패닉하면 기다리는 쪽에서 같은 패닉을 다시 일으킨다.
/// GPUI 결정적 테스트 스케줄러(`BackgroundExecutor::is_test`)는 다른 스레드에서 깨우는 것을 막으므로, 그때는 이 자리에서 실행한다
pub(crate) async fn run_blocking_call<T: Send + 'static>(
    is_deterministic_test: bool,
    call: impl FnOnce() -> T + Send + 'static,
) -> T {
    if is_deterministic_test {
        return call();
    }
    run_on_dedicated_thread(call).await
}

fn run_on_dedicated_thread<T: Send + 'static>(
    call: impl FnOnce() -> T + Send + 'static,
) -> impl Future<Output = T> {
    let (sender, receiver) = futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name(BLOCKING_CALL_THREAD_NAME.to_string())
        .spawn(move || {
            let _ = sender.send(catch_unwind(AssertUnwindSafe(call)));
        })
        .expect("blocking 서비스 호출 스레드를 만들 수 없다");
    async move {
        match receiver
            .await
            .expect("blocking 서비스 호출 스레드가 결과 없이 끝났다")
        {
            Ok(value) => value,
            Err(panic_payload) => resume_unwind(panic_payload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_call_runs_on_the_dedicated_blocking_thread() {
        let thread_name = futures::executor::block_on(run_blocking_call(false, || {
            std::thread::current().name().map(str::to_string)
        }));
        assert_eq!(thread_name.as_deref(), Some(BLOCKING_CALL_THREAD_NAME));
    }

    #[test]
    fn the_deterministic_test_scheduler_runs_the_call_in_place() {
        let caller = std::thread::current().id();
        let call_thread =
            futures::executor::block_on(run_blocking_call(true, || std::thread::current().id()));
        assert_eq!(call_thread, caller);
    }

    #[test]
    fn a_panicking_call_panics_the_waiting_side_with_the_same_message() {
        let waiting = std::panic::catch_unwind(|| {
            futures::executor::block_on(run_blocking_call(false, || -> () {
                panic!("서비스 호출 실패")
            }))
        });
        let payload = waiting.expect_err("기다리는 쪽도 패닉해야 한다");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"서비스 호출 실패"));
    }
}
