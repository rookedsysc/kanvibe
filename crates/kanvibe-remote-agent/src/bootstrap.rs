//! 원격에서 바이너리를 확인하고 실행하는 POSIX sh loader(TS `HOST_BOOTSTRAP_LOADER`를 대체).
//!
//! 원격에 Node가 없어도 되도록 loader는 `sh`, `uname`, `id`, `ls`, `cut`, `cat`, `ln`과 `sha256sum` 또는
//! `shasum`만 쓴다. 데스크탑이 가진 플랫폼별 바이너리 sha256 표를 loader에 넣고, loader는
//! `/tmp/kv-<uid>/<namespace>/agent-<sha256>`(본인 소유 0700 파일, 0700 디렉터리)이 그 hash와 맞을 때만 실행한다.
//!
//! loader가 stdout에 쓰는 줄:
//! - `{"type":"unsupported"}`: 이 플랫폼의 바이너리가 없거나 sha256 도구가 없다. 데스크탑은 순수 ssh 경로로 간다
//! - `{"type":"upload-required","platform":"<uname -s>-<uname -m>"}`: 데스크탑이 그 플랫폼 바이너리를 stdin으로
//!   끝까지 보내고 닫는다. loader는 임시 파일에 받아 hash를 확인한 뒤 `ln`으로 한 번에 게시하고 `{"type":"uploaded"}`를
//!   쓰고 끝난다. 데스크탑은 같은 명령으로 다시 연결해 이번에는 바로 실행되게 한다
//! - 그 밖에는 바이너리가 실행되어 자기 첫 줄(`{"type":"bridge-ready","buildHash":<sha256>}` 또는 `bootstrap-ready`)을 쓴다
//!
//! 소스, 인증서 키, grant 권한은 argv에 넣지 않는다. 바이너리 hash와 namespace만 argv로 넘긴다.

use crate::protocol::is_valid_host_namespace;

/// loader가 아는 플랫폼(`uname -s`-`uname -m`)
pub const AGENT_PLATFORMS: [&str; 4] = [
    "Linux-x86_64",
    "Linux-aarch64",
    "Darwin-x86_64",
    "Darwin-arm64",
];

/// 바이너리를 어떤 역할로 실행할지
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLaunchMode {
    /// SSH stdin/stdout을 daemon rpc 소켓에 잇는다
    HostBridge,
    /// 모바일 원격 터미널 agent
    TerminalAgent,
}

impl AgentLaunchMode {
    pub fn argument(self) -> &'static str {
        match self {
            Self::HostBridge => "host-bridge",
            Self::TerminalAgent => "terminal-agent",
        }
    }
}

/// 플랫폼 하나의 바이너리 hash
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentBinaryDigest {
    pub platform: String,
    pub sha256: String,
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// `sh -lc`로 실행할 loader. namespace나 hash가 형식에 맞지 않으면 `None`이다
pub fn build_agent_loader_script(
    namespace: &str,
    mode: AgentLaunchMode,
    digests: &[AgentBinaryDigest],
) -> Option<String> {
    let is_valid = is_valid_host_namespace(namespace)
        && digests.iter().all(|digest| {
            AGENT_PLATFORMS.contains(&digest.platform.as_str()) && is_sha256(&digest.sha256)
        });
    if !is_valid {
        return None;
    }
    let platform_cases: String = digests
        .iter()
        .map(|digest| format!("  {}) kv_hash={} ;;\n", digest.platform, digest.sha256))
        .collect();
    let script = LOADER_TEMPLATE
        .replace("__PLATFORM_CASES__", &platform_cases)
        .replace("__NAMESPACE__", namespace)
        .replace("__MODE__", mode.argument());
    Some(script)
}

const LOADER_TEMPLATE: &str = r#"umask 077
kv_fail() { printf '%s\n' "$1" >&2; exit 1; }
kv_platform="$(uname -s)-$(uname -m)"
case "$kv_platform" in
__PLATFORM_CASES__  *) printf '{"type":"unsupported"}\n'; exit 0 ;;
esac
if command -v sha256sum >/dev/null 2>&1; then kv_sha() { sha256sum < "$1" | cut -c1-64; }
elif command -v shasum >/dev/null 2>&1; then kv_sha() { shasum -a 256 < "$1" | cut -c1-64; }
else printf '{"type":"unsupported"}\n'; exit 0; fi
kv_uid=$(id -u) || exit 1
kv_parent="/tmp/kv-$kv_uid"
kv_dir="$kv_parent/__NAMESPACE__"
kv_private() { [ -d "$1" ] && [ ! -L "$1" ] && [ -O "$1" ] && [ "$(ls -ld "$1" | cut -c1-10)" = "drwx------" ]; }
for kv_folder in "$kv_parent" "$kv_dir"; do
  mkdir -m 700 "$kv_folder" 2>/dev/null
  kv_private "$kv_folder" || kv_fail 'Untrusted host private directory'
done
kv_bin="$kv_dir/agent-$kv_hash"
kv_trusted() { [ -f "$1" ] && [ ! -L "$1" ] && [ -O "$1" ] && [ "$(ls -ld "$1" | cut -c1-10)" = "-rwx------" ] && [ "$(kv_sha "$1")" = "$kv_hash" ]; }
if kv_trusted "$kv_bin"; then exec "$kv_bin" __MODE__ __NAMESPACE__ "$kv_hash"; fi
printf '{"type":"upload-required","platform":"%s"}\n' "$kv_platform"
kv_pending="$kv_bin.$$.tmp"
(set -C; : > "$kv_pending") 2>/dev/null || kv_fail 'Host bundle pending file exists'
if ! cat > "$kv_pending"; then rm -f "$kv_pending"; kv_fail 'Host bundle upload failed'; fi
chmod 700 "$kv_pending"
if [ "$(kv_sha "$kv_pending")" != "$kv_hash" ]; then rm -f "$kv_pending"; kv_fail 'Host bundle integrity mismatch'; fi
ln "$kv_pending" "$kv_bin" 2>/dev/null
rm -f "$kv_pending"
kv_trusted "$kv_bin" || kv_fail 'Host bundle integrity mismatch'
printf '{"type":"uploaded"}\n'
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_platforms_namespaces_and_hashes() {
        let digest = |platform: &str, sha256: &str| AgentBinaryDigest {
            platform: platform.into(),
            sha256: sha256.into(),
        };
        let hash = "a".repeat(64);
        assert!(
            build_agent_loader_script(
                "dev",
                AgentLaunchMode::HostBridge,
                &[digest("Linux-x86_64", &hash)]
            )
            .is_some()
        );
        assert!(build_agent_loader_script("../dev", AgentLaunchMode::HostBridge, &[]).is_none());
        assert!(
            build_agent_loader_script(
                "dev",
                AgentLaunchMode::HostBridge,
                &[digest("Plan9-x86", &hash)]
            )
            .is_none()
        );
        assert!(
            build_agent_loader_script(
                "dev",
                AgentLaunchMode::HostBridge,
                &[digest("Linux-x86_64", "zz")]
            )
            .is_none()
        );
    }
}
