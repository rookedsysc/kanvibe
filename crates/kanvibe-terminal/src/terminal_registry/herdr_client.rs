//! 공유 Herdr Space에 붙는 client 중계 준비(TS `prepareHerdrTaskClient`).
//! 원격은 daemon이 소켓을 이어 주면 그것을 쓰고, 아니면 ssh `-L`·`-R` 포워딩 인자와 로컬 임시 디렉터리를 만든다.

use std::ffi::{CString, OsString};
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    HerdrClientRelay, HerdrTabBarPosition, RemoteHostSocketForward, TerminalRegistryDependencies,
};
use crate::environment::TerminalHostEnvironment;

pub(super) struct HerdrTaskClient {
    pub relay: Arc<dyn HerdrClientRelay>,
    pub workspace_id: String,
    pub forwarding_args: Vec<String>,
    pub client_socket_path: String,
    pub tab_bar_position: Option<HerdrTabBarPosition>,
    pub ssh_host: Option<String>,
    /// daemon이 소켓을 이어 줘 로컬에서 Herdr client를 띄운다
    pub has_host_transport: bool,
    host_forward: Option<Arc<dyn RemoteHostSocketForward>>,
    forwarding_directory: Option<PathBuf>,
}

impl HerdrTaskClient {
    /// 중계, daemon 소켓 포워딩, 포워딩 임시 디렉터리를 차례로 정리한다
    pub fn close(&self) -> Result<(), String> {
        self.relay.close()?;
        close_forwarding(
            self.host_forward.as_ref(),
            self.forwarding_directory.as_deref(),
        )
    }
}

fn close_forwarding(
    host_forward: Option<&Arc<dyn RemoteHostSocketForward>>,
    forwarding_directory: Option<&Path>,
) -> Result<(), String> {
    if let Some(host_forward) = host_forward {
        host_forward.close()?;
    }
    if let Some(directory) = forwarding_directory {
        remove_directory_if_present(directory)?;
    }
    Ok(())
}

/// TS `rm(dir, { recursive: true, force: true })`
fn remove_directory_if_present(directory: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(directory) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.to_string()),
        _ => Ok(()),
    }
}

/// 원격 서버 소켓에 닿는 길. daemon 포워딩, ssh 포워딩용 임시 디렉터리, 로컬 서버 소켓 중 하나다
struct UpstreamRoute {
    host_forward: Option<Arc<dyn RemoteHostSocketForward>>,
    forwarding_directory: Option<PathBuf>,
    upstream_socket_path: String,
}

impl UpstreamRoute {
    /// 중계를 만들지 못했을 때 열어 둔 포워딩을 정리한다. 원래 실패를 돌려주려고 정리 실패는 버린다
    fn close(&self) {
        let _ = close_forwarding(
            self.host_forward.as_ref(),
            self.forwarding_directory.as_deref(),
        );
    }
}

pub(super) fn prepare_herdr_task_client(
    dependencies: &TerminalRegistryDependencies,
    session_name: &str,
    workspace_id: Option<String>,
    ssh_host: Option<&str>,
) -> Result<HerdrTaskClient, String> {
    let workspace_id =
        workspace_id.ok_or_else(|| "공유 Herdr Space를 찾을 수 없습니다.".to_string())?;
    let tab_bar_position = dependencies.herdr.client_tab_bar_position(ssh_host);
    let server_socket_path = dependencies
        .herdr
        .client_socket_path(session_name, ssh_host)
        .map_err(|error| error.to_string())?;
    let route = open_upstream_route(dependencies, ssh_host, &server_socket_path)?;
    let relay = dependencies
        .herdr_relays
        .create(&route.upstream_socket_path, &workspace_id)
        .inspect_err(|_| route.close())?;
    let is_ssh_forwarded = ssh_host.is_some() && route.host_forward.is_none();
    let remote_proxy_path = format!("/tmp/kvhc-{}.sock", uuid::Uuid::new_v4());
    let relay_socket_path = relay.socket_path();
    let forwarding_args = match is_ssh_forwarded {
        true => build_ssh_forwarding_args(
            &route.upstream_socket_path,
            &server_socket_path,
            &remote_proxy_path,
            &relay_socket_path,
        ),
        false => Vec::new(),
    };
    Ok(HerdrTaskClient {
        relay,
        workspace_id,
        forwarding_args,
        client_socket_path: if is_ssh_forwarded {
            remote_proxy_path
        } else {
            relay_socket_path
        },
        tab_bar_position,
        ssh_host: ssh_host.map(str::to_string),
        has_host_transport: route.host_forward.is_some(),
        host_forward: route.host_forward,
        forwarding_directory: route.forwarding_directory,
    })
}

