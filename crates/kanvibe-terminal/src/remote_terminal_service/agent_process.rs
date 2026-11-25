//! 원격 terminal agent를 실행하는 경계. agent 실행 명령과 소스, 직접 연결용 TLS 인증서, ssh 프로세스를 각각 trait 뒤에 둔다.
//! agent 본체와 그 WSS 서버는 M4에서 Rust로 다시 쓰므로 앱이 새 agent를 이 trait들로 끼운다.
//! 데스크탑이 정하는 ssh 인자, stdin 레코드 모양, 시간 제한, 오류 문구는 서비스가 정하고 여기서는 바꾸지 않는다.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::environment::TerminalHostEnvironment;

/// 원격에서 띄울 agent. 실행 명령은 런타임이 없으면 `{"type":"unsupported"}` 한 줄을 내고 끝나야 하고,
/// 런타임이 있으면 `{"type":"bootstrap-ready"}`를 낸 뒤 stdin 첫 줄로 소스와 인증서를 받는다
pub trait RemoteTerminalAgentBundle: Send + Sync {
    /// `sh -lc`에 넣을 원격 셸 명령. 소스, 인증서 키, grant 권한은 넣지 않는다
    fn launch_command(&self) -> String;
    /// bootstrap 레코드의 `source`로 보낼 agent 소스
    fn read_source(&self) -> Result<String, String>;
}

/// agent의 직접 연결 WSS 서버가 쓸 자체 서명 인증서(PEM)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTerminalCertificate {
    pub certificate_pem: String,
    pub private_key_pem: String,
}

/// 직접 연결 TLS 인증서 발급(TS `selfsigned.generate`). TLS 서버 쪽 일이라 M4 서버가 구현한다.
/// TS는 commonName `KanVibe Remote Terminal`, RSA 2048, sha256 서명으로 만든다
pub trait RemoteTerminalCertificateIssuer: Send + Sync {
    /// `not_after_ms`(Unix epoch ms)까지 유효한 인증서를 만든다
    fn issue(&self, not_after_ms: i64) -> Result<RemoteTerminalCertificate, String>;
}

/// agent 프로세스 종료. 시스템 ssh면 SIGTERM이고, ssh 클라이언트가 죽으면 원격 프로세스는 stdin EOF를 받는다
pub trait RemoteTerminalAgentTermination: Send + Sync {
    fn terminate(&self);
}

/// 띄운 agent 프로세스. stderr는 실행기가 버린다(TS `child.stderr.resume()`)
pub struct RemoteTerminalAgentProcess {
    pub stdin: Box<dyn Write + Send>,
    pub stdout: Box<dyn Read + Send>,
    pub termination: Box<dyn RemoteTerminalAgentTermination>,
}

/// agent 프로세스 실행. 원격 host daemon이 붙은 호스트면 `ssh_arguments`의 마지막 원소(원격 명령)를 그 연결로 실행해도 된다
pub trait RemoteTerminalAgentSpawner: Send + Sync {
    fn spawn(
        &self,
        ssh_host: &str,
        ssh_arguments: &[String],
    ) -> Result<RemoteTerminalAgentProcess, String>;
}

/// 시스템 `ssh`로 agent를 띄운다. 자식에는 좁힌 로컬 셸 환경만 넘긴다
pub struct SystemRemoteTerminalAgentSpawner {
    environment: Arc<TerminalHostEnvironment>,
}

impl SystemRemoteTerminalAgentSpawner {
    pub fn new(environment: Arc<TerminalHostEnvironment>) -> Self {
        Self { environment }
    }
}

