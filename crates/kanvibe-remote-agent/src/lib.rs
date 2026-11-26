//! SSH 원격 호스트에서 도는 KanVibe daemon과 원격 터미널 agent(TS `remoteHostServer`, `remoteTerminalAgent`).
//!
//! 원격에는 이 crate의 정적 바이너리 하나만 올린다. 데스크탑이 SSH stdin으로 올리고 sha256으로 확인한 뒤
//! `host-bridge`(daemon 연결), `host-serve`(daemon 본체), `terminal-agent`(모바일 WSS agent) 가운데 하나로 실행한다.
//! 데스크탑 쪽(`kanvibe-remote-host`)도 같은 프레이밍과 부트스트랩 명령을 이 crate에서 가져다 쓴다.

pub mod bootstrap;
pub mod daemon;
pub mod environment;
pub mod outbox;
pub mod protocol;
pub mod terminal_agent;

use std::sync::Arc;

use daemon::pty_launcher::NativePtyLauncher;
use daemon::{HostServer, HostServerOptions};

/// 실행 파일 인자(`<mode> <namespace> <buildHash>`)를 처리하고 종료 코드를 돌려준다
pub fn run_command_line(arguments: &[String]) -> i32 {
    let [mode, namespace, build_hash] = arguments else {
        eprintln!(
            "usage: kanvibe-remote-agent <host-bridge|host-serve|terminal-agent> <namespace> <buildHash>"
        );
        return 2;
    };
    let result = match mode.as_str() {
        "host-bridge" => {
            daemon::bridge::run_bridge(namespace, build_hash, std::io::stdin(), std::io::stdout())
        }
        "host-serve" => serve_daemon(namespace, build_hash),
        "terminal-agent" => terminal_agent::run_with_stdio(build_hash),
        _ => Err("Unknown agent mode".into()),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

/// daemon을 띄우고 `stop` 레코드나 SIGTERM·SIGINT를 받을 때까지 돈다
fn serve_daemon(namespace: &str, build_hash: &str) -> Result<(), String> {
    let signals = block_termination_signals();
    let server = Arc::new(HostServer::start(HostServerOptions {
        namespace: namespace.to_string(),
        build_hash: build_hash.to_string(),
        root_directory: None,
        pty_launcher: Arc::new(NativePtyLauncher),
    })?);
    let signalled = Arc::clone(&server);
    std::thread::spawn(move || {
        let mut signal = 0;
        // SAFETY: 막아 둔 신호 집합만 기다린다
        unsafe { libc::sigwait(&signals, &mut signal) };
        signalled.close();
    });
    server.wait_for_stop_request();
    server.close();
    Ok(())
}

/// 종료 신호를 모든 스레드에서 막고 전용 스레드가 `sigwait`로 받게 한다. 자식 프로세스는 신호 마스크를 초기화한 채 시작한다
fn block_termination_signals() -> libc::sigset_t {
    // SAFETY: 지역 sigset_t를 초기화해 지금 스레드(이후 만드는 스레드가 물려받는다)의 마스크에만 더한다
    unsafe {
        let mut signals: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut signals);
        libc::sigaddset(&mut signals, libc::SIGTERM);
        libc::sigaddset(&mut signals, libc::SIGINT);
        libc::pthread_sigmask(libc::SIG_BLOCK, &signals, std::ptr::null_mut());
        signals
    }
}
