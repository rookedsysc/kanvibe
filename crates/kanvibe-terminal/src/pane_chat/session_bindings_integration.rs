//! 실제 hook 프로세스가 보낸 상태 알림으로 pane 채팅 세션 바인딩이 이어지는지 확인한다.
//! 설치되는 bash hook 원문(`kanvibe-process`)과 바인딩 판정(`kanvibe-ai`)을 함께 돌리는 자리라 두 crate에 모두 기대는 이 crate에 둔다.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use kanvibe_ai::AiResult;
use kanvibe_ai::host::SessionHost;
use kanvibe_ai::pane_chat::process::PaneChatProvider;
use kanvibe_ai::pane_chat::session_bindings::{
    PaneChatBindingContext, PaneChatBindingQuery, PaneChatSessionBindings, PaneChatSessionCatalog,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{AggregatedAiSession, AiSessionMatchScope, AiSessionReaderContext};
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::hooks::shell_script::{
    ShellHookAgentProvider, ShellHookStatus, build_shell_kanvibe_status_updater,
};
use serde_json::Value;

use crate::environment::TerminalHostEnvironment;
use crate::session_command_runner::ShellSessionCommandRunner;

const HOOK_METADATA_TIMEOUT: Duration = Duration::from_secs(10);

/// TS 테스트가 `readClaudeSessions`·`readCodexSessions`를 바꿔 끼운 것처럼, 기록 리더 대신 정해진 세션 하나를 준다
struct AdmittedSessionCatalog {
    session_id: String,
    source_ref: String,
}

impl PaneChatSessionCatalog for AdmittedSessionCatalog {
    fn read_sessions(
        &self,
        provider: PaneChatProvider,
        _context: &AiSessionReaderContext,
    ) -> AiResult<Vec<AggregatedAiSession>> {
        let provider = match provider {
            PaneChatProvider::Codex => AiSessionProvider::Codex,
            _ => AiSessionProvider::Claude,
        };
        Ok(vec![AggregatedAiSession {
            id: self.session_id.clone(),
            provider,
            started_at: None,
            updated_at: None,
            matched_path: None,
            match_scope: AiSessionMatchScope::Worktree,
            title: None,
            first_user_prompt: None,
            message_count: 0,
            source_ref: Some(self.source_ref.clone()),
        }])
    }

    fn find_codex_main_thread_rollout(
        &self,
        _context: &AiSessionReaderContext,
        _thread_id: &str,
    ) -> AiResult<Option<String>> {
        Ok(None)
    }
}

/// 요청 하나를 받아 본문의 `agentSession`을 바인딩에 기록하고 `{}`로 답하는 hook 서버
fn serve_one_hook_request(
    listener: TcpListener,
    bindings: Arc<PaneChatSessionBindings>,
    context: PaneChatBindingContext,
    posted: mpsc::Sender<()>,
) {
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(stream.try_clone().expect("소켓 복제"));
        let mut content_length = 0;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body).expect("요청 본문");
        let input: Value = serde_json::from_slice(&body).expect("JSON 본문");
        bindings.record_pane_chat_session_binding(&context, &input["agentSession"]);
        let mut stream = stream;
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
        let _ = posted.send(());
    });
}

/// 에이전트 CLI처럼 hook을 자식으로 돌리고 계속 살아 있는 프로세스
fn spawn_agent(directory: &std::path::Path, provider: &str, hook: &std::path::Path) -> Child {
    let executable = directory.join(provider);
    std::fs::write(
        &executable,
        "import subprocess,sys,time\nsubprocess.run([\"bash\",sys.argv[1]],input=sys.argv[2],text=True,check=True)\ntime.sleep(30)\n",
    )
    .expect("에이전트 스크립트");
    let payload = serde_json::json!({
        "session_id": format!("session-{provider}"),
        "cwd": directory,
        "transcript_path": "/ignored/client-path.jsonl",
    });
    Command::new("python3")
        .arg(&executable)
        .arg(hook)
        .arg(payload.to_string())
        .current_dir(directory)
        .env_clear()
        .envs(&TerminalHostEnvironment::for_current_process().shell_environment)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("python3 실행")
}

