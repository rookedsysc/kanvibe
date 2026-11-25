//! 붙여넣은 이미지·파일을 scp로 원격 `/tmp`에 올린다(TS `lib/remoteImagePaste.ts`).
//! 로컬 임시 파일은 0600으로 쓰고, 전송이 성공하든 실패하든 임시 디렉터리를 지운다.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, LazyLock};

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use kanvibe_process::shell_environment::ShellEnvironment;
use kanvibe_process::ssh_config::{SshHostConfig, build_scp_args};
use regex::Regex;

use crate::error::ServiceError;

const REMOTE_TEMP_DIRECTORY: &str = "/tmp";
const LOCAL_TEMP_DIRECTORY_PREFIX: &str = "kanvibe-paste-";

static ATTACHMENT_EXTENSION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[a-z0-9]{1,12}$").expect("유효한 정규식"));

/// `scp` 실행. 실제 구현은 좁힌 로컬 셸 환경으로 시스템 `scp`를 띄운다
pub trait ScpTransport: Send + Sync {
    /// 종료 코드 0이면 성공. 실패 메시지는 사용자에게 그대로 보인다
    fn copy(&self, scp_arguments: &[String]) -> Result<(), String>;
}

pub struct SystemScpTransport {
    /// `create_local_shell_environment`로 만든 환경. 서버 런타임 값이 scp와 ssh에 새지 않는다
    environment: ShellEnvironment,
}

impl SystemScpTransport {
    pub fn new(environment: ShellEnvironment) -> Self {
        Self { environment }
    }
}

impl ScpTransport for SystemScpTransport {
    fn copy(&self, scp_arguments: &[String]) -> Result<(), String> {
        let output = Command::new("scp")
            .args(scp_arguments)
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            return Ok(());
        }
        let exit_code = output
            .status
            .code()
            .map_or("null".to_string(), |code| code.to_string());
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "scp 전송 실패 (exit {exit_code}): {}",
            stderr.trim()
        ))
    }
}

/// `data:<mime>;base64,<payload>`에서 바이트를 꺼낸다. Node `Buffer.from(_, "base64")`처럼 알파벳 밖 문자는 건너뛰고 첫 `=`에서 멈춘다
pub fn decode_data_url_to_bytes(data_url: &str) -> Result<Vec<u8>, ServiceError> {
    let (_, payload) = data_url
        .split_once(',')
        .ok_or_else(|| ServiceError::new("잘못된 이미지 데이터입니다."))?;
    let mut normalized: String = payload
        .chars()
        .take_while(|character| *character != '=')
        .filter_map(|character| match character {
            '-' => Some('+'),
            '_' => Some('/'),
            'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' => Some(character),
            _ => None,
        })
        .collect();
    // 4로 나눈 나머지가 1인 마지막 글자는 한 바이트도 만들지 못하므로 Node처럼 버린다
    if normalized.len() % 4 == 1 {
        normalized.pop();
    }
    STANDARD_NO_PAD
        .decode(normalized)
        .map_err(|error| ServiceError::new(error.to_string()))
}

/// 붙여넣은 이미지가 원격에 저장될 경로. SSH로 접속하는 원격 호스트는 사실상 항상 유닉스 계열이라 `/tmp`를 고정으로 쓴다
pub fn build_remote_image_paste_path(uuid: &str) -> String {
    format!("{REMOTE_TEMP_DIRECTORY}/kanvibe-paste-{uuid}.png")
}

pub struct RemoteImagePaste {
    transport: Arc<dyn ScpTransport>,
    /// 로컬 임시 디렉터리를 만들 상위 경로(TS `os.tmpdir()`)
    local_temp_root: PathBuf,
    new_uuid: Box<dyn Fn() -> String + Send + Sync>,
}

impl RemoteImagePaste {
    pub fn new(transport: Arc<dyn ScpTransport>, local_temp_root: PathBuf) -> Self {
        Self::with_uuid_source(transport, local_temp_root, || {
            uuid::Uuid::new_v4().to_string()
        })
    }

    pub fn with_uuid_source(
        transport: Arc<dyn ScpTransport>,
        local_temp_root: PathBuf,
        new_uuid: impl Fn() -> String + Send + Sync + 'static,
    ) -> Self {
        Self {
            transport,
            local_temp_root,
            new_uuid: Box::new(new_uuid),
        }
    }

    /// 클립보드 이미지를 올리고 원격 경로를 돌려준다
    pub fn transfer_image_to_remote_host(
        &self,
        ssh_config: &SshHostConfig,
        image_bytes: &[u8],
    ) -> Result<String, ServiceError> {
        self.transfer_file_to_remote_host(ssh_config, image_bytes, "png")
    }

    pub fn transfer_file_to_remote_host(
        &self,
        ssh_config: &SshHostConfig,
        bytes: &[u8],
        extension: &str,
    ) -> Result<String, ServiceError> {
        if !ATTACHMENT_EXTENSION_PATTERN.is_match(extension) {
            return Err(ServiceError::new("첨부 파일 확장자가 올바르지 않습니다."));
        }
        let uuid = (self.new_uuid)();
        let remote_path = if extension == "png" {
            build_remote_image_paste_path(&uuid)
        } else {
            format!("{REMOTE_TEMP_DIRECTORY}/kanvibe-paste-{uuid}.{extension}")
        };
        let local_directory = self.create_local_directory()?;
        let local_path = local_directory.join(format!("{uuid}.{extension}"));
        let result = self.upload(ssh_config, &local_path, bytes, &remote_path);
        let _ = fs::remove_dir_all(&local_directory);
        result.map(|()| remote_path)
    }

    fn create_local_directory(&self) -> Result<PathBuf, ServiceError> {
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let directory = self
            .local_temp_root
            .join(format!("{LOCAL_TEMP_DIRECTORY_PREFIX}{}", &suffix[..6]));
        fs::create_dir(&directory).map_err(|error| ServiceError::new(error.to_string()))?;
        Ok(directory)
    }

    fn upload(
        &self,
        ssh_config: &SshHostConfig,
        local_path: &Path,
        bytes: &[u8],
        remote_path: &str,
    ) -> Result<(), ServiceError> {
        write_private_file(local_path, bytes)
            .map_err(|error| ServiceError::new(error.to_string()))?;
        let local_path_text = local_path.to_string_lossy();
        let arguments = build_scp_args(ssh_config, &local_path_text, remote_path);
        self.transport.copy(&arguments).map_err(ServiceError::new)
    }
}

fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

#[cfg(test)]
mod tests;
