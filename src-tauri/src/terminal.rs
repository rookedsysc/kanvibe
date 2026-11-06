use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager};

type OutputListener = Arc<dyn Fn(Vec<u8>) + Send + Sync>;
struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    clients: HashMap<String, usize>,
    mobile: HashMap<String, OutputListener>,
    screen: vt100::Parser,
    task_id: String,
    tab_id: Option<String>,
    plain: bool,
    login_root: Option<String>,
    desktop_size: Option<(u16, u16)>,
    mobile_size: Option<(u16, u16)>,
}
#[derive(Default)]
pub struct TerminalState {
    sessions: Mutex<HashMap<String, Arc<Mutex<Session>>>>,
    tabs: Mutex<HashMap<String, TabState>>,
    pending_opens: PendingOpens,
}
/// 세션 준비를 PTY 잠금 밖에서 기다리는 연결을 추적해, 준비 중 창이 닫히면 PTY를 만들지 않게 한다.
#[derive(Default)]
struct PendingOpens {
    clients: Mutex<HashMap<u64, String>>,
    next_ticket: AtomicU64,
}
impl PendingOpens {
    fn begin(&self, client_id: &str) -> Result<u64, String> {
        let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        self.clients
            .lock()
            .map_err(err)?
            .insert(ticket, client_id.into());
        Ok(ticket)
    }
    /// 준비 도중 취소되지 않았으면 true를 돌려주며 대기 표시를 지운다.
    fn finish(&self, ticket: u64) -> Result<bool, String> {
        Ok(self.clients.lock().map_err(err)?.remove(&ticket).is_some())
    }
    fn cancel_client(&self, client_id: &str) {
        if let Ok(mut clients) = self.clients.lock() {
            clients.retain(|_, pending| pending != client_id);
        }
    }
}
struct TabState {
    entries: Vec<(String, String)>,
    active: String,
    next: u64,
}
fn key(task: &str, tab: Option<&str>) -> String {
    tab.map(|s| format!("{task}#{s}"))
        .unwrap_or_else(|| task.to_owned())
}
fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn task(app: &AppHandle, id: &str) -> Result<Value, String> {
    crate::core::dispatch(app, "kanban", "getTaskById", vec![json!(id)]).and_then(|v| {
        if v.is_null() {
            Err("작업을 찾을 수 없습니다.".into())
        } else {
            Ok(v)
        }
    })
}
fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