impl RemoteTerminalAgentSpawner for SystemRemoteTerminalAgentSpawner {
    fn spawn(
        &self,
        _ssh_host: &str,
        ssh_arguments: &[String],
    ) -> Result<RemoteTerminalAgentProcess, String> {
        let mut child = Command::new("ssh")
            .args(ssh_arguments)
            .env_clear()
            .envs(&self.environment.shell_environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let stdin = child.stdin.take().expect("stdin을 파이프로 열었다");
        let stdout = child.stdout.take().expect("stdout을 파이프로 열었다");
        let has_exited = Arc::new(AtomicBool::new(false));
        let termination = SystemAgentTermination {
            process_id: child.id(),
            has_exited: has_exited.clone(),
        };
        // 끝난 자식을 회수해 좀비로 남기지 않는다. 회수한 뒤에는 같은 pid에 신호를 보내지 않는다
        std::thread::spawn(move || {
            let _ = child.wait();
            has_exited.store(true, Ordering::SeqCst);
        });
        Ok(RemoteTerminalAgentProcess {
            stdin: Box::new(stdin),
            stdout: Box::new(stdout),
            termination: Box::new(termination),
        })
    }
}

struct SystemAgentTermination {
    process_id: u32,
    has_exited: Arc<AtomicBool>,
}

impl RemoteTerminalAgentTermination for SystemAgentTermination {
    fn terminate(&self) {
        if self.has_exited.load(Ordering::SeqCst) {
            return;
        }
        // SAFETY: 아직 회수하지 않은 자식 프로세스의 pid에만 신호를 보낸다
        unsafe {
            libc::kill(self.process_id as libc::pid_t, libc::SIGTERM);
        }
    }
}

/// 이 loader는 비공개 bootstrap 한 줄만 읽고 나머지 stdin을 agent에 넘긴다.
/// 소스, 인증서 키, grant 권한은 원격 프로세스 argv에 나타나지 않는다
const NODE_AGENT_LOADER: &str = r#"process.stdout.write('{"type":"bootstrap-ready"}\n');let b='';const f=c=>{b+=c;if(Buffer.byteLength(b)>16777216)process.exit(1);const n=b.indexOf('\n');if(n<0)return;process.stdin.off('data',f);process.stdin.pause();try{const p=JSON.parse(b.slice(0,n));const m={exports:{}};new Function('require','module','exports',p.source)(require,m,m.exports);m.exports.startRemoteTerminalAgent(p.certificate,process.stdin,process.stdout).then(()=>process.stdin.resume()).catch(()=>process.exit(1));}catch{process.exit(1)}};process.stdin.setEncoding('utf8');process.stdin.on('data',f);"#;

const NODE_AGENT_BUNDLE_PATH: [&str; 3] = ["build", "remote", "terminal-agent.cjs"];
/// 소스 실행과 패키징 빌드 모두 앱 루트를 기준으로 찾도록 위로 올라가며 찾는 단계 수
const NODE_AGENT_SEARCH_DEPTH: usize = 8;

/// 지금 TS 앱이 쓰는 Node agent 번들(`build/remote/terminal-agent.cjs`). 원격에 Node 18 이상이 있어야 한다
pub struct NodeRemoteTerminalAgentBundle {
    /// 번들을 찾기 시작할 디렉터리. TS는 `__dirname`이다
    search_start_directory: PathBuf,
}

impl NodeRemoteTerminalAgentBundle {
    pub fn new(search_start_directory: PathBuf) -> Self {
        Self {
            search_start_directory,
        }
    }
}

impl RemoteTerminalAgentBundle for NodeRemoteTerminalAgentBundle {
    fn launch_command(&self) -> String {
        [
            r#"kanvibe_node=$(command -v node) || { printf '{"type":"unsupported"}\n'; exit 0; }"#.to_string(),
            r#""$kanvibe_node" -e 'process.exit(Number(process.versions.node.split(".")[0])>=18?0:1)' || { printf '{"type":"unsupported"}\n'; exit 0; }"#.to_string(),
            format!(
                r#"exec "$kanvibe_node" -e {}"#,
                kanvibe_process::worktree::quote_for_posix_shell(NODE_AGENT_LOADER)
            ),
        ]
        .join("; ")
    }

    fn read_source(&self) -> Result<String, String> {
        let mut directory = self.search_start_directory.clone();
        for _ in 0..NODE_AGENT_SEARCH_DEPTH {
            let bundle_path: PathBuf = NODE_AGENT_BUNDLE_PATH
                .iter()
                .fold(directory.clone(), |path, segment| path.join(segment));
            match std::fs::read_to_string(&bundle_path) {
                Ok(source) => return Ok(source),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
            if !directory.pop() {
                break;
            }
        }
        Err("Remote terminal agent bundle is missing; run the desktop build.".to_string())
    }
}