fn process_start_time(pid: u32) -> String {
    let output = Command::new("ps")
        .args(["-ww", "-p", &pid.to_string(), "-o", "lstart="])
        .output()
        .expect("ps 실행");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn stop_agent(mut child: Child) {
    // SAFETY: 이 테스트가 띄운 자식 프로세스에만 신호를 보낸다
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    let _ = child.wait();
}

/// git 저장소 하나와 그 저장소를 보는 바인딩 문맥
struct HookFixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    admitted: String,
    task_id: String,
}

impl HookFixture {
    fn new(provider_name: &str) -> Self {
        let directory = tempfile::Builder::new()
            .prefix("kanvibe-hook-process-")
            .tempdir()
            .expect("임시 디렉터리");
        let root = directory.path().canonicalize().expect("실제 경로");
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status();
        assert!(status.expect("git").success());
        Self {
            admitted: root.join("admitted.jsonl").to_string_lossy().into_owned(),
            root,
            _directory: directory,
            task_id: format!("fixture-{provider_name}"),
        }
    }

    fn binding_context(&self) -> PaneChatBindingContext {
        let root_text = self.root.to_string_lossy().into_owned();
        let runner = Arc::new(ShellSessionCommandRunner::new(Arc::new(
            SystemShellCommandRunner::for_current_process(),
        )));
        let host = SessionHost::local(&root_text, Default::default(), runner);
        let mut reader = AiSessionReaderContext::new(host, Some(&root_text));
        reader.repo_path = Some(root_text);
        PaneChatBindingContext {
            task_id: self.task_id.clone(),
            reader,
        }
    }

    /// TS 테스트처럼 TASK_ID와 KANVIBE_URL을 앞에 두고 실제 설치되는 상태 갱신 조각을 붙인 hook
    fn write_hook(&self, port: u16, hook_provider: ShellHookAgentProvider) -> PathBuf {
        let hook = self.root.join("hook.sh");
        let updater =
            build_shell_kanvibe_status_updater(ShellHookStatus::Progress, Some(hook_provider));
        let task_id = &self.task_id;
        let body =
            format!("TASK_ID=\"{task_id}\"\nKANVIBE_URL=\"http://127.0.0.1:{port}\"\n{updater}");
        std::fs::write(&hook, body).expect("hook");
        hook
    }
}

fn binds_real_stdin_session(
    provider: PaneChatProvider,
    provider_name: &str,
    hook_provider: ShellHookAgentProvider,
) {
    let fixture = HookFixture::new(provider_name);
    let bindings = Arc::new(PaneChatSessionBindings::new(Arc::new(
        AdmittedSessionCatalog {
            session_id: format!("session-{provider_name}"),
            source_ref: fixture.admitted.clone(),
        },
    )));
    let listener = TcpListener::bind("127.0.0.1:0").expect("hook 서버");
    let port = listener.local_addr().expect("포트").port();
    let (posted_sender, posted) = mpsc::channel();
    serve_one_hook_request(
        listener,
        bindings.clone(),
        fixture.binding_context(),
        posted_sender,
    );
    let hook = fixture.write_hook(port, hook_provider);
    let child = spawn_agent(&fixture.root, provider_name, &hook);

    let received = posted.recv_timeout(HOOK_METADATA_TIMEOUT);
    let query = PaneChatBindingQuery {
        task_id: fixture.task_id.clone(),
        ssh_host: None,
        provider,
        provider_pid: i64::from(child.id()),
        process_start_time: process_start_time(child.id()),
    };
    let found = bindings.find_pane_chat_session_binding(&query);
    let status = std::fs::read_to_string(fixture.root.join(".kanvibe/status.json"));
    stop_agent(child);

    received.expect("hook metadata timeout");
    let binding = found.expect("바인딩 조회").expect("바인딩");
    assert_eq!(binding.session_id, format!("session-{provider_name}"));
    assert_eq!(binding.source_ref, fixture.admitted);
    let status: Value = serde_json::from_str(&status.expect("status.json")).expect("JSON");
    assert_eq!(status["status"], "progress");
}

// parity: TS-aiSessions-paneChatSessionBindingsIntegration-ad3b95
#[test]
fn ancestor_pid_and_start_time_bind_the_real_stdin_session() {
    binds_real_stdin_session(
        PaneChatProvider::Claude,
        "claude",
        ShellHookAgentProvider::Claude,
    );
    binds_real_stdin_session(
        PaneChatProvider::Codex,
        "codex",
        ShellHookAgentProvider::Codex,
    );
}
