use crate::database::{text, Database};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::{collections::HashSet, path::Path, sync::Mutex, time::Duration};

pub const TASK_CLI_SCRIPT: &str = include_str!("../resources/task.sh");
const TASK_CLI_SECRET_KEY: &str = "task_cli_secret";
const SECRET_BYTES: usize = 32;
const PEER_RESULT_PREFIX: &str = "__KANVIBE_PEER_RESULT__\t";
// 원격 worktree 생성까지 기다리는 상대 client가 있어 incoming CLI와 같은 상한을 둔다.
const PEER_PROPAGATION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
static SECRET_CREATION: Mutex<()> = Mutex::new(());

fn token_mac(db: &Database, task: &str) -> Result<Hmac<Sha256>, String> {
    // 첫 발급이 겹쳐도 이미 배포한 토큰의 서명 키를 다른 호출이 덮어쓰지 못하게 한다.
    let _guard = SECRET_CREATION.lock().map_err(|error| error.to_string())?;
    let secret = match db
        .setting(TASK_CLI_SECRET_KEY)?
        .filter(|secret| !secret.is_empty())
    {
        Some(secret) => secret,
        None => {
            let bytes: [u8; SECRET_BYTES] = rand::random();
            let secret = hex::encode(bytes);
            db.set_setting(TASK_CLI_SECRET_KEY, &secret)?;
            secret
        }
    };
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|error| error.to_string())?;
    mac.update(task.as_bytes());
    Ok(mac)
}

pub fn build_token(db: &Database, task: &str) -> Result<String, String> {
    Ok(hex::encode(token_mac(db, task)?.finalize().into_bytes()))
}

pub fn is_valid_token(db: &Database, task: &str, submitted: &str) -> Result<bool, String> {
    let mac = token_mac(db, task)?;
    // Node의 hex 문자열 비교처럼 대소문자를 구분하고 실제 MAC 비교는 상수 시간 검증에 맡긴다.
    if !submitted
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Ok(false);
    }
    let Ok(bytes) = hex::decode(submitted) else {
        return Ok(false);
    };
    Ok(mac.verify_slice(&bytes).is_ok())
}

pub fn propagate_deletion(worktree: &str, host: Option<&str>) -> Vec<Value> {
    propagate(worktree, host, "propagate-delete", &[])
}

pub fn propagate_registration(repo: &str, branch: &str, host: Option<&str>) -> Vec<Value> {
    propagate(repo, host, "propagate-register", &["--branch", branch])
}

fn propagate(repo: &str, host: Option<&str>, action: &str, extra: &[&str]) -> Vec<Value> {
    let mut peers = Vec::new();
    let result = (|| {
        let own_url = crate::server::hook_url(host)?;
        let targets_path = Path::new(repo)
            .join(".kanvibe/targets.json")
            .to_string_lossy()
            .into_owned();
        let content = if host.is_some() {
            let path = crate::runtime::quote(&targets_path);
            crate::runtime::run_shell(&format!("test -f {path} && cat {path} || true"), host)?
        } else {
            // targets.json이 없는 예전 worktree는 전파할 client가 없는 것으로 다룬다.
            std::fs::read_to_string(&targets_path).unwrap_or_default()
        };
        peers = peer_urls(&content, &own_url);
        if peers.is_empty() {
            return Ok(Vec::new());
        }
        let mut arguments = vec![action, "--exclude-url", own_url.as_str()];
        arguments.extend_from_slice(extra);
        let output = crate::runtime::run_shell_input(
            &script_command(repo, &arguments),
            host,
            Some(TASK_CLI_SCRIPT.as_bytes()),
            PEER_PROPAGATION_TIMEOUT,
        )?;
        Ok::<_, String>(parse_peer_results(&output))
    })();
    match result {
        Ok(results) => results,
        Err(message) => {
            eprintln!("[task-cli] 다른 client로 전파하지 못했습니다: {message}");
            if peers.is_empty() {
                peers.push("unknown".into());
            }
            peers
                .into_iter()
                .map(
                    |url| json!({"url":url,"isSuccess":false,"httpStatus":"000","message":message}),
                )
                .collect()
        }
    }
}

