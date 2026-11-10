use crate::integrations::MirrorTarget;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock, Weak,
    },
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

const STREAM_QUEUE_CHUNKS: usize = 256;
const STREAM_CHUNK_BYTES: usize = 16 * 1024;
const STREAM_READY: &str = "KANVIBE_PANE_STREAM_READY";

#[derive(Clone)]
struct Subscriber {
    output: mpsc::Sender<Vec<u8>>,
    overflow: Arc<AtomicBool>,
}
struct Stream {
    process: Mutex<Child>,
    subscribers: Mutex<HashMap<u64, Subscriber>>,
    clients: AtomicUsize,
    ended: AtomicBool,
    cleaned: AtomicBool,
    refresh: AtomicBool,
    ready: Arc<AtomicBool>,
    cleanup: String,
    host: Option<String>,
}
#[derive(Default)]
struct Registry {
    streams: Mutex<HashMap<String, Arc<Stream>>>,
    next_subscriber: AtomicU64,
}
static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();

pub struct Subscription {
    registry: Weak<Registry>,
    key: String,
    stream: Arc<Stream>,
    id: u64,
    pub output: mpsc::Receiver<Vec<u8>>,
    pub overflow: Arc<AtomicBool>,
}

pub fn subscribe(target: &MirrorTarget, pane: &str) -> Result<Subscription, String> {
    let key = format!(
        "{}\0{}\0{pane}",
        target.host.as_deref().unwrap_or(""),
        target.name
    );
    let registry = REGISTRY
        .get_or_init(|| Arc::new(Registry::default()))
        .clone();
    if target.kind == "herdr" {
        let extent = crate::herdr::pane_extent(
            &target.name,
            pane,
            target.host.as_deref(),
            &crate::herdr::system_shell,
        )?;
        return registry.subscribe_source(
            key,
            || {
                Ok((
                    spawn_herdr_observer(target, pane)?,
                    String::new(),
                    target.host.clone(),
                ))
            },
            Some((target.clone(), pane.to_owned(), extent)),
        );
    }
    registry.subscribe(key, || {
        let directory = format!("/tmp/kanvibe-pane-{}", uuid::Uuid::new_v4());
        let script = stream_script(pane, &directory);
        let mut command = stream_command(target.host.as_deref(), &script);
        let cleanup = cleanup_script(pane, &directory);
        let process = command.spawn().map_err(|e| e.to_string())?;
        Ok((process, cleanup, target.host.clone()))
    })
}

impl Registry {
    fn subscribe(
        self: &Arc<Self>,
        key: String,
        spawn: impl FnOnce() -> Result<(Child, String, Option<String>), String>,
    ) -> Result<Subscription, String> {
        self.subscribe_source(key, spawn, None)
    }
    fn subscribe_source(
        self: &Arc<Self>,
        key: String,
        spawn: impl FnOnce() -> Result<(Child, String, Option<String>), String>,
        herdr: Option<(MirrorTarget, String, crate::herdr::PaneExtent)>,
    ) -> Result<Subscription, String> {
        let id = self.next_subscriber.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(STREAM_QUEUE_CHUNKS);
        let overflow = Arc::new(AtomicBool::new(false));
        let subscriber = Subscriber {
            output: tx,
            overflow: overflow.clone(),
        };
        let mut streams = self.streams.lock().map_err(|e| e.to_string())?;
        if let Some(existing) = streams.get(&key) {
            if !existing.ended.load(Ordering::Acquire) {
                existing
                    .subscribers
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(id, subscriber);
                existing.clients.fetch_add(1, Ordering::Relaxed);
                existing.refresh.store(true, Ordering::Release);
                return Ok(Subscription {
                    registry: Arc::downgrade(self),
                    key,
                    stream: existing.clone(),
                    id,
                    output: rx,
                    overflow,
                });
            }
            existing.stop();
            streams.remove(&key);
        }
        let (mut process, cleanup, host) = spawn()?;
        let stdout = process.stdout.take().ok_or("Missing pane stream stdout")?;
        let stderr = process.stderr.take().ok_or("Missing pane stream stderr")?;
        let ready = Arc::new(AtomicBool::new(false));
        let stream = Arc::new(Stream {
            process: Mutex::new(process),
            subscribers: Mutex::new(HashMap::from([(id, subscriber)])),
            clients: AtomicUsize::new(1),
            ended: AtomicBool::new(false),
            cleaned: AtomicBool::new(false),
            refresh: AtomicBool::new(false),
            ready: ready.clone(),
            cleanup,
            host,
        });
        streams.insert(key.clone(), stream.clone());
        let weak = Arc::downgrade(&stream);
        if let Some((target, pane, extent)) = herdr {
            std::thread::spawn(move || observe_herdr(weak, target, pane, extent, stdout));
        } else {
            std::thread::spawn(move || {
                let mut stdout = stdout;
                let mut buffer = [0u8; STREAM_CHUNK_BYTES];
                loop {
                    match stdout.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            let Some(stream) = weak.upgrade() else { break };
                            stream.publish(&buffer[..count]);
                        }
                    }
                }
                if let Some(stream) = weak.upgrade() {
                    stream.ended.store(true, Ordering::Release);
                    if let Ok(mut subscribers) = stream.subscribers.lock() {
                        subscribers.clear();
                    }
                }
            });
        }
        // stderr는 읽어 비워야 SSH 경고가 파이프를 채워 출력을 막지 않는다.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if line == STREAM_READY {
                    ready.store(true, Ordering::Release);
                }
            }
        });
        Ok(Subscription {
            registry: Arc::downgrade(self),
            key,
            stream,
            id,
            output: rx,
            overflow,
        })
    }
}

