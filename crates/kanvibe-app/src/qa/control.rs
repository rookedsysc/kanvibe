//! QA 제어 소켓. 재생 드라이버(`parity/tools/screens-rust/driver.py`)가 한 줄 JSON 명령을 보내고 한 줄 JSON 응답을 받는다.
//!
//! 명령: `navigate`, `click`, `hover`, `bounds`, `focus`, `fill`, `scroll_into_view`, `mouse_move`, `mouse_down`,
//! `mouse_up`, `park_pointer`, `key`, `type`, `wait_for`, `wait_idle`, `report_ready`, `fault`, `clear_fault`,
//! `board_event`, `main_send`, `activate_window`, `quit`.
//! 소켓 스레드는 명령을 GPUI 전경 스레드로 넘기고 응답을 기다리기만 한다. 화면 상태는 전경 스레드에서만 바뀐다.
//! 대상을 찾는 명령은 새 프레임이 그려진 뒤에 찾고, Playwright처럼 대상이 나타날 때까지 기다린다(기본 15초).
//! 직전 단계가 바꾼 화면의 위치를 써야 옛 위치를 누르지 않는다. 누르기·올리기는 Playwright처럼 대상이 잘려 있으면
//! 먼저 보이게 스크롤한다.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc as async_mpsc;
use gpui_kit::{
    AnyWindowHandle, App, AsyncApp, Bounds, Global, Keystroke, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta,
    ScrollEvent, ScrollSource, TouchPhase, Window, point, px,
};
use kanvibe_domain::notifications::AppNotification;
use kanvibe_services::board_notifier::BoardEvent;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::qa::faults::ServiceFault;
use crate::qa::targets::{QaTargetHit, QaTargetRegistry, TargetQuery};
use crate::qa::window_focus::activate_qa_window;
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
    /// 대상에 키보드 포커스를 준다(Playwright `focus`). 포커스를 받지 않는 대상이면 실패한다
    Focus {
        target: TargetQuery,
    },
    /// 입력란 값을 통째로 바꾼다(Playwright `fill`)
    Fill {
        target: TargetQuery,
        value: String,
    },
    /// 대상이 잘려 있으면 보이게 스크롤한다(Playwright `scrollIntoViewIfNeeded`)
    ScrollIntoView {
        target: TargetQuery,
    },
    /// Electron main이 창에 보내던 메시지(TS 드라이버 `main_send`). 지금은 `kanvibe:notification-activated`만 있다
    MainSend {
        channel: String,
        payload: Value,
    },
    /// 창을 활성(포커스)으로 만들고 GPUI가 활성으로 볼 때까지 기다린다
    ActivateWindow,
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

/// TS 드라이버 `page.setDefaultTimeout(15000)`
const DEFAULT_WAIT_FOR_TIMEOUT: Duration = Duration::from_millis(15_000);
const WAIT_FOR_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// 창이 활성으로 바뀌기를 기다리는 시간
const WINDOW_ACTIVATION_TIMEOUT: Duration = Duration::from_millis(5_000);
/// 스크롤 한 번에 다 못 옮기는 중첩 영역을 위해 몇 번까지 다시 굴린다
const MAX_SCROLL_STEPS: usize = 8;
/// TS 드라이버가 알림 열기를 보내던 채널
const NOTIFICATION_ACTIVATED_CHANNEL: &str = "kanvibe:notification-activated";

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
        ControlCommand::WaitIdle | ControlCommand::ReportReady => {
            wait_two_frames(window_handle, reply, cx);
        }
        ControlCommand::ActivateWindow => spawn_activate_window(window_handle, reply, cx),
        command @ (ControlCommand::WaitFor { .. }
        | ControlCommand::Click { .. }
        | ControlCommand::Hover { .. }
        | ControlCommand::Bounds { .. }
        | ControlCommand::Focus { .. }
        | ControlCommand::Fill { .. }
        | ControlCommand::ScrollIntoView { .. }) => {
            cx.spawn(async move |cx: &mut AsyncApp| {
                let response = run_target_command(command, window_handle, cx).await;
                let _ = reply.send(response);
            })
            .detach();
        }
        command => {
            let response = execute_immediate(command, window_handle, cx);
            let _ = reply.send(response);
        }
    }
}

