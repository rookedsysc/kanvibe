//! 자동 QA 하네스 모드(`kanvibe --qa-harness`). 스크린샷 재생 드라이버가 TS 드라이버와 같은 결정성을 얻도록 고정 시각, 고정 네트워크 응답,
//! 서비스 고장 주입, 제어 소켓을 제공한다. 모든 입력은 실행 인자로 받고 프로세스 환경은 바꾸지 않는다.

pub mod control;
pub mod faults;
pub mod options;
pub mod perf_markers;
pub mod prompts;
pub mod targets;
pub mod window_focus;

use gpui_kit::Global;

use crate::qa::options::QaOptions;

/// 앱 시각과 고정 응답 표는 런타임 서비스(AI 사용량, 릴리스 확인)도 함께 써서 kanvibe-runtime에 둔다
pub use kanvibe_runtime::{AppClock, NetworkResponse, NetworkResponseTable};

/// QA 모드로 실행 중일 때만 있는 전역 상태
pub struct QaRuntime {
    pub options: QaOptions,
    pub network_responses: NetworkResponseTable,
}

impl Global for QaRuntime {}