pub fn open(
    app: &AppHandle,
    client_id: &str,
    task_id: &str,
    tab_id: Option<&str>,
    cols: u16,
    rows: u16,
) -> Result<Value, String> {
    let target = task(app, task_id)?;
    let kind = field(&target, "sessionType").ok_or("작업에 연결된 세션이 없습니다.")?;
    let name = field(&target, "sessionName").ok_or("작업에 연결된 세션이 없습니다.")?;
    let cwd = field(&target, "worktreePath")
        .map(String::from)
        .unwrap_or_else(crate::runtime::home);
    let registry = app.state::<TerminalState>();
    let session_key = key(task_id, tab_id);
    let host = field(&target, "sshHost");
    let has_session = registry
        .sessions
        .lock()
        .map_err(err)?
        .contains_key(&session_key);
    let herdr_ticket = if kind == "herdr" && !has_session {
        Some(prepare_herdr(
            app, &registry, client_id, &target, name, host,
        )?)
    } else {
        None
    };
    let mut sessions = registry.sessions.lock().map_err(err)?;
    if let Some(ticket) = herdr_ticket {
        if !registry.pending_opens.finish(ticket)? {
            return Ok(json!({"ok":false,"error":"터미널 연결이 취소되었습니다."}));
        }
    }
    if let Some(existing) = sessions.get(&session_key) {
        let mut s = existing.lock().map_err(err)?;
        *s.clients.entry(client_id.to_owned()).or_default() += 1;
        remember_size(&mut s, client_id, cols, rows)?;
        let data = String::from_utf8_lossy(&snapshot(&s.screen)).into_owned();
        app.emit_to(
            client_id,
            "kanvibe:terminal-data",
            json!({"taskId":task_id,"tabId":tab_id,"data":data}),
        )
        .map_err(err)?;
        return Ok(json!({"ok":true}));
    }
    let layout = crate::core::dispatch(
        app,
        "paneLayout",
        "getEffectivePaneLayout",
        vec![target["projectId"].clone()],
    )?;
    if kind == "zellij" && !layout.is_null() {
        write_zellij_layout(&cwd, host, &layout)?;
    }
    let command = attach_command_with_layout(kind, name, &cwd, &layout);
    let mut cmd = if let Some(host) = host {
        crate::dependencies::ensure_remote(kind, host)?;
        let mut c = CommandBuilder::new("ssh");
        c.args([
            "-tt",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "TCPKeepAlive=yes",
            "--",
            host,
        ]);
        c.arg(format!("sh -lc {}", crate::runtime::quote(&command)));
        c.cwd(crate::runtime::home());
        c
    } else {
        let mut c = CommandBuilder::new(std::env::var("SHELL").unwrap_or("/bin/sh".into()));
        c.args(["-lc", &command]);
        c.cwd(if Path::new(&cwd).is_dir() {
            cwd
        } else {
            crate::runtime::home()
        });
        c
    };
    configure_environment(&mut cmd);
    let entry = spawn(
        app,
        &session_key,
        client_id,
        cmd,
        cols,
        rows,
        task_id,
        tab_id,
        kind == "terminal",
        None,
    )?;
    sessions.insert(session_key, entry);
    Ok(json!({"ok":true}))
}
/// Herdr 서버 기동과 첫 레이아웃 적용은 수 초가 걸릴 수 있어 다른 터미널을 막지 않도록 세션 잠금 밖에서 끝낸다.
fn prepare_herdr(
    app: &AppHandle,
    registry: &TerminalState,
    client_id: &str,
    target: &Value,
    name: &str,
    host: Option<&str>,
) -> Result<u64, String> {
    let ticket = registry.pending_opens.begin(client_id)?;
    let prepared = (|| {
        if let Some(host) = host {
            crate::dependencies::ensure_remote("herdr", host)?;
        }
        let layout = crate::core::dispatch(
            app,
            "paneLayout",
            "getEffectivePaneLayout",
            vec![target["projectId"].clone()],
        )?;
        let cwd = field(target, "worktreePath")
            .map(String::from)
            .unwrap_or_else(|| {
                if host.is_some() {
                    "~".into()
                } else {
                    crate::runtime::home()
                }
            });
        crate::herdr::ensure_session(name, &cwd, &layout, host, &crate::herdr::system_shell)
    })();
    if let Err(error) = prepared {
        registry.pending_opens.finish(ticket)?;
        return Err(error);
    }
    Ok(ticket)
}
fn configure_environment(cmd: &mut CommandBuilder) {
    cmd.env_clear();
    for (k, v) in crate::runtime::shell_environment() {
        cmd.env(k, v);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env_remove("TMUX");
    cmd.env_remove("TMUX_PANE");
    cmd.env_remove("ZELLIJ");
    cmd.env_remove("ZELLIJ_SESSION_NAME");
}
fn attach_command(kind: &str, name: &str, cwd: &str) -> String {
    let q = crate::runtime::quote;
    match kind{
        "tmux"=>format!("tmux has-session -t {name} 2>/dev/null || tmux new-session -d -s {name} -c {cwd}; tmux set-option -t {name} status off; exec tmux attach-session -t {name}",name=q(name),cwd=q(cwd)),
        "herdr"=>format!("exec {}",crate::herdr::command(name,&[])),
        "zellij"=>format!("cd {dir} || exit; if zellij list-sessions 2>/dev/null | awk '{{ gsub(/\\033\\[[0-9;]*m/, \"\") }} $1 != \"EXITED:\" {{ print $1 }}' | grep -qFx -- {name}; then exec zellij attach {name}; elif [ -f .zellij-layout.kdl ]; then exec zellij --session {name} --new-session-with-layout .zellij-layout.kdl; else exec zellij --session {name}; fi",dir=q(cwd),name=q(name)),
        _=>format!("cd {} || printf '%s\\n' 'KanVibe: cannot enter working directory' >&2; exec \"${{SHELL:-/bin/sh}}\" -l",q(cwd)),
    }
}
fn zellij_layout(cwd: &str, layout: &Value) -> String {
    let panes = layout["panes"].as_array();
    let pane = |position: u64, indent: &str| -> String {
        let command = panes
            .and_then(|p| p.iter().find(|p| p["position"].as_u64() == Some(position)))
            .and_then(|p| p["command"].as_str())
            .unwrap_or("")
            .trim();
        if command.is_empty() {
            format!("{indent}pane cwd={}", json!(cwd))
        } else {
            format!("{indent}pane command=\"bash\" {{\n{indent}    args \"-c\" {}\n{indent}    cwd {}\n{indent}}}",json!(command),json!(cwd))
        }
    };
    let body=match layout["layoutType"].as_str().unwrap_or("single"){
        "horizontal_2"=>format!("{}\n{}",pane(0,"    "),pane(1,"    ")),
        "vertical_2"=>format!("    pane split_direction=\"vertical\" {{\n{}\n{}\n    }}",pane(0,"        "),pane(1,"        ")),
        "left_right_tb"=>format!("    pane split_direction=\"vertical\" {{\n{}\n        pane {{\n{}\n{}\n        }}\n    }}",pane(0,"        "),pane(1,"            "),pane(2,"            ")),
        "left_tb_right"=>format!("    pane split_direction=\"vertical\" {{\n        pane {{\n{}\n{}\n        }}\n{}\n    }}",pane(0,"            "),pane(1,"            "),pane(2,"        ")),
        "quad"=>format!("    pane split_direction=\"vertical\" {{\n        pane {{\n{}\n{}\n        }}\n        pane {{\n{}\n{}\n        }}\n    }}",pane(0,"            "),pane(2,"            "),pane(1,"            "),pane(3,"            ")),
        _=>pane(0,"    ")
    };
    format!("layout {{\n{body}\n}}\n")
}
fn write_zellij_layout(cwd: &str, host: Option<&str>, layout: &Value) -> Result<(), String> {
    let body = zellij_layout(cwd, layout);
    let path = Path::new(cwd).join(".zellij-layout.kdl");
    if host.is_some() {
        crate::runtime::run_shell(
            &format!(
                "printf '%s' {} > {}",
                crate::runtime::quote(&body),
                crate::runtime::quote(&path.to_string_lossy())
            ),
            host,
        )?;
    } else {
        std::fs::write(path, body).map_err(err)?;
    }
    Ok(())
}
fn snapshot(parser: &vt100::Parser) -> Vec<u8> {
    let screen = parser.screen();
    if screen.alternate_screen() {
        let (rows, cols) = screen.size();
        let mut normal = vt100::Parser::new(rows, cols, 10000);
        *normal.screen_mut() = screen.clone();
        normal.process(b"\x1b[?1049l");
        let mut output = snapshot_screen(normal.screen());
        output.extend_from_slice(b"\x1b[?1049h");
        output.extend_from_slice(&screen.state_formatted());
        return output;
    }
    snapshot_screen(screen)
}
fn snapshot_screen(screen: &vt100::Screen) -> Vec<u8> {
    let mut view = screen.clone();
    let (rows, cols) = view.size();
    view.set_scrollback(usize::MAX);
    let history = view.scrollback();
    let mut output = b"\x1bc\x1b[3J\x1b[2J\x1b[H".to_vec();
    let mut wrapping = false;
    for offset in (1..=history).rev() {
        if wrapping {
            output.extend_from_slice(b" \x08");
        }
        view.set_scrollback(offset);
        output.extend_from_slice(b"\x1b[0m");
        if let Some(row) = view.rows_formatted(0, cols).next() {
            output.extend_from_slice(&row);
        }
        wrapping = view.row_wrapped(0);
        if !wrapping {
            output.extend_from_slice(b"\r\n");
        }
    }
    view.set_scrollback(0);
    for (row, bytes) in view.rows_formatted(0, cols).enumerate() {
        if wrapping {
            output.extend_from_slice(b" \x08");
        }
        output.extend_from_slice(b"\x1b[0m");
        output.extend_from_slice(&bytes);
        wrapping = view.row_wrapped(row as u16);
        if row + 1 < usize::from(rows) && !wrapping {
            output.extend_from_slice(b"\r\n");
        }
    }
    output.extend_from_slice(&screen.state_formatted());
    output
}
fn drain_utf8(pending: &mut Vec<u8>) -> String {
    let mut text = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(valid) => {
                text.push_str(valid);
                pending.clear();
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                text.push_str(&String::from_utf8_lossy(&pending[..valid]));
                if let Some(count) = error.error_len() {
                    text.push('\u{fffd}');
                    pending.drain(..valid + count);
                } else {
                    pending.drain(..valid);
                    break;
                }
            }
        }
    }
    text
}
fn attach_command_with_layout(kind: &str, name: &str, cwd: &str, layout: &Value) -> String {
    if kind != "tmux" {
        return attach_command(kind, name, cwd);
    }
    let q = crate::runtime::quote;
    let target = q(name);
    let dir = q(cwd);
    let mut commands = vec![
        format!("new-session -d -s {target} -c {dir}"),
        format!("set-option -t {target} destroy-unattached off"),
        format!("set-option -t {target} window-size latest"),
        format!("set-option -t {target} status off"),
    ];
    let layout_type = layout["layoutType"].as_str().unwrap_or("single");
    let splits: Vec<(&str, &str)> = match layout_type {
        "horizontal_2" => vec![("-v", "0")],
        "vertical_2" => vec![("-h", "0")],
        "left_right_tb" => vec![("-h", "0"), ("-v", "1")],
        "left_tb_right" => vec![("-h", "0"), ("-v", "0")],
        "quad" => vec![("-h", "0"), ("-v", "0"), ("-v", "2")],
        _ => vec![],
    };
    for (direction, pane) in splits {
        commands.push(format!(
            "split-window {direction} -t {} -c {dir}",
            q(&format!("{name}:0.{pane}"))
        ));
    }
    if let Some(panes) = layout["panes"].as_array() {
        for pane in panes {
            if let (Some(position), Some(command)) =
                (pane["position"].as_u64(), pane["command"].as_str())
            {
                if !command.trim().is_empty() {
                    commands.push(format!(
                        "send-keys -t {} -- {} Enter",
                        q(&format!("{name}:0.{position}")),
                        q(command)
                    ));
                }
            }
        }
    }
    commands.push(format!("select-pane -t {}", q(&format!("{name}:0.0"))));
    let bootstrap = format!("tmux {}", commands.join(" \\; "));
    let clipboard_check = r#"kanvibe_clipboard_preference() { for kanvibe_config in /etc/tmux.conf /usr/local/etc/tmux.conf /opt/homebrew/etc/tmux.conf "$HOME/.tmux.conf" "${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf"; do if [ -e "$kanvibe_config" ]; then [ -r "$kanvibe_config" ] || return 2; grep -qE '^[[:space:]]*set(-option)?[[:space:]].*set-clipboard' "$kanvibe_config"; kanvibe_result=$?; [ "$kanvibe_result" -eq 0 ] && return 0; [ "$kanvibe_result" -gt 1 ] && return 2; fi; done; return 1; }; kanvibe_clipboard_preference"#;
    format!("if ! tmux has-session -t {target} 2>/dev/null; then {bootstrap} || tmux -f /dev/null {}; if {clipboard_check}; then :; elif [ $? -eq 1 ]; then tmux set-option -s set-clipboard on; fi; fi; tmux set-option -t {target} status off; exec tmux attach-session -t {target}",commands.join(" \\; "))
}
#[allow(clippy::too_many_arguments)]
fn spawn(
    app: &AppHandle,
    session_key: &str,
    client_id: &str,
    cmd: CommandBuilder,
    cols: u16,
    rows: u16,
    task_id: &str,
    tab_id: Option<&str>,
    plain: bool,
    login_root: Option<String>,
) -> Result<Arc<Mutex<Session>>, String> {
    let pair = native_pty_system().openpty(size(cols, rows)).map_err(err)?;
    let child = pair.slave.spawn_command(cmd).map_err(err)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(err)?;
    let writer = pair.master.take_writer().map_err(err)?;
    let entry = Arc::new(Mutex::new(Session {
        master: pair.master,
        writer,
        child,
        clients: HashMap::from([(client_id.into(), 1)]),
        mobile: HashMap::new(),
        screen: vt100::Parser::new(rows.max(1), cols.max(1), 10000),
        task_id: task_id.into(),
        tab_id: tab_id.map(String::from),
        plain,
        login_root,
        desktop_size: if client_id.starts_with("mobile:") {
            None
        } else {
            Some((cols, rows))
        },
        mobile_size: if client_id.starts_with("mobile:") {
            Some((cols, rows))
        } else {
            None
        },
    }));
    let weak = Arc::downgrade(&entry);
    let app = app.clone();
    let session_key = session_key.to_owned();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16384];
        let mut pending_utf8 = Vec::new();
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let Some(entry) = weak.upgrade() else { break };
            let Ok(mut s) = entry.lock() else { break };
            s.screen.process(&buffer[..count]);
            pending_utf8.extend_from_slice(&buffer[..count]);
            let text = drain_utf8(&mut pending_utf8);
            let (event, payload) = if let Some(root) = &s.login_root {
                (
                    "kanvibe:ai-login-data",
                    json!({"accountRoot":root,"data":text}),
                )
            } else {
                (
                    "kanvibe:terminal-data",
                    json!({"taskId":s.task_id,"tabId":s.tab_id,"data":text}),
                )
            };
            for label in s.clients.keys() {
                let _ = app.emit_to(label, event, &payload);
            }
            let callbacks: Vec<_> = s.mobile.values().cloned().collect();
            drop(s);
            for callback in callbacks {
                callback(buffer[..count].to_vec());
            }
        }
        if let Some(entry) = weak.upgrade() {
            if let Ok(mut s) = entry.lock() {
                let exit = s.child.wait().map(|v| v.exit_code()).unwrap_or(1);
                for label in s.clients.keys() {
                    if let Some(root) = &s.login_root {
                        let _ = app.emit_to(
                            label,
                            "kanvibe:ai-login-exit",
                            json!({"accountRoot":root,"exitCode":exit}),
                        );
                    } else {
                        let _ = app.emit_to(
                            label,
                            "kanvibe:terminal-close",
                            json!({"taskId":s.task_id,"tabId":s.tab_id,"reason":null}),
                        );
                    }
                }
                let task_id = s.task_id.clone();
                let tab_id = s.tab_id.clone();
                drop(s);
                if let Some(id) = tab_id {
                    remove_tab(&app, &task_id, &id);
                }
            }
            let state = app.state::<TerminalState>();
            if let Ok(mut sessions) = state.sessions.lock() {
                if sessions
                    .get(&session_key)
                    .is_some_and(|v| Arc::ptr_eq(v, &entry))
                {
                    sessions.remove(&session_key);
                }
            };
        }
    });
    Ok(entry)
}
fn session(app: &AppHandle, id: &str, tab: Option<&str>) -> Result<Arc<Mutex<Session>>, String> {
    app.state::<TerminalState>()
        .sessions
        .lock()
        .map_err(err)?
        .get(&key(id, tab))
        .cloned()
        .ok_or_else(|| "터미널 세션을 찾을 수 없습니다.".into())
}
pub fn write(app: &AppHandle, id: &str, tab: Option<&str>, data: &str) -> Result<(), String> {
    let entry = session(app, id, tab)?;
    let mut s = entry.lock().map_err(err)?;
    s.writer.write_all(data.as_bytes()).map_err(err)?;
    s.writer.flush().map_err(err)
}
fn remember_size(s: &mut Session, client: &str, cols: u16, rows: u16) -> Result<(), String> {
    if client.starts_with("mobile:") {
        s.mobile_size = Some((cols, rows));
    } else {
        s.desktop_size = Some((cols, rows));
    }
    let chosen = s.mobile_size.or(s.desktop_size).unwrap_or((cols, rows));
    s.master.resize(size(chosen.0, chosen.1)).map_err(err)?;
    s.screen
        .screen_mut()
        .set_size(chosen.1.max(1), chosen.0.max(1));
    Ok(())
}
pub fn release_mobile_viewport(
    app: &AppHandle,
    id: &str,
    tab: Option<&str>,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let entry = session(app, id, tab)?;
    let mut s = entry.lock().map_err(err)?;
    s.mobile_size = None;
    let (cols, rows) = s.desktop_size.unwrap_or((cols, rows));
    s.master.resize(size(cols, rows)).map_err(err)?;
    s.screen.screen_mut().set_size(rows.max(1), cols.max(1));
    Ok(())
}
pub fn resize_client(
    app: &AppHandle,
    id: &str,
    tab: Option<&str>,
    client: &str,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let entry = session(app, id, tab)?;
    let mut s = entry.lock().map_err(err)?;
    remember_size(&mut s, client, cols, rows)
}
pub fn resize(
    app: &AppHandle,
    id: &str,
    tab: Option<&str>,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let entry = session(app, id, tab)?;
    let mut s = entry.lock().map_err(err)?;
    s.master.resize(size(cols, rows)).map_err(err)?;
    s.screen.screen_mut().set_size(rows.max(1), cols.max(1));
    Ok(())
}
pub fn detach(app: &AppHandle, client_id: &str, id: &str, tab: Option<&str>) -> Result<(), String> {
    let entry = session(app, id, tab)?;
    let mut s = entry.lock().map_err(err)?;
    if let Some(count) = s.clients.get_mut(client_id) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            s.clients.remove(client_id);
        }
    }
    if !s.clients.keys().any(|c| !c.starts_with("mobile:")) {
        s.desktop_size = None;
        if let Some((cols, rows)) = s.mobile_size {
            remember_size(&mut s, "mobile:remaining", cols, rows)?;
        }
    }
    let destroy = !s.plain && s.clients.is_empty() && s.mobile.is_empty();
    drop(s);
    if destroy {
        destroy_session(app, &key(id, tab));
    }
    Ok(())
}
fn destroy_session(app: &AppHandle, k: &str) {
    let state = app.state::<TerminalState>();
    let removed = state.sessions.lock().ok().and_then(|mut map| map.remove(k));
    if let Some(entry) = removed {
        if let Ok(mut s) = entry.lock() {
            let _ = s.child.kill();
        }
    }
}
pub fn close_client(app: &AppHandle, label: &str) {
    let state = app.state::<TerminalState>();
    state.pending_opens.cancel_client(label);
    let entries: Vec<_> = state
        .sessions
        .lock()
        .map(|m| m.iter().map(|(k, s)| (k.clone(), s.clone())).collect())
        .unwrap_or_default();
    for (k, s) in entries {
        if let Ok(mut s) = s.lock() {
            s.clients.remove(label);
            if s.login_root.is_some() || (!s.plain && s.clients.is_empty() && s.mobile.is_empty()) {
                let _ = s.child.kill();
                drop(s);
                destroy_session(app, &k);
            }
        }
    }
}
pub fn kill_task(app: &AppHandle, id: &str) -> Result<(), String> {
    let keys: Vec<_> = app
        .state::<TerminalState>()
        .sessions
        .lock()
        .map_err(err)?
        .iter()
        .filter_map(|(k, s)| s.lock().ok().filter(|v| v.task_id == id).map(|_| k.clone()))
        .collect();
    for k in keys {
        destroy_session(app, &k);
    }
    app.state::<TerminalState>()
        .tabs
        .lock()
        .map_err(err)?
        .remove(id);
    Ok(())
}
pub fn shutdown(app: &AppHandle) {
    let state = app.state::<TerminalState>();
    let entries = state
        .sessions
        .lock()
        .map(|mut m| m.drain().map(|(_, v)| v).collect::<Vec<_>>())
        .unwrap_or_default();
    for entry in entries {
        if let Ok(mut s) = entry.lock() {
            let _ = s.child.kill();
        }
    }
}
pub fn subscribe(
    app: &AppHandle,
    id: &str,
    tab: Option<&str>,
    client_id: &str,
    callback: OutputListener,
) -> Result<Vec<u8>, String> {
    let entry = session(app, id, tab)?;
    let mut s = entry.lock().map_err(err)?;
    let snapshot = snapshot(&s.screen);
    s.mobile.insert(client_id.into(), callback);
    Ok(snapshot)
}
pub fn unsubscribe(app: &AppHandle, id: &str, tab: Option<&str>, client_id: &str) {
    if let Ok(entry) = session(app, id, tab) {
        if let Ok(mut s) = entry.lock() {
            s.mobile.remove(client_id);
        }
    }
}
pub fn dimensions(app: &AppHandle, id: &str, tab: Option<&str>) -> Result<(u16, u16), String> {
    let entry = session(app, id, tab)?;
    let s = entry.lock().map_err(err)?;
    let p = s.master.get_size().map_err(err)?;
    Ok((p.cols, p.rows))
}

