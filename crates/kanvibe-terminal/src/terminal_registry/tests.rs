//! 터미널 레지스트리 테스트. TS `terminal.test.ts`, `terminalSessionLifecycle.test.ts`,
//! `terminalSessionPreparation.test.ts`, `terminalRemoteHost.test.ts`를 옮긴다.

mod environment_isolation;
mod fakes;
mod herdr_remote;
mod herdr_shared;
mod lifecycle;
mod local_tmux;
mod local_zellij;
mod preparation;
mod remote_host;
mod remote_ssh;
mod tabs;
