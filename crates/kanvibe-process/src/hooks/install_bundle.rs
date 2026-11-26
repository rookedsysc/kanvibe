//! 원격 hook 설치용 일회성 스크립트(TS `hookInstallBundle.ts`).
//! 원격은 hook 소켓으로 `/api/install/hooks.sh`를 내려받아 SSH로 받은 체크섬과 대조한 뒤에만 실행한다.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use sha2::{Digest, Sha256};

use super::HookProviderFile;
use super::socket_path::{HookSocketPathError, build_shell_owned_hook_socket_assignment};
use crate::host_file_access::quote_shell_argument;
use kanvibe_domain::project::posix_dirname;

/// 설치 스크립트를 내려주는 Hook 서버 경로
pub const HOOK_INSTALL_SCRIPT_PATH: &str = "/api/install/hooks.sh";
/// 설치 토큰을 싣는 요청 헤더. URL에 두면 프록시·서버 접근 로그에 남는다
pub const HOOK_INSTALL_TOKEN_HEADER: &str = "x-kanvibe-install-token";
/// 설치 스크립트가 모든 파일을 기록한 뒤 마지막 줄에 남기는 표시. 원격 설치 성공 판정에 쓴다
pub const HOOK_INSTALL_SUCCESS_MARKER: &str = "__KANVIBE_HOOK_INSTALL_OK__";

/// 설치 실패 시 재시도가 같은 토큰을 다시 쓸 수 있을 만큼 길고, LAN에 오래 남지 않을 만큼 짧게 잡는다
const HOOK_INSTALL_TOKEN_TTL: Duration = Duration::from_secs(5 * 60);
const HOOK_INSTALL_TOKEN_BYTES: usize = 24;
/// 원격이 Hook 서버로 되돌아오지 못하는 구성에서는 무응답이라, 상한이 없으면 폴백이 그만큼 늦어진다
const HOOK_INSTALL_DOWNLOAD_CONNECT_TIMEOUT_SECONDS: u32 = 3;
const HOOK_INSTALL_DOWNLOAD_MAX_TIMEOUT_SECONDS: u32 = 10;

/// 원격이 설치 스크립트를 내려받아 검증하는 데 필요한 값.
/// 토큰은 조회 자격이고, 체크섬은 내려받은 본문이 발급 시점 그대로인지 판정하는 근거다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInstallTicket {
    pub token: String,
    pub script_sha256: String,
}

struct PendingHookInstall {
    script: String,
    expires_at: Instant,
}

/// 발급한 설치 스크립트 보관소. 평문 Hook 서버가 LAN에 열려 있을 수 있으므로 토큰 없이는 설치 산출물을 내주지 않는다
#[derive(Default)]
pub struct HookInstallScripts {
    pending_installs: Mutex<HashMap<String, PendingHookInstall>>,
}

fn create_install_token() -> String {
    let mut token_bytes = [0_u8; HOOK_INSTALL_TOKEN_BYTES];
    getrandom::fill(&mut token_bytes).expect("운영체제 난수를 얻지 못했습니다");
    hex::encode(token_bytes)
}

impl HookInstallScripts {
    fn purge_expired(pending_installs: &mut HashMap<String, PendingHookInstall>) {
        let now = Instant::now();
        pending_installs.retain(|_, pending_install| pending_install.expires_at > now);
    }

    /// 원격이 내려받아 실행할 설치 스크립트를 등록하고 조회·검증에 필요한 값을 돌려준다
    pub fn issue(&self, files: &[HookProviderFile]) -> HookInstallTicket {
        let script = build_hook_install_script(files);
        let script_sha256 = hex::encode(Sha256::digest(build_downloaded_script_form(&script)));
        let token = create_install_token();
        let mut pending_installs = self.pending_installs.lock().unwrap();
        Self::purge_expired(&mut pending_installs);
        pending_installs.insert(
            token.clone(),
            PendingHookInstall {
                script,
                expires_at: Instant::now() + HOOK_INSTALL_TOKEN_TTL,
            },
        );
        HookInstallTicket {
            token,
            script_sha256,
        }
    }

    /// 토큰에 해당하는 설치 스크립트. 없거나 만료됐으면 `None`
    pub fn read(&self, token: Option<&str>) -> Option<String> {
        let token = token.filter(|token| !token.is_empty())?;
        let mut pending_installs = self.pending_installs.lock().unwrap();
        Self::purge_expired(&mut pending_installs);
        pending_installs
            .get(token)
            .map(|pending_install| pending_install.script.clone())
    }

    /// 설치가 끝난 토큰을 즉시 폐기한다. 토큰은 SSH 명령줄에 실려 원격 `ps`에 노출되므로 유효 구간을 설치 명령 수명만큼으로 좁힌다
    pub fn revoke(&self, token: &str) {
        self.pending_installs.lock().unwrap().remove(token);
    }

