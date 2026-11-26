//! 실제 키 입력과 단축키 표기를 맞대는 판정. 앱 전역 처리와 터미널보다 먼저 가로채는 처리가 같은 판정을 쓴다.

use super::defaults::BLOCKED_DESKTOP_SHORTCUTS;
use super::notation::{ShortcutModifier, ShortcutParts, join_shortcut, normalize_key_token};
use crate::platform::ShortcutPlatform;

/// 눌려 있는 수식키
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModifierState {
    pub has_meta: bool,
    pub has_ctrl: bool,
    pub has_alt: bool,
    pub has_shift: bool,
}

/// 키 하나의 입력. `key`는 브라우저 `KeyboardEvent.key`와 같은 이름(`o`, `{`, `Meta`, `Control`, ` `)을 쓴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutKeyEvent {
    pub key: String,
    pub modifiers: ModifierState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPhase {
    Down,
    Up,
}

/// 누름·뗌 구분과 자동 반복, IME 조합 여부까지 담은 입력. 수식키 단독 release 판정에 필요하다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyTransition {
    pub phase: KeyPhase,
    pub is_auto_repeat: bool,
    pub is_composing: bool,
    pub event: ShortcutKeyEvent,
}

const MODIFIER_KEY_NAMES: [&str; 5] = ["Meta", "Control", "Ctrl", "Alt", "Shift"];

/// 수식키 단독 입력은 아직 조합이 완성되지 않은 상태다. 녹화 화면이 이 경우를 따로 다루려면 밖에서 판별해야 한다.
pub fn is_shortcut_modifier_key(key: &str) -> bool {
    MODIFIER_KEY_NAMES.contains(&key)
}

/// Shift를 누르면 대괄호가 중괄호로 보고된다. 접지 않으면 `Mod+Shift+[`가 영영 매칭되지 않는다.
pub(crate) fn normalize_event_key(key: &str) -> String {
    if key == " " {
        return "Space".to_string();
    }
    let mut chars = key.chars();
    if let (Some(single_char), None) = (chars.next(), chars.next()) {
        let unshifted_char = match single_char {
            '{' => '[',
            '}' => ']',
            other => other,
        };
        return unshifted_char.to_uppercase().collect();
    }
    if key == "Control" {
        return "Ctrl".to_string();
    }
    normalize_key_token(key)
}

pub(crate) fn matches_shortcut_modifiers(
    state: ModifierState,
    parts: &ShortcutParts,
    platform: ShortcutPlatform,
) -> bool {
    let requires_mod = parts.has(ShortcutModifier::Mod);
    let expected_meta =
        parts.has(ShortcutModifier::Meta) || (requires_mod && platform == ShortcutPlatform::Mac);
    let expected_ctrl =
        parts.has(ShortcutModifier::Ctrl) || (requires_mod && platform == ShortcutPlatform::Linux);

    state.has_meta == expected_meta
        && state.has_ctrl == expected_ctrl
        && state.has_alt == parts.has(ShortcutModifier::Alt)
        && state.has_shift == parts.has(ShortcutModifier::Shift)
}

/// 수식키가 정확히 같고 키가 같을 때만 일치로 본다. 수식키가 하나라도 더 눌려 있으면 다른 단축키다.
pub fn match_shortcut_event(
    event: &ShortcutKeyEvent,
    shortcut: &str,
    platform: ShortcutPlatform,
) -> bool {
    let parts = ShortcutParts::parse(shortcut);
    if parts.key.is_empty() || parts.key != normalize_event_key(&event.key) {
        return false;
    }
    matches_shortcut_modifiers(event.modifiers, &parts, platform)
}

/// 터미널보다 먼저 가로채는 경로의 판정. 누르는 순간만 보고 자동 반복은 무시한다.
pub fn match_key_transition(
    transition: &KeyTransition,
    shortcut: &str,
    platform: ShortcutPlatform,
) -> bool {
    if transition.phase != KeyPhase::Down || transition.is_auto_repeat {
        return false;
    }
    match_shortcut_event(&transition.event, shortcut, platform)
}