fn peer_urls(content: &str, own_url: &str) -> Vec<String> {
    let state: Value = serde_json::from_str(content).unwrap_or(Value::Null);
    let mut seen = HashSet::new();
    state["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|target| {
            let url = text(target, "url").trim().trim_end_matches('/');
            if url.is_empty()
                || text(target, "taskId").trim().is_empty()
                || !seen.insert(url.to_owned())
                || url == own_url.trim_end_matches('/')
            {
                return None;
            }
            Some(url.to_owned())
        })
        .collect()
}

fn parse_peer_results(output: &str) -> Vec<Value> {
    output.lines().filter_map(|line| {
        let line = line.strip_prefix(PEER_RESULT_PREFIX)?;
        let mut fields = line.split('\t');
        let url = fields.next().unwrap_or("");
        let status = fields.next().unwrap_or("000");
        let message = fields.next().unwrap_or("");
        Some(json!({"url":url,"isSuccess":status.starts_with('2'),"httpStatus":status,"message":message}))
    }).collect()
}

fn script_command(repo: &str, arguments: &[&str]) -> String {
    let script_path = Path::new(repo)
        .join(".kanvibe/task.sh")
        .to_string_lossy()
        .into_owned();
    // checkout으로 바뀐 task.sh를 실행하지 않는다. stdin의 내장 본문을 eval하면 $0 경로는 targets.json 탐색에만 쓰인다.
    let mut command = format!(
        "bash -c {} {}",
        crate::runtime::quote("eval \"$(cat)\""),
        crate::runtime::quote(&script_path)
    );
    for argument in arguments {
        command.push(' ');
        command.push_str(&crate::runtime::quote(argument));
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
    };

    struct Client {
        url: String,
        calls: Arc<Mutex<Vec<Value>>>,
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }
    impl Client {
        fn start(status: u16, response: Value) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let captured = calls.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let worker = thread::spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let Ok((mut socket, _)) = listener.accept() else {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    socket
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut buffer = [0u8; 4096];
                    let (header_end, length) = loop {
                        let count = socket.read(&mut buffer).unwrap();
                        if count == 0 {
                            panic!("incomplete HTTP headers")
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                        if let Some(index) =
                            bytes.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            let headers = String::from_utf8_lossy(&bytes[..index]);
                            let length = headers
                                .lines()
                                .find_map(|line| {
                                    line.to_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|value| value.trim().parse::<usize>().unwrap())
                                })
                                .unwrap();
                            break (index + 4, length);
                        }
                    };
                    while bytes.len() < header_end + length {
                        let count = socket.read(&mut buffer).unwrap();
                        assert!(count > 0);
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    let headers = String::from_utf8_lossy(&bytes[..header_end]);
                    let authorization = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("Authorization: "))
                        .unwrap_or("");
                    let path = headers
                        .lines()
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .nth(1)
                        .unwrap();
                    let body: Value =
                        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                    captured
                        .lock()
                        .unwrap()
                        .push(json!({"path":path,"authorization":authorization,"body":body}));
                    let body = response.to_string();
                    let message=format!("HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{body}",body.len());
                    socket.write_all(message.as_bytes()).unwrap();
                }
            });
            Self {
                url,
                calls,
                stop,
                worker: Some(worker),
            }
        }
        fn target(&self, id: &str) -> Value {
            json!({"url":self.url,"taskId":id,"token":format!("token-{id}")})
        }
    }
    impl Drop for Client {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            self.worker.take().unwrap().join().unwrap();
        }
    }
    fn repo(targets: Value) -> tempfile::TempDir {
        let directory = tempfile::Builder::new()
            .prefix("task cli ' fixture ")
            .tempdir()
            .unwrap();
        std::fs::create_dir(directory.path().join(".kanvibe")).unwrap();
        std::fs::write(
            directory.path().join(".kanvibe/targets.json"),
            json!({"schemaVersion":1,"targets":targets}).to_string(),
        )
        .unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q", "-b", "feature/current"])
            .arg(directory.path())
            .status()
            .unwrap()
            .success());
        directory
    }
    fn run_cli(repo: &Path, args: &[&str]) -> Result<String, String> {
        crate::runtime::run_shell_input(
            &script_command(repo.to_str().unwrap(), args),
            None,
            Some(TASK_CLI_SCRIPT.as_bytes()),
            Duration::from_secs(10),
        )
    }
    fn cli_output(repo: &Path, args: &[&str]) -> std::process::Output {
        use std::process::{Command, Stdio};
        let mut child = Command::new("/bin/sh")
            .args(["-lc", &script_command(repo.to_str().unwrap(), args)])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(TASK_CLI_SCRIPT.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    #[test]
    fn built_in_script_creates_once_with_current_branch_and_literal_json_title() {
        let first = Client::start(201, json!({"success":true}));
        let second = Client::start(201, json!({"success":true}));
        let repo = repo(json!([first.target("one"), second.target("two")]));
        let title = "quote \" slash \\ 한글\nline\tend\u{0001}";
        run_cli(
            repo.path(),
            &["create", "--branch", "feature/new", "--title", title],
        )
        .unwrap();
        assert!(second.calls.lock().unwrap().is_empty());
        assert_eq!(
            first.calls.lock().unwrap()[0],
            json!({"path":"/api/cli/tasks","authorization":"Bearer token-one","body":{"contextTaskId":"one","branchName":"feature/new","baseBranch":"feature/current","title":title}})
        );
    }

    #[test]
    fn fallback_only_happens_before_connection_and_http_failure_is_not_retried() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let client = Client::start(201, json!({"success":true}));
        let fixture =
            repo(json!([{"url":closed,"taskId":"offline","token":"unused"},client.target("live")]));
        run_cli(fixture.path(), &["create", "--branch", "feature/new"]).unwrap();
        assert_eq!(client.calls.lock().unwrap().len(), 1);
        let failed = Client::start(409, json!({"success":false,"error":"conflict"}));
        let fixture = repo(json!([failed.target("failed"), client.target("live")]));
        assert!(
            run_cli(fixture.path(), &["create", "--branch", "feature/new"])
                .unwrap_err()
                .contains("conflict")
        );
        assert_eq!(client.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn deletion_requires_yes_and_preserves_force_and_peer_warning() {
        let client = Client::start(
            200,
            json!({"success":true,"data":{"peers":[{"isSuccess":false}]}}),
        );
        let fixture = repo(json!([client.target("self")]));
        assert!(run_cli(fixture.path(), &["delete"])
            .unwrap_err()
            .contains("--yes"));
        assert!(client.calls.lock().unwrap().is_empty());
        let output = cli_output(
            fixture.path(),
            &["delete", "--yes", "--force", "--branch", "other"],
        );
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("다른 client 1곳에는 삭제를 전파하지 못했습니다"));
        assert_eq!(
            client.calls.lock().unwrap()[0]["body"],
            json!({"contextTaskId":"self","confirmed":true,"force":true,"branchName":"other"})
        );
    }

    #[test]
    fn peer_propagation_uses_builtin_stdin_and_never_executes_checked_out_script() {
        let own = Client::start(200, json!({"success":true}));
        let peer = Client::start(200, json!({"success":true}));
        let fixture = repo(json!([own.target("own"), peer.target("peer")]));
        let marker = fixture.path().join("untrusted-executed");
        std::fs::write(
            fixture.path().join(".kanvibe/task.sh"),
            format!("touch {}", crate::runtime::quote(marker.to_str().unwrap())),
        )
        .unwrap();
        let output = run_cli(
            fixture.path(),
            &["propagate-delete", "--exclude-url", &own.url],
        )
        .unwrap();
        assert!(!marker.exists());
        assert!(own.calls.lock().unwrap().is_empty());
        assert_eq!(
            peer.calls.lock().unwrap()[0]["body"],
            json!({"contextTaskId":"peer","confirmed":true,"force":true,"propagateToPeers":false})
        );
        assert_eq!(parse_peer_results(&output)[0]["httpStatus"], "200");
        run_cli(
            fixture.path(),
            &[
                "propagate-register",
                "--exclude-url",
                &own.url,
                "--branch",
                "feature/it's new",
            ],
        )
        .unwrap();
        assert_eq!(
            peer.calls.lock().unwrap()[1]["body"],
            json!({"contextTaskId":"peer","branchName":"feature/it's new"})
        );
    }

    #[test]
    fn untrusted_target_urls_and_missing_tokens_cannot_issue_requests() {
        let fixture = repo(
            json!([{"url":"file:///etc/hosts","taskId":"file","token":"token"},{"url":"-K/tmp/options","taskId":"option","token":"token"}]),
        );
        assert_eq!(run_cli(fixture.path(), &["propagate-delete"]).unwrap(), "");
        let client = Client::start(200, json!({"success":true}));
        std::fs::write(
            fixture.path().join(".kanvibe/targets.json"),
            json!({"targets":[{"url":client.url,"taskId":"missing-token"}]}).to_string(),
        )
        .unwrap();
        assert!(run_cli(fixture.path(), &["create", "--branch", "new"])
            .unwrap_err()
            .contains("hooks"));
        assert!(client.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn concurrent_token_creation_keeps_one_persisted_secret() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(&directory.path().join("tokens.db")).unwrap());
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let db = db.clone();
                thread::spawn(move || build_token(&db, "same-task").unwrap())
            })
            .collect();
        let tokens: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert!(tokens.iter().all(|token| token == &tokens[0]));
        assert_eq!(
            db.setting(TASK_CLI_SECRET_KEY).unwrap().unwrap().len(),
            SECRET_BYTES * 2
        );
    }

    #[test]
    fn persisted_hmac_is_task_scoped_and_survives_database_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tokens.db");
        let db = Database::open(&path).unwrap();
        let first = build_token(&db, "task-1").unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(build_token(&db, "task-1").unwrap(), first);
        assert_ne!(build_token(&db, "task-2").unwrap(), first);
        assert!(is_valid_token(&db, "task-1", &first).unwrap());
        assert!(!is_valid_token(&db, "task-2", &first).unwrap());
        assert!(!is_valid_token(&db, "task-1", "forged").unwrap());
        drop(db);
        assert!(is_valid_token(&Database::open(&path).unwrap(), "task-1", &first).unwrap());
    }

    #[test]
    fn hmac_matches_node_crypto_and_keeps_hex_comparison_case_sensitive() {
        let directory = tempfile::tempdir().unwrap();
        let db = Database::open(&directory.path().join("tokens.db")).unwrap();
        db.set_setting("task_cli_secret", "test-secret").unwrap();
        let token = build_token(&db, "task-1").unwrap();
        assert_eq!(
            token,
            "2e81b25d1744dc40bc8e838d2823e19fd4e9a248508da1e97f8aea0e3074fcb9"
        );
        assert!(!is_valid_token(&db, "task-1", &token.to_uppercase()).unwrap());
        db.set_setting(TASK_CLI_SECRET_KEY, "Jefe").unwrap();
        assert_eq!(
            build_token(&db, "what do ya want for nothing?").unwrap(),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn peer_results_keep_order_status_and_single_line_messages() {
        let output="noise\n__KANVIBE_PEER_RESULT__\thttp://peer\t200\t{\"success\":true}\n__KANVIBE_PEER_RESULT__\thttp://offline\t000\t연결할 수 없습니다\n";
        assert_eq!(
            parse_peer_results(output),
            vec![
                json!({"url":"http://peer","isSuccess":true,"httpStatus":"200","message":"{\"success\":true}"}),
                json!({"url":"http://offline","isSuccess":false,"httpStatus":"000","message":"연결할 수 없습니다"})
            ]
        );
        assert_eq!(
            peer_urls(
                r#"{"targets":[{"url":" http://own/ ","taskId":"a"},{"url":"http://peer/","taskId":"b"},{"url":"http://peer","taskId":"c"},{"url":"http://invalid"}]}"#,
                "http://own"
            ),
            vec!["http://peer"]
        );
    }
}