pub fn open_login(
    app: &AppHandle,
    label: &str,
    provider: &str,
    root: &str,
    cols: u16,
    rows: u16,
) -> Result<Value, String> {
    let (command, args, env_name, default_root) = match provider {
        "claude" => (
            "claude",
            vec!["auth", "login", "--claudeai"],
            "CLAUDE_CONFIG_DIR",
            Path::new(&crate::runtime::home()).join(".claude"),
        ),
        "codex" => (
            "codex",
            vec!["login"],
            "CODEX_HOME",
            Path::new(&crate::runtime::home()).join(".codex"),
        ),
        "gemini" => (
            "gemini",
            vec![],
            "GEMINI_CLI_HOME",
            Path::new(&crate::runtime::home()).to_path_buf(),
        ),
        _ => return Err("지원하지 않는 AI provider입니다.".into()),
    };
    let login_key = format!("login:{label}:{root}");
    let state = app.state::<TerminalState>();
    let mut sessions = state.sessions.lock().map_err(err)?;
    if sessions.contains_key(&login_key) {
        return Ok(json!({"ok":true}));
    }
    let mut cmd = CommandBuilder::new(command);
    cmd.args(args);
    cmd.cwd(crate::runtime::home());
    configure_environment(&mut cmd);
    if Path::new(root) != default_root {
        cmd.env(env_name, root);
    } else {
        cmd.env_remove(env_name);
    }
    let entry = spawn(
        app,
        &login_key,
        label,
        cmd,
        cols,
        rows,
        &login_key,
        None,
        false,
        Some(root.into()),
    )?;
    sessions.insert(login_key, entry);
    Ok(json!({"ok":true}))
}
pub fn login_action(
    app: &AppHandle,
    label: &str,
    root: &str,
    action: &str,
    data: &str,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let k = format!("login:{label}:{root}");
    match action {
        "write" => write(app, &k, None, data),
        "resize" => resize(app, &k, None, cols, rows),
        "close" => {
            destroy_session(app, &k);
            Ok(())
        }
        _ => Err("잘못된 로그인 동작".into()),
    }
}

