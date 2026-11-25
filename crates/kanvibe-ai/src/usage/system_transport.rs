//! 실제 네트워크와 하위 프로세스. 시험에서는 가짜로 바꾸고 앱에서만 이 구현을 쓴다.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::usage::context::{
    HttpRequest, HttpResponse, HttpTransport, ProcessLauncher, ProcessOutput, ProcessRequest,
};

/// `ureq`(rustls)로 보내는 HTTP. 상태 코드는 오류로 바꾸지 않고 그대로 돌려줘 호출부가 401·429를 가르게 한다
pub struct UreqHttpTransport {
    agent: ureq::Agent,
}

impl Default for UreqHttpTransport {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl UreqHttpTransport {
    fn run_with_body<S: ureq::AsSendBody>(
        &self,
        request: Result<ureq::http::Request<S>, ureq::http::Error>,
        timeout: Option<Duration>,
    ) -> Result<ureq::http::Response<ureq::Body>, String> {
        let request = request.map_err(|error| error.to_string())?;
        let configured = self
            .agent
            .configure_request(request)
            .timeout_global(timeout)
            .build();
        self.agent
            .run(configured)
            .map_err(|error| error.to_string())
    }
}

impl HttpTransport for UreqHttpTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let timeout = Some(Duration::from_millis(request.timeout_ms));
        let mut builder = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        // 본문이 없는 GET에 빈 본문을 실으면 거절되므로 본문 유무에 따라 요청 모양을 나눈다
        let response = match &request.body {
            Some(body) => self.run_with_body(builder.body(body.clone()), timeout),
            None => self.run_with_body(builder.body(()), timeout),
        };
        let mut response = response?;
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string())))
            .collect();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?;
        Ok(HttpResponse {
            status: response.status().as_u16(),
            headers,
            body,
        })
    }
}

/// 환경을 비운 뒤 요청에 담긴 값만 넣어 실행한다. 시간 초과나 출력 상한을 넘기면 끊고 `exit_code`를 비운다
pub struct SystemProcessLauncher;

const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

fn read_pipe(
    pipe: Option<impl Read + Send + 'static>,
    limit: usize,
) -> JoinHandle<Option<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe?.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() <= limit).then_some(bytes)
    })
}

fn wait_with_deadline(child: &mut Child, timeout_ms: u64) -> Option<i32> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return status.code();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(WAIT_POLL_INTERVAL);
    }
}

impl ProcessLauncher for SystemProcessLauncher {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        let spawned = Command::new(&request.program)
            .args(&request.args)
            .env_clear()
            .envs(&request.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let Ok(mut child) = spawned else {
            return ProcessOutput::default();
        };
        let stdout = read_pipe(child.stdout.take(), request.max_output_bytes);
        let stderr = read_pipe(child.stderr.take(), request.max_output_bytes);
        let exit_code = wait_with_deadline(&mut child, request.timeout_ms);
        let stdout = stdout.join().ok().flatten();
        let stderr = stderr.join().ok().flatten();
        let is_within_limit = stdout.is_some() && stderr.is_some();
        ProcessOutput {
            exit_code: exit_code.filter(|_| is_within_limit),
            stdout: String::from_utf8_lossy(&stdout.unwrap_or_default()).into_owned(),
            stderr: String::from_utf8_lossy(&stderr.unwrap_or_default()).into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn request(program: &str, args: &[&str], environment: &[(&str, &str)]) -> ProcessRequest {
        ProcessRequest {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            environment: environment
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>(),
            timeout_ms: 5_000,
            max_output_bytes: 1024,
        }
    }

    #[test]
    fn child_sees_only_the_given_environment() {
        let output = SystemProcessLauncher.run(&request("/usr/bin/env", &[], &[("ONLY", "this")]));
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(output.stdout, "ONLY=this\n");
    }

    #[test]
    fn missing_program_and_timeouts_have_no_exit_code() {
        assert_eq!(
            SystemProcessLauncher
                .run(&request("/nonexistent/kanvibe-cli", &[], &[]))
                .exit_code,
            None
        );
        let mut slow = request("/bin/sleep", &["5"], &[]);
        slow.timeout_ms = 50;
        assert_eq!(SystemProcessLauncher.run(&slow).exit_code, None);
    }

    #[test]
    fn exit_codes_pass_through() {
        assert_eq!(
            SystemProcessLauncher
                .run(&request("/bin/sh", &["-c", "exit 44"], &[]))
                .exit_code,
            Some(44)
        );
    }
}