pub fn is_blocked_shortcut_event(event: &ShortcutKeyEvent, platform: ShortcutPlatform) -> bool {
    BLOCKED_DESKTOP_SHORTCUTS
        .iter()
        .any(|shortcut| match_shortcut_event(event, shortcut, platform))
}

pub fn is_blocked_key_transition(transition: &KeyTransition, platform: ShortcutPlatform) -> bool {
    BLOCKED_DESKTOP_SHORTCUTS
        .iter()
        .any(|shortcut| match_key_transition(transition, shortcut, platform))
}

fn captured_modifiers(
    modifiers: ModifierState,
    platform: ShortcutPlatform,
) -> Vec<ShortcutModifier> {
    let is_mac = platform == ShortcutPlatform::Mac;
    let mut captured = Vec::new();
    if modifiers.has_meta {
        captured.push(if is_mac {
            ShortcutModifier::Mod
        } else {
            ShortcutModifier::Meta
        });
    }
    if modifiers.has_ctrl {
        captured.push(if is_mac {
            ShortcutModifier::Ctrl
        } else {
            ShortcutModifier::Mod
        });
    }
    if modifiers.has_alt {
        captured.push(ShortcutModifier::Alt);
    }
    if modifiers.has_shift {
        captured.push(ShortcutModifier::Shift);
    }
    captured.sort();
    captured
}