fn spawn_herdr_observer(target: &MirrorTarget, pane: &str) -> Result<Child, String> {
    let panes = crate::herdr::mirror_panes(
        &target.name,
        target.host.as_deref(),
        &crate::herdr::system_shell,
    )?;
    let pane = panes
        .iter()
        .find(|entry| entry["id"] == pane)
        .ok_or("session-not-running")?;
    let width = pane["width"]
        .as_u64()
        .ok_or("Herdr pane 너비가 없습니다.")?;
    let height = pane["height"]
        .as_u64()
        .ok_or("Herdr pane 높이가 없습니다.")?;
    let command = crate::herdr::observe_command(
        &target.name,
        pane["id"].as_str().ok_or("Herdr pane ID가 없습니다.")?,
        width,
        height,
    );
    stream_command(target.host.as_deref(), &command)
        .spawn()
        .map_err(|error| error.to_string())
}

type ObserverEvent = (u64, Option<crate::herdr::ObserveRecord>);
fn read_herdr_frames(
    stdout: impl Read + Send + 'static,
    epoch: u64,
    sender: std::sync::mpsc::SyncSender<ObserverEvent>,
) {
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(record) = crate::herdr::parse_observe_record(&line) {
                if sender.send((epoch, Some(record))).is_err() {
                    return;
                }
            }
        }
        let _ = sender.send((epoch, None));
    });
}
fn observe_herdr(
    weak: Weak<Stream>,
    target: MirrorTarget,
    pane: String,
    mut extent: crate::herdr::PaneExtent,
    stdout: impl Read + Send + 'static,
) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(STREAM_QUEUE_CHUNKS);
    let mut epoch = 0;
    read_herdr_frames(stdout, epoch, sender.clone());
    // 화면 폴링 간격을 재사용해 출력이 잠잠해진 뒤에만 이력을 읽고 SSH 왕복을 제한한다.
    let quiet_period = Duration::from_secs(if target.host.is_some() { 3 } else { 1 });
    let mut quiet_deadline = None;
    loop {
        let Some(stream) = weak.upgrade() else { break };
        if stream.ended.load(Ordering::Acquire) {
            break;
        }
        let mut restart = stream.refresh.swap(false, Ordering::AcqRel);
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok((record_epoch, record)) if record_epoch == epoch => match record {
                Some(crate::herdr::ObserveRecord::Frame { full, output }) => {
                    stream.publish(&output);
                    if !full {
                        quiet_deadline = Some(Instant::now() + quiet_period);
                    }
                }
                Some(crate::herdr::ObserveRecord::Closed) | None => {
                    stream.stop();
                    break;
                }
            },
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                stream.stop();
                break;
            }
        }
        let result = (|| {
            if quiet_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                quiet_deadline = None;
                let next = crate::herdr::pane_extent(
                    &target.name,
                    &pane,
                    target.host.as_deref(),
                    &crate::herdr::system_shell,
                )?;
                if next != extent {
                    extent = next;
                    let mut child = stream.process.lock().map_err(|error| error.to_string())?;
                    terminate(&mut child);
                    drop(child);
                    if stream.ended.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    stream.publish(
                        crate::integrations::snapshot_verified(&target, &pane)?.as_bytes(),
                    );
                    restart = true;
                }
            }
            if restart && !stream.ended.load(Ordering::Acquire) {
                let mut child = stream.process.lock().map_err(|error| error.to_string())?;
                terminate(&mut child);
                if stream.ended.load(Ordering::Acquire) {
                    return Ok(());
                }
                let mut observer = spawn_herdr_observer(&target, &pane)?;
                let stdout = observer
                    .stdout
                    .take()
                    .ok_or("Missing Herdr observe stdout")?;
                let stderr = observer
                    .stderr
                    .take()
                    .ok_or("Missing Herdr observe stderr")?;
                // SSH의 stderr가 가득 차면 NDJSON 스트림도 멈추므로 별도로 비운다.
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut BufReader::new(stderr), &mut std::io::sink());
                });
                *child = observer;
                epoch += 1;
                read_herdr_frames(stdout, epoch, sender.clone());
                quiet_deadline = None;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            eprintln!("Herdr observe failed: {error}");
            stream.stop();
            break;
        }
    }
}

