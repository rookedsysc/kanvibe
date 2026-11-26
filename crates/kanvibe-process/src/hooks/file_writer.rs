//! hook 설치 산출물(스크립트·설정 파일)을 로컬 또는 원격 repo에 기록한다(TS `hookFileWriter.ts`).
//! 로컬과 원격 설치가 같은 파일 목록을 공유하도록 쓰기 경로만 나눈다.

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use super::HookProviderFile;
use super::client_runtime::{HookInstallContext, HookInstallError};
use super::install_bundle::{
    HOOK_INSTALL_SUCCESS_MARKER, build_hook_file_write_command,
    build_hook_install_bootstrap_command,
};
use crate::exec::ExecGitOptions;
use crate::host_file_access::HostFileError;

/// 원격이 Hook 서버로 되돌아오지 못하는 구성에서 SSH 주입 폴백이 늦어지지 않도록 두는 상한.
/// 내려받기 자체에도 상한이 있으므로 이 값은 그보다 뒤에서 받쳐 주는 역할만 한다.
pub const REMOTE_HOOK_INSTALL_TIMEOUT: Duration = Duration::from_secs(30);

pub fn write_hook_provider_files(
    context: &HookInstallContext,
    files: &[HookProviderFile],
    ssh_host: Option<&str>,
) -> Result<(), HookInstallError> {
    if files.is_empty() {
        return Ok(());
    }
    match ssh_host.filter(|host| !host.is_empty()) {
        None => write_local_hook_provider_files(files),
        Some(host) => write_remote_hook_provider_files(context, files, host),
    }
}

fn write_local_hook_provider_files(files: &[HookProviderFile]) -> Result<(), HookInstallError> {
    for file in files {
        let file_path = file.file_path.as_str();
        let io_error = |error| HookInstallError::Files(HostFileError::io(file_path, error));
        if let Some(parent) = std::path::Path::new(file_path).parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        std::fs::write(file_path, &file.content).map_err(io_error)?;
        if let Some(mode) = file.mode.filter(|mode| *mode != 0) {
            std::fs::set_permissions(file_path, std::fs::Permissions::from_mode(mode))
                .map_err(io_error)?;
        }
    }
    Ok(())
}

/// 원격 설치는 Hook 서버에서 설치 스크립트를 내려받아 실행하는 경로를 먼저 쓴다.
/// 파일 본문을 SSH 명령줄에 싣지 않아 명령이 짧아지고, 터널 소켓으로 내려받을 수 없으면 검증된 SSH 연결에서 파일을 설치한다.
fn write_remote_hook_provider_files(
    context: &HookInstallContext,
    files: &[HookProviderFile],
    ssh_host: &str,
) -> Result<(), HookInstallError> {
    if let Err(error) = install_remote_hook_provider_files_over_http(context, files, ssh_host) {
        eprintln!(
            "[hooks] 소켓 설치 경로 실패, SSH 주입으로 대체합니다 {{ sshHost: {ssh_host:?}, error: {:?} }}",
            error.to_string()
        );
        return inject_remote_hook_provider_files_over_ssh(context, files, ssh_host);
    }
    Ok(())
}

fn strict_options(timeout: Option<Duration>) -> ExecGitOptions {
    ExecGitOptions {
        timeout,
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    }
}

fn install_remote_hook_provider_files_over_http(
    context: &HookInstallContext,
    files: &[HookProviderFile],
    ssh_host: &str,
) -> Result<(), HookInstallError> {
    let runtime = context.runtime.as_ref();
    let hook_server_url = runtime.hook_server_url(Some(ssh_host))?;
    let install_scripts = runtime.install_scripts();
    let install_ticket = install_scripts.issue(files);
    let install_result = (|| {
        let client_id = runtime.hook_client_id()?;
        let namespace = runtime.remote_host_namespace(ssh_host)?;
        let command = build_hook_install_bootstrap_command(
            &hook_server_url,
            &install_ticket,
            &client_id,
            namespace.as_deref(),
        )
        .map_err(|error| HookInstallError::Message(error.to_string()))?;
        let output = context.files.runner().exec_git(
            &command,
            Some(ssh_host),
            &strict_options(Some(REMOTE_HOOK_INSTALL_TIMEOUT)),
        )?;
        if !output.contains(HOOK_INSTALL_SUCCESS_MARKER) {
            return Err(HookInstallError::Message(
                "원격 hook 설치 스크립트가 완료 표시를 남기지 않았습니다.".into(),
            ));
        }
        Ok(())
    })();
    install_scripts.revoke(&install_ticket.token);
    install_result
}

fn inject_remote_hook_provider_files_over_ssh(
    context: &HookInstallContext,
    files: &[HookProviderFile],
    ssh_host: &str,
) -> Result<(), HookInstallError> {
    let file_commands: Vec<String> = files.iter().map(build_hook_file_write_command).collect();
    context.files.runner().exec_git(
        &file_commands.join(" && "),
        Some(ssh_host),
        &strict_options(None),
    )?;
    Ok(())
}
