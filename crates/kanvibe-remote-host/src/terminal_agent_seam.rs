//! 원격 터미널 서비스(`kanvibe_terminal::remote_terminal_service`)가 Node 번들 대신 Rust agent를 띄우게 하는 구현.
//!
//! 실행 명령은 바이너리 loader(`terminal-agent` 모드)이고, 원격에 바이너리가 없으면 loader가 `upload-required`를
//! 내므로 실행기가 그 자리에서 바이너리를 올리고 같은 ssh 명령을 다시 띄운다. 서비스가 보는 첫 줄은 늘 agent의
//! `bootstrap-ready`(또는 `unsupported`)다. bootstrap의 `source`에는 데스크탑이 아는 바이너리 hash 목록을 실어
//! 원격 agent가 자기 hash가 그 안에 있는지 확인한다. 직접 연결 인증서는 ring으로 만든 ECDSA P-256 자체 서명이다.

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::sync::Arc;
use std::time::Duration;

use kanvibe_remote_agent::bootstrap::AgentLaunchMode;
use kanvibe_terminal::remote_terminal_service::{
    CERTIFICATE_VALIDITY_MS, RemoteTerminalAgentBundle, RemoteTerminalAgentProcess,
    RemoteTerminalAgentSpawner, RemoteTerminalAgentTermination, RemoteTerminalCertificate,
    RemoteTerminalCertificateIssuer,
};
use serde_json::Value;

use crate::agent_binaries::AgentBinaryCatalog;
use crate::connection::HostProcessTermination;
use crate::pool::RemoteHostConnectionPool;

const MAX_FIRST_LINE_BYTES: u64 = 4096;

pub struct RustRemoteTerminalAgentBundle {
    binaries: Arc<AgentBinaryCatalog>,
    namespace: String,
}

impl RustRemoteTerminalAgentBundle {
    /// 바이너리는 `/tmp/kv-<uid>/<namespace>/`에 둔다. host daemon과 같은 namespace를 쓴다
    pub fn new(binaries: Arc<AgentBinaryCatalog>, namespace: &str) -> Self {
        Self {
            binaries,
            namespace: namespace.to_string(),
        }
    }
}