impl Stream {
    fn publish(&self, bytes: &[u8]) {
        let Ok(mut subscribers) = self.subscribers.lock() else {
            return;
        };
        subscribers.retain(
            |_, subscriber| match subscriber.output.try_send(bytes.to_vec()) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    subscriber.overflow.store(true, Ordering::Release);
                    true
                }
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            },
        );
    }
    fn stop(&self) {
        if self.cleaned.swap(true, Ordering::AcqRel) {
            return;
        }
        self.ended.store(true, Ordering::Release);
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.clear();
        }
        if let Ok(mut child) = self.process.lock() {
            terminate(&mut child);
        }
        // 강제 종료나 SSH 끊김으로 trap이 실행되지 못해도 자신이 연 파이프만 정리한다.
        if self.ready.load(Ordering::Acquire) || !self.cleanup.is_empty() {
            let _ = crate::git::run(&self.cleanup, self.host.as_deref());
        }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        if let Ok(mut subscribers) = self.stream.subscribers.lock() {
            subscribers.remove(&self.id);
        }
        if self.stream.clients.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        if let Some(registry) = self.registry.upgrade() {
            if let Ok(mut streams) = registry.streams.lock() {
                if streams
                    .get(&self.key)
                    .is_some_and(|entry| Arc::ptr_eq(entry, &self.stream))
                {
                    self.stream.stop();
                    streams.remove(&self.key);
                    return;
                }
            }
        }
        self.stream.stop();
    }
}

fn stream_command(host: Option<&str>, script: &str) -> Command {
    let mut command = if let Some(host) = host {
        let mut command = Command::new("ssh");
        command.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "--",
            host,
        ]);
        command.arg(format!("sh -lc {}", crate::runtime::quote(script)));
        command
    } else {
        let mut command = Command::new("/bin/sh");
        command.args(["-lc", script]);
        command
    };
    command
        .env_clear()
        .envs(crate::runtime::shell_environment());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}