    /// 앱 종료 경로에서 대기 중인 설치 토큰을 모두 지운다
    pub fn clear(&self) {
        self.pending_installs.lock().unwrap().clear();
    }
}

/// 파일 하나를 base64로 풀어 기록하고 필요하면 권한을 맞추는 명령. 설치 스크립트와 SSH 주입 폴백이 같이 쓴다
pub(crate) fn build_hook_file_write_command(file: &HookProviderFile) -> String {
    let encoded_content = BASE64.encode(file.content.as_bytes());
    let quoted_file_path = quote_shell_argument(&file.file_path);
    let mut commands = vec![
        format!(
            "mkdir -p {}",
            quote_shell_argument(posix_dirname(&file.file_path))
        ),
        format!(
            "printf '%s' {} | (base64 -d 2>/dev/null || base64 -D) > {quoted_file_path}",
            quote_shell_argument(&encoded_content)
        ),
    ];
    if let Some(mode) = file.mode.filter(|mode| *mode != 0) {
        commands.push(format!("chmod {mode:o} {quoted_file_path}"));
    }
    commands.join(" && ")
}

/// hook 산출물을 원격에서 그대로 실행할 수 있는 POSIX sh 스크립트로 만든다.
/// 파일 본문은 base64로 실어 개행이나 따옴표가 셸 해석에 걸리지 않게 한다.
pub fn build_hook_install_script(files: &[HookProviderFile]) -> String {
    let mut lines = vec!["set -e".to_string()];
    lines.extend(files.iter().map(build_hook_file_write_command));
    lines.push(format!(
        "printf '%s\\n' {}",
        quote_shell_argument(HOOK_INSTALL_SUCCESS_MARKER)
    ));
    lines.push(String::new());
    lines.join("\n")
}

/// `$(...)`가 끝의 개행을 모두 떼고 원격이 `printf '%s\n'`으로 한 줄만 되붙이므로, 체크섬 대상도 그 형태와 같아야 한다
fn build_downloaded_script_form(script: &str) -> String {
    format!("{}\n", script.trim_end_matches('\n'))
}

/// 원격이 설치 스크립트를 내려받아 실행하는 한 줄짜리 명령.
/// 스크립트 본문은 HTTP로 오가므로 SSH에는 짧은 명령만 실리고, 평문 HTTP 구간의 바꿔치기를 막으려고
/// 암호화된 SSH 채널로 전달한 체크섬과 대조한 뒤에만 실행한다. 중간에 끊긴 스크립트를 실행하지 않도록 전부 받은 뒤에 sh로 넘긴다.
pub fn build_hook_install_bootstrap_command(
    hook_server_url: &str,
    ticket: &HookInstallTicket,
    client_id: &str,
    namespace: Option<&str>,
) -> Result<String, HookSocketPathError> {
    let script_url = quote_shell_argument(&format!(
        "{}{HOOK_INSTALL_SCRIPT_PATH}",
        hook_server_url.trim_end_matches('/')
    ));
    let token_header =
        quote_shell_argument(&format!("{HOOK_INSTALL_TOKEN_HEADER}: {}", ticket.token));
    let curl_options = format!(
        "--connect-timeout {HOOK_INSTALL_DOWNLOAD_CONNECT_TIMEOUT_SECONDS} --max-time {HOOK_INSTALL_DOWNLOAD_MAX_TIMEOUT_SECONDS} -fsSL -H {token_header}"
    );
    let client_header = match namespace {
        Some(_) => format!(
            " -H {}",
            quote_shell_argument(&format!("x-kanvibe-client-id: {client_id}"))
        ),
        None => String::new(),
    };
    let download_command = format!(
        "curl --unix-socket \"$__kanvibe_socket\" {curl_options}{client_header} \"$__kanvibe_url\" 2>/dev/null"
    );
    let restored_script = "printf '%s\\n' \"$__kanvibe_install_script\"";
    Ok([
        build_shell_owned_hook_socket_assignment("__kanvibe_socket", client_id, namespace)?,
        format!("__kanvibe_url={script_url}"),
        "[ -n \"$__kanvibe_socket\" ]".to_string(),
        format!("__kanvibe_install_script=$({download_command})"),
        format!(
            "__kanvibe_install_checksum=$({restored_script} | {{ sha256sum 2>/dev/null || shasum -a 256; }} | cut -d' ' -f1)"
        ),
        format!(
            "[ \"$__kanvibe_install_checksum\" = {} ]",
            quote_shell_argument(&ticket.script_sha256)
        ),
        format!("{restored_script} | sh"),
    ]
    .join(" && "))
}