fn shell_name() -> String {
    Path::new(&std::env::var("SHELL").unwrap_or("/bin/sh".into()))
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
fn with_tabs<T>(
    app: &AppHandle,
    id: &str,
    f: impl FnOnce(&mut TabState) -> T,
) -> Result<T, String> {
    let state = app.state::<TerminalState>();
    let mut tabs = state.tabs.lock().map_err(err)?;
    let t = tabs.entry(id.into()).or_insert_with(|| TabState {
        entries: vec![(format!("{id}-1"), shell_name())],
        active: format!("{id}-1"),
        next: 2,
    });
    Ok(f(t))
}
fn remove_tab(app: &AppHandle, id: &str, tab: &str) {
    let state = app.state::<TerminalState>();
    if let Ok(mut tabs) = state.tabs.lock() {
        if let Some(t) = tabs.get_mut(id) {
            if let Some(index) = t.entries.iter().position(|v| v.0 == tab) {
                t.entries.remove(index);
                if t.active == tab {
                    t.active = t
                        .entries
                        .get(index.min(t.entries.len().saturating_sub(1)))
                        .map(|v| v.0.clone())
                        .unwrap_or_default();
                }
            }
        }
    };
}
pub fn tabs_dispatch(app: &AppHandle, method: &str, args: Vec<Value>) -> Result<Value, String> {
    let id = args
        .first()
        .and_then(Value::as_str)
        .ok_or("taskId가 필요합니다.")?;
    let target = task(app, id)?;
    let Some(kind) = field(&target, "sessionType") else {
        return Ok(json!({"ok":false,"tabs":[],"error":"작업에 연결된 세션이 없습니다."}));
    };
    if kind != "terminal" {
        return multiplex_tabs(app, method, &args, &target);
    }
    let tab = args.get(1).and_then(Value::as_str).unwrap_or("");
    match method {
        "listTerminalTabs" => with_tabs(
            app,
            id,
            |t| json!({"ok":true,"tabs":t.entries.iter().enumerate().map(|(i,(id,name))|json!({"id":id,"nativeIndex":i,"name":name,"isActive":t.active==*id})).collect::<Vec<_>>()}),
        ),
        "createTerminalTab" => with_tabs(app, id, |t| {
            let next = format!("{id}-{}", t.next);
            t.next += 1;
            t.active = next.clone();
            t.entries.push((next, shell_name()));
            json!({"ok":true})
        }),
        "selectTerminalTab" => with_tabs(app, id, |t| {
            if t.entries.iter().any(|v| v.0 == tab) {
                t.active = tab.into();
            }
            json!({"ok":true})
        }),
        "renameTerminalTab" => with_tabs(app, id, |t| {
            if let Some(e) = t.entries.iter_mut().find(|v| v.0 == tab) {
                e.1 = args.get(2).and_then(Value::as_str).unwrap_or("").into();
            }
            json!({"ok":true})
        }),
        "moveTerminalTab" => with_tabs(app, id, |t| {
            let pos = args.get(2).and_then(Value::as_u64).unwrap_or(u64::MAX) as usize;
            if let Some(old) = t.entries.iter().position(|v| v.0 == tab) {
                if pos < t.entries.len() {
                    let e = t.entries.remove(old);
                    t.entries.insert(pos, e);
                }
            }
            json!({"ok":true})
        }),
        "closeTerminalTab" => {
            destroy_session(app, &key(id, Some(tab)));
            remove_tab(app, id, tab);
            with_tabs(
                app,
                id,
                |t| json!({"ok":true,"shouldCloseWindow":t.entries.is_empty()}),
            )
        }
        _ => Err(format!("알 수 없는 terminalTabs 메서드: {method}")),
    }
}
fn multiplex_tabs(
    _app: &AppHandle,
    method: &str,
    args: &[Value],
    target: &Value,
) -> Result<Value, String> {
    let kind = field(target, "sessionType").unwrap_or("");
    let name = field(target, "sessionName").ok_or("세션이 없습니다.")?;
    let host = field(target, "sshHost");
    let q = crate::runtime::quote;
    if kind == "herdr" {
        return Ok(crate::herdr::tabs_dispatch(
            method,
            args,
            name,
            field(target, "worktreePath"),
            host,
            &crate::herdr::system_shell,
        ));
    }
    let tab = args.get(1).and_then(Value::as_str).unwrap_or("");
    let listing = if kind == "tmux" {
        format!("tmux list-windows -t {} -F '#{{window_id}}\t#{{window_index}}\t#{{window_active}}\t#{{window_name}}'",q(name))
    } else {
        format!("zellij --session {} action list-tabs --json", q(name))
    };
    let mut output = crate::runtime::run_shell(&listing, host);
    let mut supports_ids = true;
    let tabs = if kind == "tmux" {
        output.as_ref().map(|s| parse_tmux(s)).unwrap_or_default()
    } else {
        if let Ok(output) = &output {
            parse_zellij(output)
        } else {
            supports_ids = false;
            output = crate::runtime::run_shell(
                &format!("zellij --session {} action query-tab-names", q(name)),
                host,
            );
            let dump = crate::runtime::run_shell(
                &format!("zellij --session {} action dump-layout", q(name)),
                host,
            )
            .unwrap_or_default();
            output
                .as_ref()
                .map(|s| parse_zellij_legacy(s, &dump))
                .unwrap_or_default()
        }
    };
    if method == "listTerminalTabs" {
        return match output {
            Ok(_) => Ok(json!({"ok":true,"tabs":tabs})),
            Err(e) => Ok(json!({"ok":false,"tabs":[],"error":e})),
        };
    }
    if method == "closeTerminalTab" && tabs.len() <= 1 {
        return Ok(json!({"ok":true,"shouldCloseWindow":true}));
    }
    let cwd = field(target, "worktreePath")
        .map(|v| format!(" -c {}", q(v)))
        .unwrap_or_default();
    let command = if kind == "tmux" {
        match method {
            "createTerminalTab" => format!("tmux new-window -t {}:{cwd}", q(name)),
            "selectTerminalTab" => format!("tmux select-window -t {}", q(tab)),
            "closeTerminalTab" => format!("tmux kill-window -t {}", q(tab)),
            "renameTerminalTab" => format!(
                "tmux rename-window -t {} -- {}",
                q(tab),
                q(args.get(2).and_then(Value::as_str).unwrap_or(""))
            ),
            "moveTerminalTab" => {
                let index = args
                    .get(2)
                    .and_then(Value::as_u64)
                    .ok_or("탭 위치가 필요합니다.")? as usize;
                let old = tabs
                    .iter()
                    .position(|v| v["id"] == tab)
                    .ok_or("탭이 없습니다.")?;
                let destination = tabs.get(index).ok_or("잘못된 탭 위치")?;
                format!(
                    "tmux move-window {} -s {} -t {}; tmux move-window -r -t {}",
                    if old < index { "-a" } else { "-b" },
                    q(tab),
                    q(destination["id"].as_str().unwrap_or("")),
                    q(name)
                )
            }
            _ => return Err(format!("알 수 없는 탭 메서드: {method}")),
        }
    } else {
        let prefix = format!("zellij --session {} action", q(name));
        match method {
            "createTerminalTab" => format!(
                "{prefix} new-tab{}",
                field(target, "worktreePath")
                    .map(|v| format!(" --cwd {}", q(v)))
                    .unwrap_or_default()
            ),
            "selectTerminalTab" => {
                if supports_ids {
                    format!("{prefix} go-to-tab-by-id {}", q(tab))
                } else {
                    format!(
                        "{prefix} go-to-tab-name {}",
                        q(tabs
                            .iter()
                            .find(|v| v["id"] == tab)
                            .and_then(|v| v["name"].as_str())
                            .ok_or("탭을 찾을 수 없습니다.")?)
                    )
                }
            }
            "closeTerminalTab" => {
                if supports_ids {
                    format!("{prefix} close-tab-by-id {}", q(tab))
                } else {
                    format!(
                        "{prefix} go-to-tab-name {} && {prefix} close-tab",
                        q(tabs
                            .iter()
                            .find(|v| v["id"] == tab)
                            .and_then(|v| v["name"].as_str())
                            .ok_or("탭을 찾을 수 없습니다.")?)
                    )
                }
            }
            "renameTerminalTab" => {
                if supports_ids {
                    format!(
                        "{prefix} rename-tab-by-id {} {}",
                        q(tab),
                        q(args.get(2).and_then(Value::as_str).unwrap_or(""))
                    )
                } else {
                    format!(
                        "{prefix} go-to-tab-name {} && {prefix} rename-tab {}",
                        q(tabs
                            .iter()
                            .find(|v| v["id"] == tab)
                            .and_then(|v| v["name"].as_str())
                            .ok_or("탭을 찾을 수 없습니다.")?),
                        q(args.get(2).and_then(Value::as_str).unwrap_or(""))
                    )
                }
            }
            "moveTerminalTab" => {
                let index = args
                    .get(2)
                    .and_then(Value::as_u64)
                    .ok_or("탭 위치가 필요합니다.")? as usize;
                if index >= tabs.len() {
                    return Err("잘못된 탭 위치".into());
                }
                let old = tabs
                    .iter()
                    .position(|v| v["id"] == tab)
                    .ok_or("탭이 없습니다.")?;
                if old == index {
                    return Ok(json!({"ok":true}));
                }
                let direction = if old < index { "right" } else { "left" };
                let moves = (0..old.abs_diff(index))
                    .map(|_| {
                        if supports_ids {
                            format!("{prefix} move-tab {direction} -t {}", q(tab))
                        } else {
                            format!("{prefix} move-tab {direction}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" && ");
                if supports_ids {
                    moves
                } else {
                    format!(
                        "{prefix} go-to-tab-name {} && {moves}",
                        q(tabs[old]["name"].as_str().ok_or("탭 이름이 없습니다.")?)
                    )
                }
            }
            _ => return Err(format!("알 수 없는 탭 메서드: {method}")),
        }
    };
    match crate::runtime::run_shell(&command, host) {
        Ok(_) => Ok(json!({"ok":true})),
        Err(e) => Ok(json!({"ok":false,"error":e})),
    }
}
fn parse_tmux(output: &str) -> Vec<Value> {
    output.lines().filter_map(|line|{let p:Vec<_>=line.splitn(4,'\t').collect();if p.len()!=4{return None;}Some(json!({"id":p[0],"nativeIndex":p[1].parse::<u32>().ok()?,"isActive":p[2]=="1","name":p[3]}))}).collect()
}
fn parse_zellij_legacy(names: &str, layout: &str) -> Vec<Value> {
    let focus = regex::Regex::new(r#"(?m)^\s*tab\b([^\n]*)"#)
        .unwrap()
        .captures_iter(layout)
        .find_map(|c| {
            let attrs = &c[1];
            if !attrs.contains("focus=true") {
                return None;
            }
            regex::Regex::new(r#"name="((?:[^"\\]|\\.)*)""#)
                .unwrap()
                .captures(attrs)
                .and_then(|c| serde_json::from_str::<String>(&format!("\"{}\"", &c[1])).ok())
        });
    let names: Vec<_> = names
        .lines()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect();
    let active = focus
        .and_then(|f| names.iter().position(|v| *v == f))
        .unwrap_or(0);
    names.into_iter().enumerate().map(|(i,name)|json!({"id":i.to_string(),"nativeIndex":i,"name":name,"isActive":i==active})).collect()
}
fn parse_zellij(output: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(output).ok().and_then(|v|v.as_array().cloned().or_else(||v.get("tabs").and_then(Value::as_array).cloned())).unwrap_or_default().into_iter().map(|v|json!({"id":v["tab_id"].as_str().map(String::from).unwrap_or_else(||v["tab_id"].to_string()),"nativeIndex":v["position"],"name":v["name"],"isActive":v["is_focused"].as_bool().or_else(||v["active"].as_bool()).unwrap_or(false)})).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tmux_ids_and_sparse_indices_are_preserved() {
        let t = parse_tmux("@3\t1\t1\tClaude\n@7\t4\t0\tbuild\n");
        assert_eq!(t[1]["nativeIndex"], 4);
        assert_eq!(t[0]["id"], "@3");
        assert_eq!(t[0]["isActive"], true);
    }
    #[test]
    fn herdr_attach_execs_named_session_client_without_session_attach() {
        let command = attach_command("herdr", "project-task", "/work");
        assert!(command.starts_with("exec env -u HERDR_ENV"));
        assert!(command.contains("--session 'project-task'"));
        assert!(!command.contains("'session' 'attach'"));
        assert!(!command.contains("'session' 'stop'"));
    }
    #[test]
    fn window_closed_during_herdr_bootstrap_gets_no_pty_while_live_client_does() {
        for cancelled_first in [false, true] {
            let pending = PendingOpens::default();
            let (cancelled, live) = if cancelled_first {
                let cancelled = pending.begin("closed-window").unwrap();
                (cancelled, pending.begin("live-window").unwrap())
            } else {
                let live = pending.begin("live-window").unwrap();
                (pending.begin("closed-window").unwrap(), live)
            };
            pending.cancel_client("closed-window");
            assert!(!pending.finish(cancelled).unwrap());
            assert!(pending.finish(live).unwrap());
            assert!(pending.clients.lock().unwrap().is_empty());
        }
    }
    #[test]
    fn terminal_keys_do_not_collide() {
        assert_ne!(key("a", Some("b")), key("a", None));
    }
    #[test]
    fn commands_quote_user_paths() {
        assert!(attach_command("terminal", "n", "/tmp/a'b").contains("'\\''"));
    }
    #[test]
    fn utf8_chunks_preserve_multibyte_terminal_input() {
        let bytes = "한글🙂".as_bytes();
        let mut pending = Vec::new();
        let mut output = String::new();
        for b in bytes {
            pending.push(*b);
            output.push_str(&drain_utf8(&mut pending));
        }
        assert_eq!(output, "한글🙂");
        assert!(pending.is_empty());
    }
    #[test]
    fn zellij_uses_stable_id_and_legacy_focus() {
        let tabs = parse_zellij(r#"[{"tab_id":42,"position":0,"name":"Claude","active":true}]"#);
        assert_eq!(tabs[0]["id"], "42");
        let old = parse_zellij_legacy("one\ntwo\n", r#"tab name="two" focus=true {"#);
        assert_eq!(old[1]["isActive"], true);
    }
    #[test]
    fn pane_layout_retains_original_horizontal_direction() {
        let command = attach_command_with_layout(
            "tmux",
            "n",
            "/tmp",
            &json!({"layoutType":"horizontal_2","panes":[]}),
        );
        assert!(command.contains("split-window -v"));
        assert!(!command.contains("split-window -h"));
        let kdl = zellij_layout(
            "/tmp/a'b",
            &json!({"layoutType":"quad","panes":[{"position":2,"command":"echo \"a\""}]}),
        );
        assert!(kdl.contains("split_direction=\"vertical\""));
        assert!(kdl.contains("echo \\\"a\\\""));
    }
    #[test]
    fn plain_pty_shell_uses_clean_environment_and_supports_resize() {
        let pair = native_pty_system().openpty(size(80, 24)).unwrap();
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c","printf '%s|%s|%s|%s' \"${PORT-unset}\" \"${HOST-unset}\" \"${NODE_ENV-unset}\" \"$TERM\""]);
        configure_environment(&mut command);
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        pair.master.resize(size(100, 40)).unwrap();
        assert_eq!(pair.master.get_size().unwrap().cols, 100);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut output = Vec::new();
        let _ = reader.read_to_end(&mut output);
        assert!(child.wait().unwrap().success());
        assert!(String::from_utf8_lossy(&output).contains("unset|unset|unset|xterm-256color"));
    }
    #[test]
    fn snapshot_restores_scrollback_colors_modes_and_alternate_buffer() {
        let mut source = vt100::Parser::new(3, 20, 100);
        source.process(b"\x1b[31mred\r\nsecond\r\nthird\r\nfourth\r\nlast\x1b[?2004h");
        let mut restored = vt100::Parser::new(3, 20, 100);
        restored.process(&snapshot(&source));
        assert_eq!(restored.screen().contents(), source.screen().contents());
        assert_eq!(
            restored.screen().input_mode_formatted(),
            source.screen().input_mode_formatted()
        );
        restored.screen_mut().set_scrollback(100);
        source.screen_mut().set_scrollback(100);
        assert_eq!(restored.screen().contents(), source.screen().contents());
        assert_eq!(
            restored.screen().cell(0, 0).unwrap().fgcolor(),
            vt100::Color::Idx(1)
        );
        source.screen_mut().set_scrollback(0);
        source.process(b"\x1b[?1049hvim");
        let mut restored = vt100::Parser::new(3, 20, 100);
        restored.process(&snapshot(&source));
        assert!(restored.screen().alternate_screen());
        assert_eq!(restored.screen().contents(), source.screen().contents());
        restored.process(b"\x1b[?1049l");
        source.process(b"\x1b[?1049l");
        assert_eq!(restored.screen().contents(), source.screen().contents());
    }
    #[test]
    fn snapshot_preserves_wrapped_history_with_leading_blanks() {
        let mut source = vt100::Parser::new(3, 4, 100);
        source.process(b"abcd  gHijkl  opqrstuv");
        let mut restored = vt100::Parser::new(3, 4, 100);
        restored.process(&snapshot(&source));
        assert_eq!(restored.screen().contents(), source.screen().contents());
        restored.screen_mut().set_scrollback(100);
        source.screen_mut().set_scrollback(100);
        assert_eq!(restored.screen().contents(), source.screen().contents());
        assert_eq!(
            restored.screen().row_wrapped(0),
            source.screen().row_wrapped(0)
        );
    }
}
