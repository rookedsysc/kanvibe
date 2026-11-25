//! QA 제어 소켓. 재생 드라이버(`parity/tools/screens-rust/driver.py`)가 한 줄 JSON 명령을 보내고 한 줄 JSON 응답을 받는다.
//!
//! 명령: `navigate`, `click`, `hover`, `bounds`, `mouse_move`, `mouse_down`, `mouse_up`, `park_pointer`, `key`, `type`,
//! `wait_for`, `wait_idle`, `report_ready`, `fault`, `clear_fault`, `board_event`, `quit`.
//! 소켓 스레드는 명령을 GPUI 전경 스레드로 넘기고 응답을 기다리기만 한다. 화면 상태는 전경 스레드에서만 바뀐다.
//! 대상을 찾는 명령은 새 프레임이 그려진 뒤에 찾는다. 직전 단계가 바꾼 화면의 위치를 써야 옛 위치를 누르지 않는다.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc as async_mpsc;
use gpui_kit::{
    AnyWindowHandle, App, AsyncApp, Bounds, Global, Keystroke, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, Window, point, px,
};
use kanvibe_services::board_notifier::BoardEvent;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::qa::faults::ServiceFault;
use crate::qa::targets::{QaTargetRegistry, TargetQuery};
use crate::services::AppServices;
use crate::windows::WindowRegistry;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum ControlCommand {
    Navigate {
        route: String,
    },
    Click {
        target: TargetQuery,
        #[serde(default)]
        button: ClickButton,
        #[serde(default)]
        click_count: Option<usize>,
    },
    Hover {
        target: TargetQuery,
    },
    /// 대상의 창 좌표(논리 px). 드라이버가 끌기 같은 조합 동작을 만들 때 쓴다
    Bounds {
        target: TargetQuery,
    },
    /// 창 좌표로 포인터를 옮긴다. 누른 단추가 있으면 끌기로 전달된다
    MouseMove {
        x: f32,
        y: f32,
    },
    MouseDown,
    MouseUp,
    /// 포인터를 창 오른쪽 아래 바깥으로 보내 호버 상태를 지운다(TS 드라이버의 캡처 전 `mouse.move(w - 1, h - 1)`)
    ParkPointer,
    /// 보드 이벤트를 런타임 이벤트 버스로 보낸다(TS `kanvibe:board-event`)
    BoardEvent {
        event: Value,
    },
    /// `Mod+Shift+P` 표기. `Mod`는 이 플랫폼의 Control(macOS는 Command)이다
    Key {
        keys: String,
    },
    Type {
        text: String,
    },
    WaitFor {
        target: TargetQuery,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    WaitIdle,
    ReportReady,
    Fault {
        fault: ServiceFault,
    },
    ClearFault {
        target: String,
    },
    Quit,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClickButton {
    #[default]
    Left,
    Right,
}

struct ControlRequest {
    command: ControlCommand,
    reply: mpsc::Sender<Value>,
}

/// 드라이버가 움직인 포인터 위치와 누른 단추
#[derive(Default)]
struct QaPointer {
    position: Point<Pixels>,
    pressed_button: Option<MouseButton>,
}

impl Global for QaPointer {}

const DEFAULT_WAIT_FOR_TIMEOUT: Duration = Duration::from_millis(15_000);
const WAIT_FOR_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// 소켓을 열고 명령 처리 루프를 건다. 남은 소켓 파일은 지우고 다시 만든다.
pub fn start_control_server(socket_path: &Path, cx: &mut App) -> anyhow::Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    let (request_sender, mut request_receiver) = async_mpsc::unbounded::<ControlRequest>();
    std::thread::Builder::new()
        .name("kanvibe-qa-control".into())
        .spawn(move || accept_connections(listener, request_sender))?;
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(request) = request_receiver.next().await {
            cx.update(|cx| execute_request(request, cx));
        }
    })
    .detach();
    Ok(())
}

fn accept_connections(
    listener: UnixListener,
    request_sender: async_mpsc::UnboundedSender<ControlRequest>,
) {
    for stream in listener.incoming().flatten() {
        let request_sender = request_sender.clone();
        std::thread::spawn(move || serve_connection(stream, request_sender));
    }
}

fn serve_connection(
    stream: UnixStream,
    request_sender: async_mpsc::UnboundedSender<ControlRequest>,
) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let response = match serde_json::from_str::<ControlCommand>(&line) {
            Ok(command) => forward_command(command, &request_sender),
            Err(error) => json!({"ok": false, "error": format!("invalid command: {error}")}),
        };
        if writeln!(writer, "{response}").is_err() {
            return;
        }
    }
}

