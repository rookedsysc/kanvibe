//! 사용량 테스트 전용 문맥. 네트워크·하위 프로세스·시계를 가짜로 바꾸고, 홈은 임시 디렉터리를 쓴다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::usage::context::{
    HttpRequest, HttpResponse, HttpTransport, ProcessLauncher, ProcessOutput, ProcessRequest,
    UsageClock, UsageContext, UsagePlatform,
};

type HttpResponder = dyn Fn(&HttpRequest) -> Result<HttpResponse, String> + Send + Sync;
type ProcessResponder = dyn Fn(&ProcessRequest) -> ProcessOutput + Send + Sync;

/// 요청을 기록하고 바꿔 끼울 수 있는 함수로 답하는 HTTP
pub struct FakeHttp {
    responder: Mutex<Box<HttpResponder>>,
    pub requests: Mutex<Vec<HttpRequest>>,
}

impl FakeHttp {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            responder: Mutex::new(Box::new(|_| Err("응답이 정해지지 않았습니다".to_string()))),
            requests: Mutex::new(Vec::new()),
        })
    }

    pub fn respond(
        &self,
        responder: impl Fn(&HttpRequest) -> Result<HttpResponse, String> + Send + Sync + 'static,
    ) {
        *self.responder.lock().unwrap() = Box::new(responder);
    }

    /// 모든 요청에 같은 상태와 본문으로 답한다
    pub fn respond_json(&self, status: u16, body: serde_json::Value) {
        let body = body.to_string();
        self.respond(move |_| Ok(json_response(status, &body)));
    }

    pub fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    pub fn request(&self, index: usize) -> HttpRequest {
        self.requests.lock().unwrap()[index].clone()
    }
}

pub fn json_response(status: u16, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: Vec::new(),
        body: body.to_string(),
    }
}

impl HttpTransport for FakeHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        self.requests.lock().unwrap().push(request.clone());
        (self.responder.lock().unwrap())(request)
    }
}

/// 실행 요청을 기록하고 함수로 답하는 프로세스 실행기
pub struct FakeProcesses {
    responder: Mutex<Box<ProcessResponder>>,
    pub requests: Mutex<Vec<ProcessRequest>>,
}

impl FakeProcesses {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            responder: Mutex::new(Box::new(|_| ProcessOutput::default())),
            requests: Mutex::new(Vec::new()),
        })
    }

    pub fn respond(
        &self,
        responder: impl Fn(&ProcessRequest) -> ProcessOutput + Send + Sync + 'static,
    ) {
        *self.responder.lock().unwrap() = Box::new(responder);
    }

    pub fn arguments(&self) -> Vec<Vec<String>> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.args.clone())
            .collect()
    }

    pub fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl ProcessLauncher for FakeProcesses {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        self.requests.lock().unwrap().push(request.clone());
        (self.responder.lock().unwrap())(request)
    }
}

pub fn exited(exit_code: i32, stdout: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(exit_code),
        stdout: stdout.to_string(),
        stderr: String::new(),
    }
}

/// 테스트가 옮길 수 있는 시계
pub struct FakeClock {
    now_ms: Mutex<f64>,
}

impl FakeClock {
    pub fn set(&self, now_ms: f64) {
        *self.now_ms.lock().unwrap() = now_ms;
    }

    pub fn set_iso(&self, iso: &str) {
        self.set(crate::javascript::parse_date_ms(iso).expect("ISO 시각"));
    }
}

impl UsageClock for FakeClock {
    fn now_ms(&self) -> f64 {
        *self.now_ms.lock().unwrap()
    }
}

/// 가짜 부품과 그 부품을 쓰는 문맥
pub struct TestUsage {
    pub context: UsageContext,
    pub http: Arc<FakeHttp>,
    pub processes: Arc<FakeProcesses>,
    pub clock: Arc<FakeClock>,
    pub warnings: Arc<Mutex<Vec<String>>>,
}

impl TestUsage {
    pub fn new(home_directory: &str) -> Self {
        Self::with(home_directory, UsagePlatform::Linux, &[])
    }

    pub fn with(home_directory: &str, platform: UsagePlatform, variables: &[(&str, &str)]) -> Self {
        let http = FakeHttp::new();
        let processes = FakeProcesses::new();
        let clock = Arc::new(FakeClock {
            now_ms: Mutex::new(
                crate::javascript::parse_date_ms("2026-08-15T10:00:00.000Z").unwrap(),
            ),
        });
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let recorded = warnings.clone();
        let process_variables: BTreeMap<String, String> = variables
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        let context = UsageContext {
            home_directory: home_directory.to_string(),
            process_variables: Arc::new(process_variables),
            platform,
            user_name: "tester".to_string(),
            system_binary_directories: Vec::new(),
            http: http.clone(),
            processes: processes.clone(),
            clock: clock.clone(),
            warn: Arc::new(move |message: &str| recorded.lock().unwrap().push(message.to_string())),
            claude_retry_allowed_at_ms: Arc::new(Mutex::new(0.0)),
        };
        Self {
            context,
            http,
            processes,
            clock,
            warnings,
        }
    }
}
