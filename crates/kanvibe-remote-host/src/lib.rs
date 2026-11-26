//! 원격 SSH 호스트 daemon의 데스크톱 쪽(TS `remoteHost*`, `hookTunnelService`).
//!
//! 바이너리 업로드와 확인, 프레이밍 다중화(명령·PTY·소켓 포워딩·hook), 호스트별 연결 풀, 원격 터미널 agent 실행 경계,
//! hook 터널(`ssh -R`과 daemon hook 재생)을 맡는다. 원격 쪽 바이너리와 프레이밍은 `kanvibe-remote-agent`에 있다.
//!
//! 앱 연결 순서: [`agent_binaries::AgentBinaryCatalog`]로 바이너리 목록을 읽고, [`pool::RemoteHostConnectionPool`]을
//! 만든 뒤, 터미널 레지스트리에는 [`registry_bindings::RegistryHostConnections`], 명령 실행에는
//! [`command_runner::DaemonRoutedCommandRunner`], hook 터널에는 [`registry_bindings::TunnelHostTransport`]를 넘긴다.

pub mod agent_binaries;
pub mod command_runner;
pub mod connection;
pub mod hook_tunnel;
pub mod pool;
pub mod registry_bindings;
pub mod ssh_transport;
pub mod terminal_agent_seam;
