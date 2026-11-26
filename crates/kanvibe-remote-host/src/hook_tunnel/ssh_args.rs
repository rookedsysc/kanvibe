//! 터널 ssh 인자, 엔드포인트 키, 원격이 알려 준 소켓 경로 확인.

use kanvibe_process::hooks::socket_path::build_hook_socket_file_name;
use kanvibe_process::ssh_config::{
    SshArgsOptions, SshConnectionHealthOptions, SshHostConfig, build_ssh_args,
};

const TUNNEL_CONNECT_TIMEOUT_SECONDS: u32 = 10;
const TUNNEL_SERVER_ALIVE_INTERVAL_SECONDS: u32 = 15;
const TUNNEL_SERVER_ALIVE_COUNT_MAX: u32 = 2;

/// 마지막 원소(접속 대상) 앞에 터널 옵션을 넣고 원격 명령으로 `cat >/dev/null`을 붙인다.
/// 전달에 실패하면 ssh가 끝나 실패가 드러나고, 앱이 죽어 stdin이 닫히면 `cat`이 끝나 ssh도 함께 끝난다
pub fn build_hook_tunnel_ssh_args(
    base_args: &[String],
    remote_socket_path: &str,
    local_socket_path: &str,
) -> Vec<String> {
    let (destination, leading) = base_args
        .split_last()
        .map_or((String::new(), &[][..]), |(last, leading)| {
            (last.clone(), leading)
        });
    let options = [
        "ExitOnForwardFailure=yes",
        "StreamLocalBindUnlink=yes",
        // 사용자 설정의 ControlMaster를 타면 터널의 수명이 이 프로세스와 분리된다
        "ControlMaster=no",
        "ControlPath=none",
        "StrictHostKeyChecking=yes",
    ];
    let mut args: Vec<String> = leading.to_vec();
    for option in options {
        args.push("-o".into());
        args.push(option.into());
    }
    args.push("-R".into());
    args.push(format!("{remote_socket_path}:{local_socket_path}"));
    args.push(destination);
    args.push("cat >/dev/null".into());
    args
}

/// 터널 ssh의 기본 인자. 터널은 오래 살아 있으므로 연결 상태 확인 간격을 따로 둔다
pub fn build_hook_tunnel_base_args(config: &SshHostConfig) -> Vec<String> {
    let options = SshArgsOptions {
        is_tty_disabled: true,
        connection_health: Some(SshConnectionHealthOptions {
            connect_timeout_seconds: TUNNEL_CONNECT_TIMEOUT_SECONDS,
            server_alive_interval_seconds: TUNNEL_SERVER_ALIVE_INTERVAL_SECONDS,
            server_alive_count_max: TUNNEL_SERVER_ALIVE_COUNT_MAX,
        }),
        ..SshArgsOptions::default()
    };
    build_ssh_args(config, &options)
}

/// daemon을 쓰지 않을 때의 엔드포인트 키. 같은 계정·주소·포트로 붙는 별칭은 원격의 같은 /tmp/kv-<uid>를 쓴다
pub fn build_plain_endpoint_key(config: &SshHostConfig) -> String {
    format!(
        "{}@{}:{}",
        config.username,
        config.hostname.to_lowercase(),
        config.port
    )
}

/// 원격 준비 명령이 출력한 경로가 `/tmp/kv-<uid>/<clientId>.sock`인지 확인한다
pub fn validate_remote_socket_path(
    remote_socket_path: &str,
    client_id: &str,
) -> Result<(), String> {
    let mismatch = || "원격 hook 소켓 경로가 설치한 client의 경로와 일치하지 않습니다.".to_string();
    let (directory, file_name) = remote_socket_path.rsplit_once('/').ok_or_else(mismatch)?;
    let uid = directory.strip_prefix("/tmp/kv-").ok_or_else(mismatch)?;
    let is_uid = !uid.is_empty() && uid.bytes().all(|byte| byte.is_ascii_digit());
    let expected_file_name = build_hook_socket_file_name(client_id).map_err(|_| mismatch())?;
    if !is_uid || file_name != expected_file_name {
        return Err(mismatch());
    }
    Ok(())
}
