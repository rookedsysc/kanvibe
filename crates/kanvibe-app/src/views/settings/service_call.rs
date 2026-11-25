//! 설정 계열 화면(설정·단축키·AI 계정·pane 레이아웃)이 서비스를 부르는 한 가지 길.
//! 호출은 백그라운드에서 돌리고, QA 모드에서는 TS IPC 이름(`namespace.method`)으로 고장 주입 판정을 먼저 거친다.
//! 고장이 `result`면 실제 호출 대신 주입된 값을 응답 모양으로 읽어 돌려준다(states.yaml `value_ref`).

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, AppContext, BackgroundExecutor, Task};
use kanvibe_db::Database;
use serde::de::DeserializeOwned;

use crate::qa::faults::FaultOutcome;
use crate::services::{AppServices, ServiceFailure, ServiceResult};

/// 데이터 디렉터리 없이 뜬 앱. TS에는 없던 상태라 사유 문구를 따로 둔다
const DATABASE_UNAVAILABLE: &str = "데이터베이스가 열려 있지 않습니다";
/// 화면 전용 묶음을 돌려주는 조회에 값 대체 고장을 건 경우
const UNSUPPORTED_SUBSTITUTION: &str = "이 조회는 값 대체 고장을 지원하지 않습니다";

/// 서비스 호출 하나. `target`은 고장 주입 대상 이름, `arguments`는 인자 JSON 문자열(고장 `match` 판정용)이다
pub fn call_service<T, F>(
    cx: &App,
    target: &'static str,
    arguments: String,
    work: F,
) -> Task<ServiceResult<T>>
where
    T: DeserializeOwned + Send + 'static,
    F: FnOnce() -> ServiceResult<T> + Send + 'static,
{
    call_service_with(
        cx,
        target,
        &arguments,
        |value| serde_json::from_value(value).map_err(failure),
        work,
    )
}

/// 열린 DB가 필요한 호출. DB가 없으면 고장 판정 뒤 실패로 끝난다
pub fn call_database_service<T, F>(
    cx: &App,
    target: &'static str,
    arguments: String,
    work: F,
) -> Task<ServiceResult<T>>
where
    T: DeserializeOwned + Send + 'static,
    F: FnOnce(&Database) -> ServiceResult<T> + Send + 'static,
{
    let work = with_database(cx, work);
    call_service(cx, target, arguments, work)
}

/// 응답 값 대체를 받지 않는 DB 조회(화면 전용 묶음을 돌려주는 조회). `result` 고장은 실패로 본다
pub fn call_database_read<T, F>(cx: &App, target: &'static str, work: F) -> Task<ServiceResult<T>>
where
    T: Send + 'static,
    F: FnOnce(&Database) -> ServiceResult<T> + Send + 'static,
{
    let work = with_database(cx, work);
    call_service_with(
        cx,
        target,
        "[]",
        |_| Err(failure(UNSUPPORTED_SUBSTITUTION)),
        work,
    )
}

fn with_database<T, F>(cx: &App, work: F) -> impl FnOnce() -> ServiceResult<T> + Send + 'static
where
    F: FnOnce(&Database) -> ServiceResult<T> + Send + 'static,
{
    let database: Option<Arc<Database>> = cx.global::<AppServices>().database().cloned();
    move || match database {
        Some(database) => work(&database),
        None => Err(failure(DATABASE_UNAVAILABLE)),
    }
}

fn call_service_with<T, S, F>(
    cx: &App,
    target: &str,
    arguments: &str,
    substitute: S,
    work: F,
) -> Task<ServiceResult<T>>
where
    T: Send + 'static,
    S: FnOnce(serde_json::Value) -> ServiceResult<T> + Send + 'static,
    F: FnOnce() -> ServiceResult<T> + Send + 'static,
{
    let fault = check_fault(cx, target, arguments);
    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        match apply_fault(fault, &executor).await? {
            Some(injected) => substitute(injected),
            None => work(),
        }
    })
}

/// 오류 값을 화면이 보일 사유 문구로 바꾼다(TS `error.message`)
pub fn failure(error: impl std::fmt::Display) -> ServiceFailure {
    ServiceFailure {
        reason: error.to_string(),
    }
}

fn check_fault(cx: &App, target: &str, arguments: &str) -> FaultOutcome {
    cx.try_global::<AppServices>()
        .and_then(AppServices::faults)
        .map(|faults| faults.check(target, arguments))
        .unwrap_or(FaultOutcome::Proceed)
}

/// 고장 판정대로 기다리거나 실패한다. 값 대체면 그 값을 돌려준다
async fn apply_fault(
    fault: FaultOutcome,
    executor: &BackgroundExecutor,
) -> ServiceResult<Option<serde_json::Value>> {
    match fault {
        FaultOutcome::Proceed => Ok(None),
        FaultOutcome::Result(value) => Ok(Some(value)),
        FaultOutcome::Hang => std::future::pending().await,
        FaultOutcome::Delay(wait) => {
            executor.timer(wait).await;
            Ok(None)
        }
        FaultOutcome::Fail { message, after } => {
            if after > Duration::ZERO {
                executor.timer(after).await;
            }
            Err(ServiceFailure { reason: message })
        }
    }
}