/// 녹화 화면에서 누른 조합을 저장할 정규 표기로 바꾼다.
/// 수식키가 없거나, 수식키만 눌렸거나, 표기에 담을 수 없는 `+` 키거나, 앱이 막는 조합이면 `None`이다.
pub fn capture_shortcut_from_event(
    event: &ShortcutKeyEvent,
    platform: ShortcutPlatform,
) -> Option<String> {
    if is_shortcut_modifier_key(&event.key) {
        return None;
    }
    let modifiers = captured_modifiers(event.modifiers, platform);
    let key = normalize_event_key(&event.key);
    if modifiers.is_empty() || key.is_empty() {
        return None;
    }

    let shortcut = join_shortcut(&modifiers, &key);
    // `+`는 토큰 구분자라 다시 파싱하면 키가 사라진다. 그대로 저장하면 배정된 것처럼 보이고 아무것도 매칭되지 않는다.
    if ShortcutParts::parse(&shortcut).key != key {
        return None;
    }
    let is_blocked = BLOCKED_DESKTOP_SHORTCUTS.iter().any(|blocked_shortcut| {
        ShortcutParts::parse(blocked_shortcut).key == key
            && match_shortcut_event(event, blocked_shortcut, platform)
    });
    if is_blocked {
        return None;
    }
    Some(shortcut)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// `"ctrl+shift"`처럼 적은 수식키로 입력을 만든다
    pub fn key_event(key: &str, modifier_names: &str) -> ShortcutKeyEvent {
        let has = |name: &str| modifier_names.split('+').any(|part| part == name);
        ShortcutKeyEvent {
            key: key.to_string(),
            modifiers: ModifierState {
                has_meta: has("meta"),
                has_ctrl: has("ctrl"),
                has_alt: has("alt"),
                has_shift: has("shift"),
            },
        }
    }

    pub fn key_down(key: &str, modifier_names: &str) -> KeyTransition {
        KeyTransition {
            phase: KeyPhase::Down,
            is_auto_repeat: false,
            is_composing: false,
            event: key_event(key, modifier_names),
        }
    }

    pub fn key_up(key: &str, modifier_names: &str) -> KeyTransition {
        KeyTransition {
            phase: KeyPhase::Up,
            ..key_down(key, modifier_names)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{key_down, key_event};
    use super::*;
    use crate::shortcuts::defaults;
    use crate::shortcuts::notation::format_shortcut_for_display;
    use crate::shortcuts::{
        TASK_DETAIL_DOCK_SHORTCUT_INDEXES, TERMINAL_TAB_SHORTCUT_INDEXES,
        task_detail_dock_shortcut, terminal_tab_shortcut,
    };

    const MAC: ShortcutPlatform = ShortcutPlatform::Mac;
    const LINUX: ShortcutPlatform = ShortcutPlatform::Linux;

    #[test]
    fn mod_matches_ctrl_outside_macos() {
        assert!(match_shortcut_event(
            &key_event("o", "ctrl+shift"),
            "Mod+Shift+O",
            LINUX
        ));
    }

    #[test]
    fn mod_matches_cmd_on_macos() {
        assert!(match_shortcut_event(
            &key_event("o", "meta+shift"),
            "Mod+Shift+O",
            MAC
        ));
    }

    #[test]
    fn page_navigation_matches_the_platform_mod() {
        assert!(match_shortcut_event(
            &key_event("[", "meta"),
            defaults::PAGE_BACK,
            MAC
        ));
        assert!(match_shortcut_event(
            &key_event("]", "meta"),
            defaults::PAGE_FORWARD,
            MAC
        ));
        assert!(match_shortcut_event(
            &key_event("[", "ctrl"),
            defaults::PAGE_BACK,
            LINUX
        ));
        assert!(match_shortcut_event(
            &key_event("]", "ctrl"),
            defaults::PAGE_FORWARD,
            LINUX
        ));
    }

    #[test]
    fn linux_page_navigation_does_not_match_alt() {
        assert!(!match_shortcut_event(
            &key_event("[", "alt"),
            defaults::PAGE_BACK,
            LINUX
        ));
    }

    #[test]
    fn shifted_bracket_is_tab_navigation_not_page_navigation() {
        let shifted_bracket = key_event("{", "meta+shift");

        assert!(!match_shortcut_event(
            &shifted_bracket,
            defaults::PAGE_BACK,
            MAC
        ));
        assert!(match_shortcut_event(
            &shifted_bracket,
            defaults::TERMINAL_TAB_PREVIOUS,
            MAC
        ));
    }

    #[test]
    fn reported_brace_matches_the_bracket_shortcut() {
        assert!(match_shortcut_event(
            &key_event("}", "ctrl+shift"),
            defaults::TERMINAL_TAB_NEXT,
            LINUX
        ));
    }

    #[test]
    fn shift_separates_tab_close_from_window_close() {
        let close_tab = key_event("w", "ctrl");
        let close_window = key_event("w", "ctrl+shift");

        assert!(match_shortcut_event(
            &close_tab,
            defaults::TERMINAL_TAB_CLOSE,
            LINUX
        ));
        assert!(!match_shortcut_event(
            &close_tab,
            defaults::TERMINAL_WINDOW_CLOSE,
            LINUX
        ));
        assert!(match_shortcut_event(
            &close_window,
            defaults::TERMINAL_WINDOW_CLOSE,
            LINUX
        ));
        assert!(!match_shortcut_event(
            &close_window,
            defaults::TERMINAL_TAB_CLOSE,
            LINUX
        ));
    }

    #[test]
    fn tab_number_shortcuts_avoid_the_macos_screenshot_combination() {
        for index in TERMINAL_TAB_SHORTCUT_INDEXES {
            let shortcut = terminal_tab_shortcut(index);
            assert_eq!(
                format_shortcut_for_display(&shortcut, MAC),
                format!("Cmd+Option+{index}")
            );
            assert_eq!(
                format_shortcut_for_display(&shortcut, LINUX),
                format!("Ctrl+Alt+{index}")
            );
            assert!(!match_shortcut_event(
                &key_event(&index.to_string(), "meta+shift"),
                &shortcut,
                MAC
            ));
        }
    }

    #[test]
    fn extra_modifier_prevents_a_match() {
        assert!(!match_shortcut_event(
            &key_event("o", "meta+ctrl+shift"),
            "Mod+Shift+O",
            MAC
        ));
    }

    #[test]
    fn macos_command_is_captured_as_portable_mod() {
        assert_eq!(
            capture_shortcut_from_event(&key_event("p", "meta+shift"), MAC).as_deref(),
            Some("Mod+Shift+P")
        );
    }

    #[test]
    fn linux_control_is_captured_as_portable_mod() {
        assert_eq!(
            capture_shortcut_from_event(&key_event("p", "ctrl+shift"), LINUX).as_deref(),
            Some("Mod+Shift+P")
        );
    }

    #[test]
    fn key_transitions_match_the_platform_mod() {
        assert!(match_key_transition(&key_down("n", "meta"), "Mod+N", MAC));
        assert!(!match_key_transition(
            &key_down("n", "meta"),
            "Mod+N",
            LINUX
        ));
    }

    #[test]
    fn page_navigation_transitions_match_cmd_and_ctrl() {
        assert!(match_key_transition(
            &key_down("[", "meta"),
            defaults::PAGE_BACK,
            MAC
        ));
        assert!(match_key_transition(
            &key_down("]", "meta"),
            defaults::PAGE_FORWARD,
            MAC
        ));
        assert!(match_key_transition(
            &key_down("[", "ctrl"),
            defaults::PAGE_BACK,
            LINUX
        ));
        assert!(match_key_transition(
            &key_down("]", "ctrl"),
            defaults::PAGE_FORWARD,
            LINUX
        ));
    }

    #[test]
    fn auto_repeat_and_key_up_never_match() {
        let repeated = KeyTransition {
            is_auto_repeat: true,
            ..key_down("n", "ctrl")
        };
        let released = KeyTransition {
            phase: KeyPhase::Up,
            ..key_down("n", "ctrl")
        };

        assert!(!match_key_transition(&repeated, "Mod+N", LINUX));
        assert!(!match_key_transition(&released, "Mod+N", LINUX));
    }

    #[test]
    fn dock_shortcuts_use_mod_and_a_number() {
        assert_eq!(TASK_DETAIL_DOCK_SHORTCUT_INDEXES, [1, 2, 3, 4, 5, 6]);
        let first_dock = task_detail_dock_shortcut(1);
        assert_eq!(format_shortcut_for_display(&first_dock, MAC), "Cmd+1");
        assert_eq!(format_shortcut_for_display(&first_dock, LINUX), "Ctrl+1");

        assert!(match_shortcut_event(
            &key_event("1", "meta"),
            &first_dock,
            MAC
        ));
        assert!(match_shortcut_event(
            &key_event("1", "ctrl"),
            &first_dock,
            LINUX
        ));
        assert!(!match_shortcut_event(
            &key_event("1", "meta+alt"),
            &first_dock,
            MAC
        ));
        assert!(!match_shortcut_event(
            &key_event("1", "ctrl+shift"),
            &first_dock,
            LINUX
        ));
        assert!(!match_shortcut_event(
            &key_event("1", "alt"),
            &first_dock,
            LINUX
        ));
    }

    #[test]
    fn mod_r_is_blocked() {
        assert!(is_blocked_shortcut_event(&key_event("r", "meta"), MAC));
        assert!(is_blocked_shortcut_event(&key_event("r", "ctrl"), LINUX));
        assert!(!is_blocked_shortcut_event(
            &key_event("r", "ctrl+shift"),
            LINUX
        ));
    }

    #[test]
    fn mod_r_transition_is_blocked() {
        assert!(is_blocked_key_transition(&key_down("r", "ctrl"), LINUX));
    }

    #[test]
    fn mod_r_is_never_captured() {
        assert_eq!(
            capture_shortcut_from_event(&key_event("r", "ctrl"), LINUX),
            None
        );
    }

    #[test]
    fn plus_key_is_never_captured() {
        assert_eq!(
            capture_shortcut_from_event(&key_event("+", "ctrl+shift"), LINUX),
            None
        );
    }

    #[test]
    fn modifier_only_input_is_not_captured() {
        assert_eq!(
            capture_shortcut_from_event(&key_event("Meta", "meta"), MAC),
            None
        );
        assert!(is_shortcut_modifier_key("Control"));
        assert!(!is_shortcut_modifier_key("o"));
    }

    #[test]
    fn input_without_modifiers_is_not_captured() {
        assert_eq!(
            capture_shortcut_from_event(&key_event("k", ""), LINUX),
            None
        );
        assert_eq!(
            capture_shortcut_from_event(&key_event("k", "meta+alt"), LINUX).as_deref(),
            Some("Meta+Alt+K")
        );
    }
}
