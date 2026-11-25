//! 사용량 조회가 바깥 세계와 만나는 자리. HTTP, 하위 프로세스, 시계, 경고 출력을 trait으로 받고,
//! 홈 디렉터리·플랫폼·사용자명·환경 변수 값은 앱이 시작할 때 한 번 읽어 넘긴다. 이 모듈은 프로세스 환경을 직접 읽지 않는다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// HTTP 요청 하나. TS `fetch(url, { method, headers, body, signal: AbortSignal.timeout(ms) })`에 해당한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    /// 넣은 순서를 지킨다
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    /// TS `response.ok`
    pub fn is_ok(&self) -> bool {
        (200..=299).contains(&self.status)
    }

    /// TS `response.headers.get(name)`. 이름은 대소문자를 가리지 않는다
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// TS `response.json()`. 본문이 JSON이 아니면 `None`이다
    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_str(&self.body).ok()
    }
}

/// 네트워크 호출. 연결 실패와 시간 초과는 `Err`다(TS에서 `fetch`가 던지는 경우)
pub trait HttpTransport: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String>;
}

/// 하위 프로세스 하나. 환경은 여기 담긴 값만 넘기고 부모 환경은 물려주지 않는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRequest {
    pub program: String,
    pub args: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
}

/// `exit_code`가 `None`이면 실행하지 못했거나, 시간 초과·출력 상한으로 끊겼거나, 신호로 끝났다(TS `error.code`가 숫자가 아닌 경우)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub trait ProcessLauncher: Send + Sync {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput;
}

/// TS `Date.now()`
pub trait UsageClock: Send + Sync {
    fn now_ms(&self) -> f64;
}

/// 맥에서만 나는 실패의 단서. 토큰 같은 비밀값은 넘기지 않는다
pub type WarningSink = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsagePlatform {
    Darwin,
    Linux,
    Other,
}

/// 사용량 조회와 계정 관리가 함께 쓰는 실행 문맥
#[derive(Clone)]
pub struct UsageContext {
    pub home_directory: String,
    /// 앱이 시작할 때 읽어 둔 환경 변수 값. provider 위치 변수(`CLAUDE_CONFIG_DIR` 등)와
    /// CLI 자식 환경을 만들 때 고르는 값(`PATH`, `LANG` 등)이 들어 있다
    pub process_variables: Arc<BTreeMap<String, String>>,
    pub platform: UsagePlatform,
    /// TS `os.userInfo().username`
    pub user_name: String,
    /// PATH 밖에서 CLI를 찾을 시스템 디렉터리(`/usr/local/bin`, `/opt/homebrew/bin`)
    pub system_binary_directories: Vec<String>,
    pub http: Arc<dyn HttpTransport>,
    pub processes: Arc<dyn ProcessLauncher>,
    pub clock: Arc<dyn UsageClock>,
    pub warn: WarningSink,
    /// 429가 알려준 Claude 재시도 가능 시각. 계정을 나눠도 한도는 같이 걸리므로 계정별이 아니라 문맥 하나가 기억한다
    pub claude_retry_allowed_at_ms: Arc<Mutex<f64>>,
}

impl UsageContext {
    pub fn now_ms(&self) -> f64 {
        self.clock.now_ms()
    }

    /// 지금 시각의 ISO 문자열(TS `new Date().toISOString()`)
    pub fn now_iso(&self) -> String {
        crate::javascript::iso_string_from_ms(self.now_ms()).unwrap_or_default()
    }

    pub fn variable(&self, name: &str) -> Option<&str> {
        self.process_variables.get(name).map(String::as_str)
    }
}

/// 실제 시계
pub struct SystemClock;

impl UsageClock for SystemClock {
    fn now_ms(&self) -> f64 {
        crate::javascript::now_ms()
    }
}
