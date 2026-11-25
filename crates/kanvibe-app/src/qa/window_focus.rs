//! QA 재생 창을 앞으로(활성) 만든다. TS 기준 이미지는 Playwright가 포커스를 준 창에서 찍었다(`document.hasFocus()`가 참).
//! 창 관리자가 없는 Xvfb에서는 `_NET_ACTIVE_WINDOW` 요청을 받을 쪽이 없어 GPUI `activate_window`가 아무 일도 하지 않는다.
//! 그래서 X11에서는 창에 입력 포커스를 직접 준다. 그러면 X 서버가 FocusIn을 보내고 GPUI가 창을 활성으로 바꾼다.

use gpui_kit::Window;

/// 창을 활성으로 만들어 달라고 요청한다. 결과는 `window.is_window_active()`로 확인한다
pub fn activate_qa_window(window: &mut Window) {
    window.activate_window();
    #[cfg(target_os = "linux")]
    if let Err(error) = focus_x11_window(window) {
        eprintln!("[kanvibe-qa] X11 input focus failed: {error:#}");
    }
}

#[cfg(target_os = "linux")]
fn focus_x11_window(window: &Window) -> anyhow::Result<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, InputFocus};

    let handle = HasWindowHandle::window_handle(window)?;
    let x_window = match handle.as_raw() {
        RawWindowHandle::Xcb(handle) => handle.window.get(),
        RawWindowHandle::Xlib(handle) => u32::try_from(handle.window)?,
        // Wayland 합성기는 창 활성을 클라이언트가 정하지 못한다
        _ => return Ok(()),
    };
    let (connection, _) = x11rb::rust_connection::RustConnection::connect(None)?;
    connection
        .set_input_focus(InputFocus::PARENT, x_window, x11rb::CURRENT_TIME)?
        .check()?;
    connection.flush()?;
    Ok(())
}