fn forward_command(
    command: ControlCommand,
    request_sender: &async_mpsc::UnboundedSender<ControlRequest>,
) -> Value {
    let (reply, reply_receiver) = mpsc::channel();
    if request_sender
        .unbounded_send(ControlRequest { command, reply })
        .is_err()
    {
        return json!({"ok": false, "error": "app is shutting down"});
    }
    reply_receiver
        .recv()
        .unwrap_or_else(|_| json!({"ok": false, "error": "command dropped"}))
}

fn ok() -> Value {
    json!({"ok": true})
}

fn failure(message: impl Into<String>) -> Value {
    json!({"ok": false, "error": message.into()})
}

fn main_window(cx: &App) -> Option<AnyWindowHandle> {
    cx.active_window().or_else(|| cx.windows().first().copied())
}

fn execute_request(request: ControlRequest, cx: &mut App) {
    let ControlRequest { command, reply } = request;
    let Some(window_handle) = main_window(cx) else {
        let _ = reply.send(failure("no window"));
        return;
    };
    match command {
        ControlCommand::WaitFor { target, timeout_ms } => {
            let timeout = timeout_ms
                .map(Duration::from_millis)
                .unwrap_or(DEFAULT_WAIT_FOR_TIMEOUT);
            spawn_wait_for(window_handle, target, timeout, reply, cx);
        }
        ControlCommand::WaitIdle | ControlCommand::ReportReady => {
            wait_two_frames(window_handle, reply, cx);
        }
        command @ (ControlCommand::Click { .. }
        | ControlCommand::Hover { .. }
        | ControlCommand::Bounds { .. }) => {
            after_next_frame(window_handle, reply, cx, move |cx| {
                execute_immediate(command, window_handle, cx)
            });
        }
        command => {
            let response = execute_immediate(command, window_handle, cx);
            let _ = reply.send(response);
        }
    }
}

fn execute_immediate(
    command: ControlCommand,
    window_handle: AnyWindowHandle,
    cx: &mut App,
) -> Value {
    match command {
        ControlCommand::Navigate { route } => navigate_window(window_handle, &route, cx),
        ControlCommand::Click {
            target,
            button,
            click_count,
        } => with_target(window_handle, &target, cx, |bounds, window, cx| {
            click_at(bounds, button, click_count.unwrap_or(1), window, cx);
        }),
        ControlCommand::Hover { target } => {
            with_target(window_handle, &target, cx, |bounds, window, cx| {
                move_mouse(bounds.center(), window, cx);
            })
        }
        ControlCommand::Bounds { target } => match find_target(window_handle, &target, cx) {
            Some(bounds) => json!({"ok": true, "bounds": {
                "x": f32::from(bounds.origin.x), "y": f32::from(bounds.origin.y),
                "width": f32::from(bounds.size.width), "height": f32::from(bounds.size.height),
            }}),
            None => failure(format!("target not found: {target:?}")),
        },
        ControlCommand::MouseMove { x, y } => move_pointer(window_handle, point(px(x), px(y)), cx),
        ControlCommand::MouseDown => press_pointer(window_handle, true, cx),
        ControlCommand::MouseUp => press_pointer(window_handle, false, cx),
        ControlCommand::ParkPointer => park_pointer(window_handle, cx),
        ControlCommand::BoardEvent { event } => emit_board_event(event, cx),
        ControlCommand::Key { keys } => dispatch_key(window_handle, &keys, cx),
        ControlCommand::Type { text } => type_text(window_handle, &text, cx),
        ControlCommand::Fault { fault } => update_faults(cx, |faults| faults.add(fault)),
        ControlCommand::ClearFault { target } => update_faults(cx, |faults| faults.clear(&target)),
        ControlCommand::Quit => {
            cx.quit();
            ok()
        }
        ControlCommand::WaitFor { .. } | ControlCommand::WaitIdle | ControlCommand::ReportReady => {
            failure("waiting commands are handled asynchronously")
        }
    }
}

/// 다음 프레임이 그려진 뒤 `act`를 부르고 그 결과로 답한다
fn after_next_frame(
    window_handle: AnyWindowHandle,
    reply: mpsc::Sender<Value>,
    cx: &mut App,
    act: impl FnOnce(&mut App) -> Value + 'static,
) {
    let result = window_handle.update(cx, |_, window, _| {
        window.refresh();
        // 프레임 콜백은 창 update 안에서 불리므로, 같은 창을 다시 update하는 동작은 그 update가 끝난 뒤로 미룬다
        window.on_next_frame(move |_, cx| {
            cx.defer(move |cx| {
                let _ = reply.send(act(cx));
            });
        });
    });
    if let Err(error) = result {
        eprintln!("[kanvibe-qa] frame wait failed: {error:#}");
    }
}

