//! 원격에 올릴 `kanvibe-remote-agent` 바이너리 목록과 sha256(TS `build/remote/host-server.cjs` 번들 자리).
//!
//! 패키징은 플랫폼마다 `kanvibe-remote-agent-<uname -s>-<uname -m>` 파일을 한 디렉터리에 둔다
//! (예: `kanvibe-remote-agent-Linux-x86_64`, `kanvibe-remote-agent-Darwin-arm64`). 없는 플랫폼의 원격은 loader가
//! `unsupported`로 답하고 데스크탑은 순수 ssh 경로로 간다. 올리기 직전에 파일을 다시 읽어 hash를 확인한다.

use std::path::{Path, PathBuf};

use kanvibe_remote_agent::bootstrap::{
    AGENT_PLATFORMS, AgentBinaryDigest, AgentLaunchMode, build_agent_loader_script,
};
use sha2::{Digest, Sha256};

pub const AGENT_BINARY_FILE_PREFIX: &str = "kanvibe-remote-agent-";

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentBinary {
    platform: String,
    sha256: String,
    path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentBinaryCatalog {
    binaries: Vec<AgentBinary>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

impl AgentBinaryCatalog {
    /// 디렉터리에서 플랫폼별 바이너리를 찾는다. 하나도 없으면 빈 목록이다
    pub fn from_directory(directory: &Path) -> Result<Self, String> {
        let entries = AGENT_PLATFORMS
            .iter()
            .map(|platform| {
                (
                    platform.to_string(),
                    directory.join(format!("{AGENT_BINARY_FILE_PREFIX}{platform}")),
                )
            })
            .filter(|(_, path)| path.is_file())
            .collect();
        Self::from_files(entries)
    }

    /// (플랫폼, 경로) 목록으로 만든다
    pub fn from_files(entries: Vec<(String, PathBuf)>) -> Result<Self, String> {
        let mut binaries = Vec::new();
        for (platform, path) in entries {
            if !AGENT_PLATFORMS.contains(&platform.as_str()) {
                return Err(format!("Unknown remote agent platform: {platform}"));
            }
            let bytes =
                std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            binaries.push(AgentBinary {
                platform,
                sha256: sha256_hex(&bytes),
                path,
            });
        }
        Ok(Self { binaries })
    }

    pub fn digests(&self) -> Vec<AgentBinaryDigest> {
        self.binaries
            .iter()
            .map(|binary| AgentBinaryDigest {
                platform: binary.platform.clone(),
                sha256: binary.sha256.clone(),
            })
            .collect()
    }

    pub fn contains_hash(&self, sha256: &str) -> bool {
        self.binaries.iter().any(|binary| binary.sha256 == sha256)
    }

    /// 플랫폼 바이너리를 읽는다. 목록을 만든 뒤 파일이 바뀌었으면 올리지 않는다
    pub fn read_binary(&self, platform: &str) -> Result<Vec<u8>, String> {
        let binary = self
            .binaries
            .iter()
            .find(|binary| binary.platform == platform)
            .ok_or_else(|| format!("Remote agent binary missing for {platform}"))?;
        let bytes = std::fs::read(&binary.path).map_err(|error| error.to_string())?;
        if sha256_hex(&bytes) != binary.sha256 {
            return Err("Remote agent binary changed after it was verified".into());
        }
        Ok(bytes)
    }

    /// `sh -lc`로 실행할 원격 loader
    pub fn loader_script(&self, namespace: &str, mode: AgentLaunchMode) -> Result<String, String> {
        build_agent_loader_script(namespace, mode, &self.digests())
            .ok_or_else(|| "Invalid host transport identity".to_string())
    }
}