impl RemoteTerminalAgentBundle for RustRemoteTerminalAgentBundle {
    fn launch_command(&self) -> String {
        // 형식이 틀린 namespace면 원격이 미지원으로 답하게 해 순수 ssh 경로로 돌린다
        self.binaries
            .loader_script(&self.namespace, AgentLaunchMode::TerminalAgent)
            .unwrap_or_else(|_| r#"printf '{"type":"unsupported"}\n'"#.to_string())
    }

    fn read_source(&self) -> Result<String, String> {
        let hashes: Vec<String> = self
            .binaries
            .digests()
            .into_iter()
            .map(|digest| digest.sha256)
            .collect();
        if hashes.is_empty() {
            return Err("Remote terminal agent binary is missing; run the desktop build.".into());
        }
        Ok(hashes.join(","))
    }
}

/// 업로드가 필요하면 바이너리를 올리고 다시 띄우는 실행기. ssh 실행 자체는 감싼 실행기가 한다
pub struct UploadingRemoteTerminalAgentSpawner {
    inner: Arc<dyn RemoteTerminalAgentSpawner>,
    binaries: Arc<AgentBinaryCatalog>,
}

impl UploadingRemoteTerminalAgentSpawner {
    pub fn new(
        inner: Arc<dyn RemoteTerminalAgentSpawner>,
        binaries: Arc<AgentBinaryCatalog>,
    ) -> Self {
        Self { inner, binaries }
    }
}

impl RemoteTerminalAgentSpawner for UploadingRemoteTerminalAgentSpawner {
    fn spawn(
        &self,
        ssh_host: &str,
        ssh_arguments: &[String],
    ) -> Result<RemoteTerminalAgentProcess, String> {
        for _ in 0..2 {
            let process = self.inner.spawn(ssh_host, ssh_arguments)?;
            let RemoteTerminalAgentProcess {
                mut stdin,
                stdout,
                termination,
            } = process;
            let mut reader = BufReader::new(stdout);
            let first_line =
                read_first_line(&mut reader).inspect_err(|_| termination.terminate())?;
            let first: Value = serde_json::from_str(first_line.trim()).unwrap_or(Value::Null);
            if first["type"] != "upload-required" {
                let stdout: Box<dyn Read + Send> =
                    Box::new(Cursor::new(first_line.into_bytes()).chain(reader));
                return Ok(RemoteTerminalAgentProcess {
                    stdin,
                    stdout,
                    termination,
                });
            }
            let platform = first["platform"].as_str().unwrap_or_default();
            let uploaded = self.binaries.read_binary(platform).and_then(|binary| {
                stdin
                    .write_all(&binary)
                    .and_then(|()| stdin.flush())
                    .map_err(|error| error.to_string())?;
                drop(stdin);
                read_first_line(&mut reader)
            });
            termination.terminate();
            let uploaded: Value = serde_json::from_str(uploaded?.trim()).unwrap_or(Value::Null);
            if uploaded["type"] != "uploaded" {
                return Err("Remote terminal agent upload failed".into());
            }
        }
        Err("Remote terminal agent did not start after upload".into())
    }
}

/// 호스트 daemon 연결로 agent 명령을 띄우는 길
pub trait DaemonAgentLauncher: Send + Sync {
    /// daemon을 쓸 수 없는 호스트면 `None`이다. 연결 준비가 실패하면 오류다(TS `getRemoteHostConnection`이 던지는 경우)
    fn launch(
        &self,
        ssh_host: &str,
        command: &str,
    ) -> Result<Option<RemoteTerminalAgentProcess>, String>;
}

/// 연결 풀의 daemon 연결로 띄운다. 시간 제한은 TS처럼 직접 연결 인증서의 유효 기간이다
pub struct PooledDaemonAgentLauncher(pub Arc<RemoteHostConnectionPool>);

impl DaemonAgentLauncher for PooledDaemonAgentLauncher {
    fn launch(
        &self,
        ssh_host: &str,
        command: &str,
    ) -> Result<Option<RemoteTerminalAgentProcess>, String> {
        let Some(connection) = self
            .0
            .connection(ssh_host)
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let timeout = Duration::from_millis(CERTIFICATE_VALIDITY_MS as u64);
        let process = connection.create_process(command, timeout)?;
        Ok(Some(RemoteTerminalAgentProcess {
            stdin: Box::new(process.stdin),
            stdout: Box::new(process.stdout),
            termination: Box::new(process.termination),
        }))
    }
}

impl RemoteTerminalAgentTermination for HostProcessTermination {
    fn terminate(&self) {
        HostProcessTermination::terminate(self);
    }
}

/// daemon 연결이 있는 호스트는 그 연결로, 없으면 감싼 실행기(시스템 ssh)로 agent를 띄운다
/// (TS `connection ? connection.createProcess(...) : spawn("ssh", ...)`). 원격 명령은 `ssh_arguments`의 마지막 원소다
pub struct DaemonFirstRemoteTerminalAgentSpawner {
    daemon: Arc<dyn DaemonAgentLauncher>,
    ssh: Arc<dyn RemoteTerminalAgentSpawner>,
}

impl DaemonFirstRemoteTerminalAgentSpawner {
    pub fn new(
        daemon: Arc<dyn DaemonAgentLauncher>,
        ssh: Arc<dyn RemoteTerminalAgentSpawner>,
    ) -> Self {
        Self { daemon, ssh }
    }
}

impl RemoteTerminalAgentSpawner for DaemonFirstRemoteTerminalAgentSpawner {
    fn spawn(
        &self,
        ssh_host: &str,
        ssh_arguments: &[String],
    ) -> Result<RemoteTerminalAgentProcess, String> {
        let remote_command = ssh_arguments
            .last()
            .ok_or("Remote terminal agent command is missing")?;
        match self.daemon.launch(ssh_host, remote_command)? {
            Some(process) => Ok(process),
            None => self.ssh.spawn(ssh_host, ssh_arguments),
        }
    }
}

fn read_first_line(reader: &mut BufReader<Box<dyn Read + Send>>) -> Result<String, String> {
    let mut line = String::new();
    let count = reader
        .by_ref()
        .take(MAX_FIRST_LINE_BYTES)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    if count == 0 {
        return Err("Remote terminal helper disconnected".into());
    }
    Ok(line)
}

/// 원격 터미널 agent의 직접 연결 인증서(TS `selfsigned.generate`, commonName `KanVibe Remote Terminal`).
/// TS는 RSA 2048이지만 ring이 RSA 키를 만들지 못해 ECDSA P-256을 쓴다. 모바일은 인증서 지문으로 고정하므로 알고리즘과 무관하다
pub struct SelfSignedCertificateIssuer;

impl RemoteTerminalCertificateIssuer for SelfSignedCertificateIssuer {
    fn issue(&self, not_after_ms: i64) -> Result<RemoteTerminalCertificate, String> {
        let mut parameters = rcgen::CertificateParams::new(Vec::<String>::new())
            .map_err(|error| error.to_string())?;
        let mut name = rcgen::DistinguishedName::new();
        name.push(rcgen::DnType::CommonName, "KanVibe Remote Terminal");
        parameters.distinguished_name = name;
        parameters.not_after = rcgen::date_time_ymd(1970, 1, 1)
            + std::time::Duration::from_millis(not_after_ms.max(0) as u64);
        let key = rcgen::KeyPair::generate().map_err(|error| error.to_string())?;
        let certificate = parameters
            .self_signed(&key)
            .map_err(|error| error.to_string())?;
        Ok(RemoteTerminalCertificate {
            certificate_pem: certificate.pem(),
            private_key_pem: key.serialize_pem(),
        })
    }
}

#[cfg(test)]
mod tests;