fn terminate(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-TERM", &format!("-{}", child.id())])
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", &format!("-{}", child.id())])
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn stream_script(pane: &str, directory: &str) -> String {
    let q = crate::runtime::quote;
    // 출력 저장소를 FIFO로 두어 연결 시간과 무관하게 디스크 사용량을 일정하게 유지한다.
    format!(
        r#"set -eu
stream_directory={directory}
stream_sink="$stream_directory/output"
stream_installed=0
stream_reader=
mkdir -m 700 -- "$stream_directory"
cleanup() {{
 trap - EXIT HUP INT TERM
 if [ "$stream_installed" -eq 1 ]; then tmux pipe-pane -t {pane} >/dev/null 2>&1 || true; fi
 if [ -n "$stream_reader" ]; then kill "$stream_reader" 2>/dev/null || true; wait "$stream_reader" 2>/dev/null || true; fi
 rm -f -- "$stream_sink" "$stream_directory/installed"
 rmdir -- "$stream_directory" 2>/dev/null || true
}}
trap cleanup EXIT
trap 'exit 0' HUP INT TERM
mkfifo -m 600 -- "$stream_sink"
stream_existing=$(tmux display-message -p -t {pane} '#{{pane_pipe}}')
[ "$stream_existing" = 0 ] || exit 70
tmux pipe-pane -o -t {pane} "cat > '$stream_sink'"
stream_installed=1
: > "$stream_directory/installed"
printf '%s\n' {ready} >&2
cat -- "$stream_sink" & stream_reader=$!
wait "$stream_reader"
"#,
        directory = q(directory),
        pane = q(pane),
        ready = q(STREAM_READY)
    )
}

fn cleanup_script(pane: &str, directory: &str) -> String {
    let q = crate::runtime::quote;
    format!("if [ -d {directory} ]; then if [ -f {installed} ]; then tmux pipe-pane -t {pane} >/dev/null 2>&1 || true; fi; rm -f -- {fifo} {installed}; rmdir -- {directory} 2>/dev/null || true; fi", directory=q(directory), pane=q(pane), fifo=q(&format!("{directory}/output")),installed=q(&format!("{directory}/installed")))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn fixture_command(script: &str) -> Child {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        command.spawn().unwrap()
    }
    #[tokio::test]
    async fn shared_stream_spawns_once_and_last_subscriber_stops_process() {
        let registry = Arc::new(Registry::default());
        let starts = Arc::new(AtomicUsize::new(0));
        let process_id = Arc::new(AtomicU64::new(0));
        let child_id = process_id.clone();
        let count = starts.clone();
        let first = registry
            .subscribe("pane".into(), move || {
                count.fetch_add(1, Ordering::Relaxed);
                let child = fixture_command(
                    "trap 'exit 0' TERM; while :; do printf '한글\\033[?1049h'; sleep 0.02; done",
                );
                child_id.store(child.id() as u64, Ordering::Relaxed);
                Ok((child, String::new(), None))
            })
            .unwrap();
        let process = process_id.load(Ordering::Relaxed);
        let mut second = registry
            .subscribe("pane".into(), || panic!("must share existing stream"))
            .unwrap();
        let bytes = tokio::time::timeout(Duration::from_secs(2), async {
            let mut bytes = Vec::new();
            while !bytes.windows(8).any(|w| w == b"\x1b[?1049h") {
                bytes.extend(
                    second
                        .output
                        .recv()
                        .await
                        .expect("stream ended before ANSI mode"),
                );
            }
            bytes
        })
        .await
        .unwrap();
        assert!(bytes.windows(8).any(|w| w == b"\x1b[?1049h"));
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        drop(first);
        assert!(second.output.recv().await.is_some());
        drop(second);
        assert!(registry.streams.lock().unwrap().is_empty());
        assert!(!Command::new("/bin/kill")
            .args(["-0", &process.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
    }
    #[tokio::test]
    async fn herdr_frames_deliver_ansi_deltas_and_drop_terminates_observer() {
        use base64::Engine;
        let full = serde_json::json!({"type":"terminal.frame","full":true,"bytes":base64::engine::general_purpose::STANDARD.encode("한글\x1b[?1049h")}).to_string();
        let delta = serde_json::json!({"type":"terminal.frame","bytes":base64::engine::general_purpose::STANDARD.encode("\x1b[2;3Hnext")}).to_string();
        let registry = Arc::new(Registry::default());
        let mut process = 0;
        let mut subscription = registry
            .subscribe_source(
                "herdr".into(),
                || {
                    let child = fixture_command(&format!(
                        "printf '%s\\n' {} {}; sleep 30",
                        crate::runtime::quote(&full),
                        crate::runtime::quote(&delta)
                    ));
                    process = child.id();
                    Ok((child, String::new(), None))
                },
                Some((
                    MirrorTarget {
                        kind: "herdr".into(),
                        name: "test".into(),
                        host: None,
                    },
                    "w1:p1".into(),
                    crate::herdr::PaneExtent {
                        history_rows: 0,
                        viewport_rows: 38,
                    },
                )),
            )
            .unwrap();
        let frames = tokio::time::timeout(Duration::from_secs(2), async {
            vec![
                subscription.output.recv().await.unwrap(),
                subscription.output.recv().await.unwrap(),
            ]
        })
        .await
        .unwrap();
        assert_eq!(
            frames,
            vec!["한글\x1b[?1049h".as_bytes(), "\x1b[2;3Hnext".as_bytes()]
        );
        drop(subscription);
        assert!(registry.streams.lock().unwrap().is_empty());
        assert!(!Command::new("/bin/kill")
            .args(["-0", &process.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
    }
    #[tokio::test]
    async fn herdr_closed_record_ends_subscription_without_waiting_for_process_exit() {
        let registry = Arc::new(Registry::default());
        let mut subscription = registry
            .subscribe_source(
                "closed-herdr".into(),
                || {
                    Ok((
                        fixture_command("printf '%s\\n' '{\"type\":\"terminal.closed\"}'; sleep 30"),
                        String::new(),
                        None,
                    ))
                },
                Some((
                    MirrorTarget {
                        kind: "herdr".into(),
                        name: "test".into(),
                        host: None,
                    },
                    "w1:p1".into(),
                    crate::herdr::PaneExtent {
                        history_rows: 0,
                        viewport_rows: 38,
                    },
                )),
            )
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), subscription.output.recv())
                .await
                .unwrap(),
            None
        );
        assert!(subscription.stream.ended.load(Ordering::Acquire));
        drop(subscription);
        assert!(registry.streams.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn slow_subscriber_is_bounded_and_does_not_block_other_clients() {
        let registry = Arc::new(Registry::default());
        let mut fast = registry
            .subscribe("pane".into(), || {
                Ok((fixture_command("sleep 30"), String::new(), None))
            })
            .unwrap();
        let slow = registry
            .subscribe("pane".into(), || panic!("must share"))
            .unwrap();
        for _ in 0..STREAM_QUEUE_CHUNKS + 1 {
            fast.stream.publish(b"x");
            assert_eq!(fast.output.recv().await.unwrap(), b"x");
        }
        assert!(slow.overflow.load(Ordering::Acquire));
        assert!(!fast.overflow.load(Ordering::Acquire));
        assert_eq!(slow.output.len(), STREAM_QUEUE_CHUNKS);
        drop(slow);
        drop(fast);
    }
    #[tokio::test]
    async fn real_fifo_replays_bytes_without_a_growing_file_and_is_removed_on_drop() {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("tmux");
        let log = directory.path().join("commands");
        std::fs::write(
            &binary,
            r#"#!/bin/sh
if [ "$1" = display-message ]; then printf '0\n'; exit 0; fi
if [ "$1" = pipe-pane ] && [ "${{2:-}}" = -o ]; then
 printf 'installed\n' >> "$KANVIBE_TEST_STREAM_LOG"
 (printf '한글\033[?1049h'; sleep 30) | sh -c "$5" >/dev/null 2>&1 &
 exit 0
fi
printf 'cancelled\n' >> "$KANVIBE_TEST_STREAM_LOG"
"#
            .replace("${{2:-}}", "${2:-}"),
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let fifo_directory = directory.path().join("private-fifo");
        let script = stream_script("%12", fifo_directory.to_str().unwrap());
        let registry = Arc::new(Registry::default());
        let path = format!(
            "{}:{}",
            directory.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let log_clone = log.clone();
        let mut subscription = registry
            .subscribe("fifo".into(), move || {
                let mut command = Command::new("/bin/sh");
                command
                    .args(["-c", &script])
                    .env("PATH", path)
                    .env("KANVIBE_TEST_STREAM_LOG", log_clone)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                use std::os::unix::process::CommandExt;
                command.process_group(0);
                Ok((
                    command.spawn().map_err(|e| e.to_string())?,
                    String::new(),
                    None,
                ))
            })
            .unwrap();
        let expected = "한글\x1b[?1049h".as_bytes();
        let bytes = tokio::time::timeout(Duration::from_secs(2), async {
            let mut bytes = Vec::new();
            while bytes.len() < expected.len() {
                bytes.extend(
                    subscription
                        .output
                        .recv()
                        .await
                        .expect("FIFO ended before replay"),
                );
            }
            bytes
        })
        .await
        .unwrap();
        assert_eq!(bytes, expected);
        let fifo = fifo_directory.join("output");
        assert!(std::fs::metadata(&fifo).unwrap().file_type().is_fifo());
        assert_eq!(
            std::fs::metadata(&fifo_directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        drop(subscription);
        assert!(!fifo_directory.exists());
        let commands = std::fs::read_to_string(log).unwrap();
        assert_eq!(commands.lines().filter(|s| *s == "installed").count(), 1);
        assert!(commands.contains("cancelled"));
    }
    #[test]
    fn fifo_script_uses_private_directory_and_cleans_pipe_on_signals() {
        let script = stream_script("%12", "/tmp/kanvibe-pane-test");
        let mut child = Command::new("/bin/sh")
            .args(["-n"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
        assert!(script.contains("mkfifo -m 600"));
        assert!(script.contains("mkdir -m 700"));
        assert!(script.contains("trap 'exit 0' HUP INT TERM"));
        assert!(!script.contains("tail -f"));
        assert!(script.contains("stream_installed"));
    }
}
