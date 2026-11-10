use crate::database::Database;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Condvar, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

pub type Hosts = Arc<Mutex<HashSet<String>>>;
static CONTROL: OnceLock<Mutex<Option<mpsc::Sender<Option<String>>>>> = OnceLock::new();
static TUNNELS: OnceLock<Mutex<HashMap<String, Arc<Tunnel>>>> = OnceLock::new();
struct Tunnel {
    host: String,
    hosts: Hosts,
    socket: PathBuf,
    child: Mutex<Option<Child>>,
    closed: AtomicBool,
    reconnect: AtomicBool,
    wait: Mutex<()>,
    wake: Condvar,
}
pub fn valid_client_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}
pub fn task_in_scope(
    task: &Value,
    project: Option<&Value>,
    hosts: Option<&HashSet<String>>,
) -> bool {
    hosts.is_none_or(|hosts| {
        task["sshHost"]
            .as_str()
            .or_else(|| project.and_then(|project| project["sshHost"].as_str()))
            .is_some_and(|host| hosts.contains(host))
    })
}
pub fn shell_socket_resolver() -> &'static str {
    r#"kanvibe_hook_socket() {
  case "$1" in
    ""|*[!A-Za-z0-9-]*) return 0 ;;
  esac
  kanvibe_socket_dir="/tmp/kv-$(id -u)"
  kanvibe_socket_path="${kanvibe_socket_dir}/$1.sock"
  if [ ! -L "${kanvibe_socket_dir}" ] && [ -O "${kanvibe_socket_dir}" ] && [ "$(ls -ld "${kanvibe_socket_dir}" 2>/dev/null | cut -c1-10)" = "drwx------" ] && [ ! -L "${kanvibe_socket_path}" ] && [ -S "${kanvibe_socket_path}" ] && [ -O "${kanvibe_socket_path}" ]; then
    printf '%s' "${kanvibe_socket_path}"
  fi
}"#
}
fn prepare_command(client: &str) -> Result<String, String> {
    if !valid_client_id(client) {
        return Err("소켓 이름으로 쓸 수 없는 client ID입니다".into());
    }
    Ok(format!(
        r#"kanvibe_socket_dir="/tmp/kv-$(id -u)" && mkdir -p "${{kanvibe_socket_dir}}" && [ ! -L "${{kanvibe_socket_dir}}" ] && [ -O "${{kanvibe_socket_dir}}" ] && chmod 700 "${{kanvibe_socket_dir}}" && rm -f "${{kanvibe_socket_dir}}/{client}.sock" && printf '%s' "${{kanvibe_socket_dir}}/{client}.sock""#
    ))
}
fn endpoint_key(config: &str) -> Result<String, String> {
    let value = |name: &str| config.lines().find_map(|line| line.strip_prefix(name));
    let host = value("hostname ").ok_or("SSH hostname이 없습니다")?;
    let user = value("user ").ok_or("SSH user가 없습니다")?;
    let port = value("port ").unwrap_or("22");
    Ok(format!("{user}@{}:{port}", host.to_lowercase()))
}
fn ssh_args(host: &str, remote: &str, local: &str) -> Vec<String> {
    [
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "StreamLocalBindUnlink=yes",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-R",
        &format!("{remote}:{local}"),
        "--",
        host,
        "cat >/dev/null",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
pub fn ensure(host: &str) {
    if let Some(sender) = CONTROL
        .get()
        .and_then(|control| control.lock().ok())
        .and_then(|control| control.as_ref().cloned())
    {
        let _ = sender.send(Some(host.to_owned()));
    }
}
pub fn reconnect() {
    if let Some(tunnels) = TUNNELS.get().and_then(|state| state.lock().ok()) {
        for tunnel in tunnels.values() {
            tunnel.reconnect.store(true, Ordering::Release);
            if let Ok(mut child) = tunnel.child.lock() {
                if let Some(child) = child.as_mut() {
                    let _ = child.kill();
                }
            }
            tunnel.wake.notify_all();
        }
    }
}
pub fn stop() {
    stop_power_resume();
    if let Some(control) = CONTROL.get() {
        if let Ok(mut control) = control.lock() {
            if let Some(sender) = control.take() {
                let _ = sender.send(None);
            }
        }
    }
    if let Some(tunnels) = TUNNELS.get() {
        if let Ok(mut tunnels) = tunnels.lock() {
            for (_, tunnel) in tunnels.drain() {
                close(&tunnel);
            }
        }
    }
}
fn close(tunnel: &Tunnel) {
    tunnel.closed.store(true, Ordering::Release);
    if let Ok(mut child) = tunnel.child.lock() {
        if let Some(child) = child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    tunnel.wake.notify_all();
    let _ = std::fs::remove_file(&tunnel.socket);
}
#[cfg(not(unix))]
pub fn start(_app: AppHandle) {}
#[cfg(unix)]
pub fn start(app: AppHandle) {
    let (sender, receiver) = mpsc::channel();
    let mut control = CONTROL.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if control.is_some() {
        return;
    }
    *control = Some(sender);
    drop(control);
    register_power_resume();
    std::thread::spawn(move || {
        use std::os::unix::fs::PermissionsExt;
        let directory = PathBuf::from(format!("/tmp/kvh-{}", uuid::Uuid::new_v4().simple()));
        let create = std::fs::create_dir(&directory).and_then(|_| {
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        });
        if let Err(error) = create {
            eprintln!("Hook tunnel directory: {error}");
            return;
        }
        let reconcile = || -> Result<(), String> {
            let db = app.state::<Database>();
            let hosts: HashSet<String> = db
                .tasks()?
                .into_iter()
                .chain(db.projects()?)
                .filter_map(|entity| {
                    entity["sshHost"]
                        .as_str()
                        .filter(|host| !host.is_empty())
                        .map(str::to_owned)
                })
                .collect();
            for host in &hosts {
                if let Err(error) = open(&app, &directory, host) {
                    eprintln!("Hook tunnel {host}: {error}");
                }
            }
            let mut tunnels = TUNNELS
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .map_err(|error| error.to_string())?;
            tunnels.retain(|_, tunnel| {
                let keep = tunnel
                    .hosts
                    .lock()
                    .map(|mut aliases| {
                        aliases.retain(|alias| hosts.contains(alias));
                        !aliases.is_empty()
                    })
                    .unwrap_or(false);
                if !keep {
                    close(tunnel)
                }
                keep
            });
            Ok(())
        };
        if let Err(error) = reconcile() {
            eprintln!("Hook tunnel reconcile: {error}");
        }
        loop {
            match receiver.recv_timeout(Duration::from_secs(5 * 60)) {
                Ok(Some(host)) => {
                    if let Err(error) = open(&app, &directory, &host) {
                        eprintln!("Hook tunnel {host}: {error}");
                    }
                }
                Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Err(error) = reconcile() {
                        eprintln!("Hook tunnel reconcile: {error}");
                    }
                }
            }
        }
        if let Some(tunnels) = TUNNELS.get() {
            if let Ok(mut tunnels) = tunnels.lock() {
                for (_, tunnel) in tunnels.drain() {
                    close(&tunnel);
                }
            }
        }
        let _ = std::fs::remove_dir_all(directory);
    });
}
#[cfg(unix)]
fn open(app: &AppHandle, directory: &std::path::Path, host: &str) -> Result<(), String> {
    let mut tunnels = TUNNELS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|error| error.to_string())?;
    if tunnels
        .values()
        .any(|tunnel| tunnel.hosts.lock().is_ok_and(|hosts| hosts.contains(host)))
    {
        return Ok(());
    }
    let output = Command::new("ssh")
        .args(["-G", "--", host])
        .env_clear()
        .envs(crate::runtime::shell_environment())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let key = endpoint_key(&String::from_utf8_lossy(&output.stdout))?;
    if let Some(tunnel) = tunnels.get(&key) {
        tunnel
            .hosts
            .lock()
            .map_err(|error| error.to_string())?
            .insert(host.into());
        return Ok(());
    }
    let socket = directory.join(format!("{}.sock", uuid::Uuid::new_v4().simple()));
    let listener =
        std::os::unix::net::UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let tunnel = Arc::new(Tunnel {
        host: host.into(),
        hosts: Arc::new(Mutex::new(HashSet::from([host.into()]))),
        socket,
        child: Mutex::new(None),
        closed: AtomicBool::new(false),
        reconnect: AtomicBool::new(false),
        wait: Mutex::new(()),
        wake: Condvar::new(),
    });
    let db = app.state::<Database>();
    let client =
        match crate::core::settings(&db, "getOrCreateHookClientId", &[]).and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or("Hook client ID가 없습니다".into())
        }) {
            Ok(client) => client,
            Err(error) => {
                drop(listener);
                let _ = std::fs::remove_file(&tunnel.socket);
                return Err(error);
            }
        };
    tunnels.insert(key, tunnel.clone());
    let app = app.clone();
    let serving = tunnel.clone();
    tauri::async_runtime::spawn(async move {
        let result = async {
            let listener =
                tokio::net::UnixListener::from_std(listener).map_err(|error| error.to_string())?;
            axum::serve(
                listener,
                crate::server::tunnel_router(app, serving.hosts.clone()),
            )
            .with_graceful_shutdown(async move {
                while !serving.closed.load(Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            })
            .await
            .map_err(|error| error.to_string())
        }
        .await;
        if let Err(error) = result {
            eprintln!("Hook tunnel listener: {error}");
        }
    });
    std::thread::spawn(move || connect(tunnel, client));
    Ok(())
}
#[cfg(unix)]
fn connect(tunnel: Arc<Tunnel>, client: String) {
    let mut delay = Duration::from_secs(5);
    while !tunnel.closed.load(Ordering::Acquire) {
        let started = Instant::now();
        let result = (|| -> Result<(), String> {
            let remote = crate::git::run(&prepare_command(&client)?, Some(&tunnel.host))?;
            if tunnel.closed.load(Ordering::Acquire) {
                return Ok(());
            }
            let mut command = Command::new("ssh");
            command
                .args(ssh_args(
                    &tunnel.host,
                    remote.trim(),
                    &tunnel.socket.to_string_lossy(),
                ))
                .env_clear()
                .envs(crate::runtime::shell_environment())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped());
            let mut slot = tunnel.child.lock().map_err(|error| error.to_string())?;
            if tunnel.closed.load(Ordering::Acquire) {
                return Ok(());
            }
            let mut child = command.spawn().map_err(|error| error.to_string())?;
            if let Some(stderr) = child.stderr.take() {
                std::thread::spawn(move || {
                    let mut stderr = stderr;
                    let _ = std::io::copy(&mut stderr, &mut std::io::sink());
                });
            }
            *slot = Some(child);
            drop(slot);
            loop {
                if tunnel.closed.load(Ordering::Acquire) {
                    return Ok(());
                }
                if tunnel
                    .child
                    .lock()
                    .map_err(|error| error.to_string())?
                    .as_mut()
                    .is_some_and(|child| child.try_wait().ok().flatten().is_some())
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            tunnel
                .child
                .lock()
                .map_err(|error| error.to_string())?
                .take();
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("Hook tunnel {} failed: {error}", tunnel.host);
        }
        if tunnel.closed.load(Ordering::Acquire) {
            break;
        }
        let requested = tunnel.reconnect.swap(false, Ordering::AcqRel);
        if started.elapsed() >= Duration::from_secs(30) || requested {
            delay = Duration::from_secs(5);
        }
        let guard = tunnel.wait.lock().unwrap();
        let _ = tunnel.wake.wait_timeout_while(guard, delay, |_| {
            !tunnel.closed.load(Ordering::Acquire) && !tunnel.reconnect.load(Ordering::Acquire)
        });
        delay = (delay * 2).min(Duration::from_secs(5 * 60));
    }
}
#[cfg(target_os = "macos")]
type PowerObserver = (
    objc2::rc::Retained<objc2_foundation::NSNotificationCenter>,
    objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
);
#[cfg(target_os = "macos")]
thread_local! {
    static POWER_OBSERVER:std::cell::RefCell<Option<PowerObserver>>=const{std::cell::RefCell::new(None)};
}
#[cfg(target_os = "macos")]
fn register_power_resume() {
    POWER_OBSERVER.with(|observer| {
        if observer.borrow().is_some() {
            return;
        }
        let center = objc2_app_kit::NSWorkspace::sharedWorkspace().notificationCenter();
        let callback =
            block2::RcBlock::new(|_: std::ptr::NonNull<objc2_foundation::NSNotification>| {
                reconnect()
            });
        // 창의 활성화가 아니라 OS 복귀를 받는다. callback은 상태를 캡처하지 않아 알림 스레드에서 호출할 수 있다.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(objc2_app_kit::NSWorkspaceDidWakeNotification),
                None,
                None,
                &callback,
            )
        };
        *observer.borrow_mut() = Some((center, token));
    });
}
#[cfg(target_os = "macos")]
fn stop_power_resume() {
    POWER_OBSERVER.with(|observer| {
        if let Some((center, token)) = observer.borrow_mut().take() {
            unsafe {
                center.removeObserver(token.as_ref());
            }
        }
    });
}
#[cfg(target_os = "linux")]
fn register_power_resume() {
    std::thread::spawn(|| {
        let result = (|| -> Result<(), String> {
            let connection =
                zbus::blocking::Connection::system().map_err(|error| error.to_string())?;
            let proxy = zbus::blocking::Proxy::new(
                &connection,
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
            )
            .map_err(|error| error.to_string())?;
            let signals = proxy
                .receive_signal("PrepareForSleep")
                .map_err(|error| error.to_string())?;
            for signal in signals {
                if signal
                    .body()
                    .deserialize::<(bool,)>()
                    .is_ok_and(|(sleeping,)| !sleeping)
                {
                    reconnect();
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("Hook tunnel power resume monitor: {error}");
        }
    });
}
#[cfg(not(target_os = "macos"))]
fn stop_power_resume() {}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn register_power_resume() {}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn tunnel_scope_never_crosses_hosts_or_local_tasks() {
        let hosts = HashSet::from(["remote".into(), "alias".into()]);
        assert!(task_in_scope(
            &json!({"sshHost":"alias"}),
            None,
            Some(&hosts)
        ));
        assert!(task_in_scope(
            &json!({}),
            Some(&json!({"sshHost":"remote"})),
            Some(&hosts)
        ));
        assert!(!task_in_scope(
            &json!({"sshHost":"other"}),
            Some(&json!({"sshHost":"remote"})),
            Some(&hosts)
        ));
        assert!(!task_in_scope(&json!({}), None, Some(&hosts)));
        assert!(task_in_scope(&json!({}), None, None));
    }
    #[cfg(unix)]
    #[test]
    fn shell_resolver_accepts_private_owned_sockets_and_rejects_links_or_public_dirs() {
        use std::os::unix::{fs::PermissionsExt, net::UnixListener};
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.path().join("client-id.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        let resolver =
            shell_socket_resolver().replace("/tmp/kv-$(id -u)", &root.path().to_string_lossy());
        let run = || {
            crate::git::run(&format!("{resolver}\nkanvibe_hook_socket client-id"), None).unwrap()
        };
        assert_eq!(run(), socket.to_string_lossy());
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(run(), "");
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::rename(&socket, root.path().join("real.sock")).unwrap();
        std::os::unix::fs::symlink(root.path().join("real.sock"), &socket).unwrap();
        assert_eq!(run(), "");
    }
    #[test]
    fn ssh_aliases_share_endpoint_and_forward_lifetime_is_owned() {
        assert_eq!(
            endpoint_key("hostname EXAMPLE.com\nuser rook\nport 2222\n").unwrap(),
            "rook@example.com:2222"
        );
        let args = ssh_args("alias", "/tmp/kv-501/id.sock", "/tmp/kvh-test/a.sock");
        for expected in [
            "ControlMaster=no",
            "ControlPath=none",
            "ExitOnForwardFailure=yes",
            "StreamLocalBindUnlink=yes",
            "cat >/dev/null",
            "/tmp/kv-501/id.sock:/tmp/kvh-test/a.sock",
        ] {
            assert!(args.iter().any(|value| value == expected));
        }
        assert!(prepare_command("id-123").unwrap().contains("[ ! -L"));
        for invalid in ["", "../victim", "a/b", "a b", "$(id)"] {
            assert!(prepare_command(invalid).is_err());
        }
    }
}