fn open_upstream_route(
    dependencies: &TerminalRegistryDependencies,
    ssh_host: Option<&str>,
    server_socket_path: &str,
) -> Result<UpstreamRoute, String> {
    let Some(ssh_host) = ssh_host else {
        return Ok(UpstreamRoute {
            host_forward: None,
            forwarding_directory: None,
            upstream_socket_path: server_socket_path.to_string(),
        });
    };
    if let Some(connection) = dependencies.remote_hosts.connection(ssh_host)? {
        let host_forward = connection.forward_socket(server_socket_path)?;
        return Ok(UpstreamRoute {
            upstream_socket_path: host_forward.socket_path(),
            host_forward: Some(host_forward),
            forwarding_directory: None,
        });
    }
    let directory =
        make_forwarding_directory(&dependencies.environment).map_err(|error| error.to_string())?;
    Ok(UpstreamRoute {
        upstream_socket_path: directory.join("upstream").to_string_lossy().into_owned(),
        host_forward: None,
        forwarding_directory: Some(directory),
    })
}

fn build_ssh_forwarding_args(
    upstream_socket_path: &str,
    server_socket_path: &str,
    remote_proxy_path: &str,
    relay_socket_path: &str,
) -> Vec<String> {
    [
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "StreamLocalBindUnlink=yes",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-L",
        &format!("{upstream_socket_path}:{server_socket_path}"),
        "-R",
        &format!("{remote_proxy_path}:{relay_socket_path}"),
    ]
    .map(str::to_string)
    .to_vec()
}

/// Node `os.tmpdir()`처럼 `TMPDIR`, `TMP`, `TEMP`, `/tmp` 순으로 고르고 끝의 `/` 하나를 뗀다.
/// 프로세스 환경이 아니라 앱이 넘긴 호스트 환경에서 읽는다
fn temp_root(environment: &TerminalHostEnvironment) -> PathBuf {
    let configured = ["TMPDIR", "TMP", "TEMP"]
        .into_iter()
        .find_map(|key| environment.variable(key))
        .unwrap_or("/tmp");
    let trimmed = match configured.strip_suffix('/') {
        Some(stripped) if configured.len() > 1 => stripped,
        _ => configured,
    };
    PathBuf::from(trimmed)
}

/// Node `mkdtemp(join(tmpdir(), "kvhf-"))`. 이름 끝 여섯 글자를 무작위로 채우고 0700으로 만든다
fn make_forwarding_directory(environment: &TerminalHostEnvironment) -> io::Result<PathBuf> {
    let template = temp_root(environment).join("kvhf-XXXXXX");
    let mut template_bytes = CString::new(template.as_os_str().as_bytes())?.into_bytes_with_nul();
    // template_bytes는 NUL로 끝나는 쓰기 가능한 버퍼이고 mkdtemp는 그 안의 X만 바꾼다
    let created = unsafe { libc::mkdtemp(template_bytes.as_mut_ptr().cast()) };
    if created.is_null() {
        return Err(io::Error::last_os_error());
    }
    template_bytes.pop();
    Ok(PathBuf::from(OsString::from_vec(template_bytes)))
}