fn move_pointer(window_handle: AnyWindowHandle, position: Point<Pixels>, cx: &mut App) -> Value {
    let pointer = cx.default_global::<QaPointer>();
    pointer.position = position;
    let pressed_button = pointer.pressed_button;
    let event = PlatformInput::MouseMove(MouseMoveEvent {
        position,
        pressed_button,
        modifiers: Modifiers::default(),
    });
    dispatch_input(window_handle, event, cx)
}

fn press_pointer(window_handle: AnyWindowHandle, is_down: bool, cx: &mut App) -> Value {
    let pointer = cx.default_global::<QaPointer>();
    pointer.pressed_button = is_down.then_some(MouseButton::Left);
    let position = pointer.position;
    let modifiers = Modifiers::default();
    let event = if is_down {
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        })
    } else {
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
        })
    };
    dispatch_input(window_handle, event, cx)
}

/// 창 내용 크기 바로 바깥. 어떤 요소의 영역에도 들지 않아 호버가 풀린다
fn park_pointer(window_handle: AnyWindowHandle, cx: &mut App) -> Value {
    let viewport = window_handle.update(cx, |_, window, _| window.viewport_size());
    match viewport {
        Ok(viewport) => move_pointer(window_handle, point(viewport.width, viewport.height), cx),
        Err(error) => failure(error.to_string()),
    }
}

fn dispatch_input(window_handle: AnyWindowHandle, event: PlatformInput, cx: &mut App) -> Value {
    match window_handle.update(cx, |_, window, cx| window.dispatch_event(event, cx)) {
        Ok(_) => ok(),
        Err(error) => failure(error.to_string()),
    }
}

fn emit_board_event(event: Value, cx: &mut App) -> Value {
    let board_event = match serde_json::from_value::<BoardEvent>(event) {
        Ok(board_event) => board_event,
        Err(error) => return failure(format!("invalid board event: {error}")),
    };
    match cx.global::<AppServices>().runtime() {
        Some(runtime) => {
            runtime.board_notifier().emit(board_event);
            ok()
        }
        None => failure("board events need --data-dir"),
    }
}

fn navigate_window(window_handle: AnyWindowHandle, route: &str, cx: &mut App) -> Value {
    let view = cx
        .default_global::<WindowRegistry>()
        .view_for(window_handle.window_id())
        .and_then(|view| view.upgrade());
    match view {
        Some(view) => {
            view.update(cx, |app_window, cx| app_window.navigate(route, cx));
            ok()
        }
        None => failure("window has no app view"),
    }
}

fn update_faults(cx: &mut App, change: impl FnOnce(&crate::qa::faults::FaultRegistry)) -> Value {
    match cx.global::<AppServices>().faults() {
        Some(faults) => {
            change(faults);
            ok()
        }
        None => failure("fault injection is only available in QA mode"),
    }
}

fn find_target(
    window_handle: AnyWindowHandle,
    target: &TargetQuery,
    cx: &App,
) -> Option<Bounds<Pixels>> {
    cx.try_global::<QaTargetRegistry>()?
        .find(window_handle.window_id(), target)
}

fn with_target(
    window_handle: AnyWindowHandle,
    target: &TargetQuery,
    cx: &mut App,
    act: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App),
) -> Value {
    let Some(bounds) = find_target(window_handle, target, cx) else {
        return failure(format!("target not found: {target:?}"));
    };
    match window_handle.update(cx, |_, window, cx| act(bounds, window, cx)) {
        Ok(()) => ok(),
        Err(error) => failure(error.to_string()),
    }
}