/// 대상을 기다렸다가(필요하면 스크롤해서) 동작한다
async fn run_target_command(
    command: ControlCommand,
    window_handle: AnyWindowHandle,
    cx: &mut AsyncApp,
) -> Value {
    let outcome = match command {
        ControlCommand::WaitFor { target, timeout_ms } => {
            let timeout = timeout_ms
                .map(Duration::from_millis)
                .unwrap_or(DEFAULT_WAIT_FOR_TIMEOUT);
            wait_for_target(window_handle, &target, timeout, cx)
                .await
                .map(|_| ok())
        }
        ControlCommand::Bounds { target } => {
            wait_for_target(window_handle, &target, DEFAULT_WAIT_FOR_TIMEOUT, cx)
                .await
                .map(|hit| bounds_response(hit.bounds))
        }
        ControlCommand::ScrollIntoView { target } => {
            scroll_target_into_view(window_handle, &target, cx)
                .await
                .map(|_| ok())
        }
        ControlCommand::Click {
            target,
            button,
            click_count,
        } => match scroll_target_into_view(window_handle, &target, cx).await {
            Ok(hit) => cx.update(|cx| {
                update_window(window_handle, cx, |window, cx| {
                    click_at(hit.bounds, button, click_count.unwrap_or(1), window, cx)
                })
            }),
            Err(error) => Err(error),
        },
        ControlCommand::Hover { target } => {
            match scroll_target_into_view(window_handle, &target, cx).await {
                Ok(hit) => cx.update(|cx| {
                    update_window(window_handle, cx, |window, cx| {
                        move_mouse(hit.bounds.center(), window, cx)
                    })
                }),
                Err(error) => Err(error),
            }
        }
        ControlCommand::Focus { target } => {
            match wait_for_target(window_handle, &target, DEFAULT_WAIT_FOR_TIMEOUT, cx).await {
                Ok(hit) => cx.update(|cx| focus_target(window_handle, &hit, cx)),
                Err(error) => Err(error),
            }
        }
        ControlCommand::Fill { target, value } => {
            match wait_for_target(window_handle, &target, DEFAULT_WAIT_FOR_TIMEOUT, cx).await {
                Ok(hit) => cx.update(|cx| fill_target(window_handle, &hit, &value, cx)),
                Err(error) => Err(error),
            }
        }
        _ => Err("not a target command".to_string()),
    };
    outcome.unwrap_or_else(failure)
}

fn bounds_response(bounds: Bounds<Pixels>) -> Value {
    json!({"ok": true, "bounds": {
        "x": f32::from(bounds.origin.x), "y": f32::from(bounds.origin.y),
        "width": f32::from(bounds.size.width), "height": f32::from(bounds.size.height),
    }})
}

fn update_window(
    window_handle: AnyWindowHandle,
    cx: &mut App,
    act: impl FnOnce(&mut Window, &mut App),
) -> Result<Value, String> {
    window_handle
        .update(cx, |_, window, cx| act(window, cx))
        .map(|()| ok())
        .map_err(|error| error.to_string())
}

fn focus_target(
    window_handle: AnyWindowHandle,
    hit: &QaTargetHit,
    cx: &mut App,
) -> Result<Value, String> {
    let Some(focus_handle) = hit.focus_handle.clone() else {
        return Err("target is not focusable".to_string());
    };
    update_window(window_handle, cx, |window, cx| {
        focus_handle.focus(window, cx)
    })
}

/// Playwright `fill`: 포커스를 주고 값을 바꾼다
fn fill_target(
    window_handle: AnyWindowHandle,
    hit: &QaTargetHit,
    value: &str,
    cx: &mut App,
) -> Result<Value, String> {
    let Some(fill) = hit.fill.clone() else {
        return Err("target is not an input".to_string());
    };
    let focus_handle = hit.focus_handle.clone();
    update_window(window_handle, cx, |window, cx| {
        if let Some(focus_handle) = focus_handle {
            focus_handle.focus(window, cx);
        }
        fill(value, window, cx);
    })
}

