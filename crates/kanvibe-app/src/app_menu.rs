//! macOS 표준 앱 메뉴. 시스템 단축키는 앱의 사용자 지정 단축키와 별도로 등록한다.

use gpui_kit::{App, KeyBinding, Menu, MenuItem, PromptButton, PromptLevel, Window, actions};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::terminal_tabs::TerminalTabShortcutCommand;

use crate::shortcuts::RunTerminalTabCommand;

actions!(
    macos_menu,
    [
        About,
        Quit,
        HideApp,
        HideOtherApps,
        ShowAllApps,
        MinimizeWindow,
        ZoomWindow
    ]
);

pub(super) fn install_app_menu(cx: &mut App) {
    if ShortcutPlatform::current() == ShortcutPlatform::Mac {
        install_macos_app_menu(cx);
    }
}

fn install_macos_app_menu(cx: &mut App) {
    cx.on_action(|_: &About, cx| {
        with_active_window(cx, |window, cx| {
            drop(window.prompt(
                PromptLevel::Info,
                "KanVibe",
                Some(concat!("Version ", env!("CARGO_PKG_VERSION"))),
                &[PromptButton::ok("OK")],
                cx,
            ));
        });
    });
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOtherApps, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAllApps, cx| cx.unhide_other_apps());
    cx.on_action(|_: &MinimizeWindow, cx| {
        with_active_window(cx, |window, _| window.minimize_window());
    });
    cx.on_action(|_: &ZoomWindow, cx| {
        with_active_window(cx, |window, _| window.zoom_window());
    });

    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-alt-h", HideOtherApps, None),
        KeyBinding::new("cmd-m", MinimizeWindow, None),
    ]);
    cx.set_menus([
        Menu::new("KanVibe").items([
            MenuItem::action("About KanVibe", About),
            MenuItem::separator(),
            MenuItem::action("Hide KanVibe", HideApp),
            MenuItem::action("Hide Others", HideOtherApps),
            MenuItem::action("Show All", ShowAllApps),
            MenuItem::separator(),
            MenuItem::action("Quit KanVibe", Quit),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", MinimizeWindow),
            MenuItem::action("Zoom", ZoomWindow),
            MenuItem::separator(),
            MenuItem::action(
                "Close Window",
                RunTerminalTabCommand {
                    command: TerminalTabShortcutCommand::CloseWindow,
                },
            ),
        ]),
    ]);
}

fn with_active_window(cx: &mut App, action: impl FnOnce(&mut Window, &mut App) + 'static) {
    if let Some(window) = cx.active_window() {
        // 액션을 전달 중인 창을 다시 빌리기 전에 디스패치가 끝나야 한다
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| action(window, cx));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AsKeystroke, Keystroke, OwnedMenuItem, TestAppContext, gpui};

    #[gpui::test]
    fn macos_menu_and_keyboard_expose_the_same_quit_action(cx: &mut TestAppContext) {
        cx.update(|cx| {
            install_macos_app_menu(cx);
            assert!(cx.is_action_available(&Quit));

            let menus = cx.get_menus().unwrap();
            assert_eq!(menus[0].name.as_ref(), "KanVibe");
            assert!(menus[0].items.iter().any(|item| matches!(
                item,
                OwnedMenuItem::Action { name, action, .. }
                    if name == "Quit KanVibe" && action.as_any().is::<Quit>()
            )));
            assert_eq!(menus[1].name.as_ref(), "Window");
            assert!(
                cx.key_bindings()
                    .borrow()
                    .bindings_for_action(&Quit)
                    .any(|binding| {
                        binding.keystrokes()[0].as_keystroke()
                            == &Keystroke::parse("cmd-q").unwrap()
                    })
            );
        });
    }
}