fn move_mouse(position: gpui_kit::Point<Pixels>, window: &mut Window, cx: &mut App) {
    cx.default_global::<QaPointer>().position = position;
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn click_at(
    bounds: Bounds<Pixels>,
    button: ClickButton,
    click_count: usize,
    window: &mut Window,
    cx: &mut App,
) {
    let position = bounds.center();
    let button = match button {
        ClickButton::Left => MouseButton::Left,
        ClickButton::Right => MouseButton::Right,
    };
    move_mouse(position, window, cx);
    for click in 1..=click_count {
        let modifiers = Modifiers::default();
        let down = MouseDownEvent {
            button,
            position,
            modifiers,
            click_count: click,
            first_mouse: false,
        };
        window.dispatch_event(PlatformInput::MouseDown(down), cx);
        let up = MouseUpEvent {
            button,
            position,
            modifiers,
            click_count: click,
        };
        window.dispatch_event(PlatformInput::MouseUp(up), cx);
    }
}

/// `Mod+Shift+P` → GPUI `ctrl-shift-p`
pub fn keystroke_text_from_shortcut(keys: &str, is_mac: bool) -> String {
    keys.split('+')
        .map(|part| match part {
            "Mod" if is_mac => "cmd".to_string(),
            "Mod" | "Control" | "Ctrl" => "ctrl".to_string(),
            "Meta" | "Cmd" => "cmd".to_string(),
            "Alt" => "alt".to_string(),
            "Shift" => "shift".to_string(),
            "Escape" => "escape".to_string(),
            "Enter" => "enter".to_string(),
            "ArrowUp" => "up".to_string(),
            "ArrowDown" => "down".to_string(),
            "ArrowLeft" => "left".to_string(),
            "ArrowRight" => "right".to_string(),
            other => other.to_lowercase(),
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn dispatch_key(window_handle: AnyWindowHandle, keys: &str, cx: &mut App) -> Value {
    let is_mac = cfg!(target_os = "macos");
    let keystroke_text = keystroke_text_from_shortcut(keys, is_mac);
    let Ok(keystroke) = Keystroke::parse_with_os(&keystroke_text, cx.platform_info().os) else {
        return failure(format!("cannot parse keys: {keys}"));
    };
    match window_handle.update(cx, |_, window, cx| window.dispatch_keystroke(keystroke, cx)) {
        Ok(_) => ok(),
        Err(error) => failure(error.to_string()),
    }
}

fn type_text(window_handle: AnyWindowHandle, text: &str, cx: &mut App) -> Value {
    let result = window_handle.update(cx, |_, window, cx| {
        for character in text.chars() {
            let keystroke = Keystroke {
                modifiers: Modifiers::default(),
                key: character.to_lowercase().to_string(),
                key_char: Some(character.to_string()),
            };
            window.dispatch_keystroke(keystroke, cx);
        }
    });
    match result {
        Ok(()) => ok(),
        Err(error) => failure(error.to_string()),
    }
}

/// 화면 변경이 반영된 프레임이 그려진 뒤 응답한다(TS 드라이버의 rAF 두 번과 같은 뜻)
fn wait_two_frames(window_handle: AnyWindowHandle, reply: mpsc::Sender<Value>, cx: &mut App) {
    let result = window_handle.update(cx, |_, window, _| {
        window.refresh();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |_, _| {
                let _ = reply.send(ok());
            });
        });
    });
    if let Err(error) = result {
        eprintln!("[kanvibe-qa] wait_idle failed: {error:#}");
    }
}

fn spawn_wait_for(
    window_handle: AnyWindowHandle,
    target: TargetQuery,
    timeout: Duration,
    reply: mpsc::Sender<Value>,
    cx: &mut App,
) {
    let started_at = Instant::now();
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            let is_found = cx.update(|cx| find_target(window_handle, &target, cx).is_some());
            if is_found {
                let _ = reply.send(ok());
                return;
            }
            if started_at.elapsed() >= timeout {
                let _ = reply.send(failure(format!("timed out waiting for {target:?}")));
                return;
            }
            cx.background_executor().timer(WAIT_FOR_POLL_INTERVAL).await;
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_notation_becomes_gpui_keystroke_text() {
        assert_eq!(
            keystroke_text_from_shortcut("Mod+Shift+P", false),
            "ctrl-shift-p"
        );
        assert_eq!(
            keystroke_text_from_shortcut("Mod+Shift+P", true),
            "cmd-shift-p"
        );
        assert_eq!(keystroke_text_from_shortcut("ArrowRight", false), "right");
    }

    #[test]
    fn commands_parse_from_driver_json_lines() {
        let command: ControlCommand = serde_json::from_str(
            r#"{"cmd":"click","target":{"role":"button","name":"다시 시도"}}"#,
        )
        .unwrap();
        assert!(matches!(command, ControlCommand::Click { .. }));
        let command: ControlCommand = serde_json::from_str(
            r#"{"cmd":"fault","fault":{"target":"kanban.getTasksByStatus","mode":"hang"}}"#,
        )
        .unwrap();
        assert!(matches!(command, ControlCommand::Fault { .. }));
    }

    #[test]
    fn pointer_and_board_event_commands_parse_from_driver_json_lines() {
        let command: ControlCommand =
            serde_json::from_str(r#"{"cmd":"mouse_move","x":288,"y":500}"#).unwrap();
        assert!(matches!(command, ControlCommand::MouseMove { x, .. } if x == 288.0));
        let command: ControlCommand = serde_json::from_str(r#"{"cmd":"mouse_down"}"#).unwrap();
        assert!(matches!(command, ControlCommand::MouseDown));
        let command: ControlCommand = serde_json::from_str(
            r#"{"cmd":"board_event","event":{"type":"task-peer-deletion-failed","taskId":"t1","taskTitle":"x","failedPeerUrls":["http://192.0.2.10:9736"]}}"#,
        )
        .unwrap();
        let ControlCommand::BoardEvent { event } = command else {
            panic!("board_event command expected");
        };
        assert!(matches!(
            serde_json::from_value::<BoardEvent>(event),
            Ok(BoardEvent::TaskPeerDeletionFailed(_))
        ));
    }
}