/// 창을 다시 그리고 다음 프레임이 그려질 때까지 기다린다. 창이 닫혔으면 `false`다
async fn next_frame(window_handle: AnyWindowHandle, cx: &mut AsyncApp) -> bool {
    let (frame_sender, frame_receiver) = futures::channel::oneshot::channel();
    let is_requested = cx.update(|cx| {
        window_handle
            .update(cx, |_, window, _| {
                window.refresh();
                window.on_next_frame(move |_, _| {
                    let _ = frame_sender.send(());
                });
            })
            .is_ok()
    });
    is_requested && frame_receiver.await.is_ok()
}

/// 대상이 그려질 때까지 프레임마다 찾는다(Playwright 자동 대기)
async fn wait_for_target(
    window_handle: AnyWindowHandle,
    target: &TargetQuery,
    timeout: Duration,
    cx: &mut AsyncApp,
) -> Result<QaTargetHit, String> {
    let started_at = Instant::now();
    loop {
        if !next_frame(window_handle, cx).await {
            return Err("window closed".to_string());
        }
        if let Some(hit) = cx.update(|cx| find_target_hit(window_handle, target, cx)) {
            return Ok(hit);
        }
        if started_at.elapsed() >= timeout {
            return Err(format!("timed out waiting for {target:?}"));
        }
        cx.background_executor().timer(WAIT_FOR_POLL_INTERVAL).await;
    }
}

/// 대상이 자르는 영역 밖에 있으면 휠을 굴려 보이게 한다. 더 굴러가지 않으면 그 자리에서 멈춘다
async fn scroll_target_into_view(
    window_handle: AnyWindowHandle,
    target: &TargetQuery,
    cx: &mut AsyncApp,
) -> Result<QaTargetHit, String> {
    let mut hit = wait_for_target(window_handle, target, DEFAULT_WAIT_FOR_TIMEOUT, cx).await?;
    for _ in 0..MAX_SCROLL_STEPS {
        let Some(offset) = scroll_offset_into_view(hit.bounds, hit.clip_bounds) else {
            return Ok(hit);
        };
        let wheel = PlatformInput::Scroll(ScrollEvent {
            position: scroll_wheel_position(hit.bounds, hit.clip_bounds),
            delta: ScrollDelta::Pixels(offset),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
        });
        cx.update(|cx| dispatch_input(window_handle, wheel, cx));
        let moved = wait_for_target(window_handle, target, DEFAULT_WAIT_FOR_TIMEOUT, cx).await?;
        let has_moved = moved.bounds != hit.bounds;
        hit = moved;
        if !has_moved {
            break;
        }
    }
    Ok(hit)
}

/// 대상이 움직여야 할 거리. 다 보이면 `None`이다. Chromium `scrollIntoViewIfNeeded`(CenterIfNeeded)처럼
/// 전혀 안 보이면 가운데로, 일부만 보이면 가까운 가장자리에 맞춘다
pub fn scroll_offset_into_view(
    target: Bounds<Pixels>,
    clip: Bounds<Pixels>,
) -> Option<Point<Pixels>> {
    let axis_offset = |start: Pixels, end: Pixels, clip_start: Pixels, clip_end: Pixels| {
        if start >= clip_start && end <= clip_end {
            return px(0.0);
        }
        let is_partly_visible = start < clip_end && end > clip_start;
        if !is_partly_visible {
            return (clip_start + clip_end) / 2.0 - (start + end) / 2.0;
        }
        if start < clip_start {
            clip_start - start
        } else {
            clip_end - end
        }
    };
    let offset = point(
        axis_offset(target.left(), target.right(), clip.left(), clip.right()),
        axis_offset(target.top(), target.bottom(), clip.top(), clip.bottom()),
    );
    (offset != point(px(0.0), px(0.0))).then_some(offset)
}

/// 휠을 굴릴 자리: 자르는 영역 안에서 대상에 가장 가까운 점. 그 점 아래의 가장 안쪽 스크롤 영역이 대상을 담는다
fn scroll_wheel_position(target: Bounds<Pixels>, clip: Bounds<Pixels>) -> Point<Pixels> {
    let center = target.center();
    let clamp =
        |value: Pixels, low: Pixels, high: Pixels| value.max(low + px(1.0)).min(high - px(1.0));
    point(
        clamp(center.x, clip.left(), clip.right()),
        clamp(center.y, clip.top(), clip.bottom()),
    )
}

/// 창을 활성으로 만들고 GPUI가 활성으로 볼 때까지 기다린다
fn spawn_activate_window(window_handle: AnyWindowHandle, reply: mpsc::Sender<Value>, cx: &mut App) {
    let _ = window_handle.update(cx, |_, window, _| activate_qa_window(window));
    let started_at = Instant::now();
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            let is_active = cx.update(|cx| {
                window_handle
                    .update(cx, |_, window, _| window.is_window_active())
                    .unwrap_or(false)
            });
            if is_active {
                let _ = reply.send(json!({"ok": true, "is_active": true}));
                return;
            }
            if started_at.elapsed() >= WINDOW_ACTIVATION_TIMEOUT {
                let _ = reply.send(failure("window did not become active"));
                return;
            }
            cx.background_executor().timer(WAIT_FOR_POLL_INTERVAL).await;
        }
    })
    .detach();
}

fn execute_immediate(
    command: ControlCommand,
    window_handle: AnyWindowHandle,
    cx: &mut App,
) -> Value {
    match command {
        ControlCommand::Navigate { route } => navigate_window(window_handle, &route, cx),
        ControlCommand::MainSend { channel, payload } => send_main_message(&channel, payload, cx),
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
        ControlCommand::WaitFor { .. }
        | ControlCommand::WaitIdle
        | ControlCommand::ReportReady
        | ControlCommand::ActivateWindow
        | ControlCommand::Click { .. }
        | ControlCommand::Hover { .. }
        | ControlCommand::Bounds { .. }
        | ControlCommand::Focus { .. }
        | ControlCommand::Fill { .. }
        | ControlCommand::ScrollIntoView { .. } => {
            failure("waiting commands are handled asynchronously")
        }
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

/// TS main이 `webContents.send(channel, payload)`로 모든 창에 보내던 메시지
fn send_main_message(channel: &str, payload: Value, cx: &mut App) -> Value {
    if channel != NOTIFICATION_ACTIVATED_CHANNEL {
        return failure(format!("unsupported main channel: {channel}"));
    }
    match serde_json::from_value::<AppNotification>(payload) {
        Ok(notification) => {
            crate::app_events::activate_notification(notification, None, cx);
            ok()
        }
        Err(error) => failure(format!("invalid notification: {error}")),
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

fn find_target_hit(
    window_handle: AnyWindowHandle,
    target: &TargetQuery,
    cx: &App,
) -> Option<QaTargetHit> {
    cx.try_global::<QaTargetRegistry>()?
        .find_hit(window_handle.window_id(), target)
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
    fn new_step_commands_parse_from_driver_json_lines() {
        let command: ControlCommand = serde_json::from_str(
            r#"{"cmd":"fill","target":{"placeholder":"계정 이름 (예: work)","nth":1},"value":"work"}"#,
        )
        .unwrap();
        assert!(matches!(command, ControlCommand::Fill { ref value, .. } if value == "work"));
        let command: ControlCommand = serde_json::from_str(
            r#"{"cmd":"main_send","channel":"kanvibe:notification-activated","payload":{"id":"n"}}"#,
        )
        .unwrap();
        assert!(matches!(command, ControlCommand::MainSend { .. }));
        for line in [
            r#"{"cmd":"focus","target":{"selector":"[data-x]"}}"#,
            r#"{"cmd":"scroll_into_view","target":{"role":"button","name":"더 보기"}}"#,
            r#"{"cmd":"activate_window"}"#,
        ] {
            assert!(
                serde_json::from_str::<ControlCommand>(line).is_ok(),
                "{line}"
            );
        }
    }

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), gpui_kit::size(px(width), px(height)))
    }

    #[test]
    fn scroll_offset_follows_center_if_needed() {
        let clip = bounds(0.0, 0.0, 100.0, 100.0);
        assert_eq!(
            scroll_offset_into_view(bounds(10.0, 10.0, 20.0, 20.0), clip),
            None
        );
        // 전혀 안 보이면 가운데로
        assert_eq!(
            scroll_offset_into_view(bounds(10.0, 300.0, 20.0, 20.0), clip),
            Some(point(px(0.0), px(-260.0)))
        );
        // 아래가 잘렸으면 아래 가장자리에 맞춘다
        assert_eq!(
            scroll_offset_into_view(bounds(10.0, 90.0, 20.0, 20.0), clip),
            Some(point(px(0.0), px(-10.0)))
        );
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
