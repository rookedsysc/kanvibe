//! 단축키 문자열 표기. 저장·비교·표시가 모두 `Mod+Shift+O` 같은 정규 표기 하나를 쓴다.

use crate::platform::ShortcutPlatform;

/// 정규 표기에서 수식키가 놓이는 순서. 선언 순서가 곧 정렬 순서다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShortcutModifier {
    /// macOS에서는 Command, 그 밖에서는 Control
    Mod,
    Meta,
    Ctrl,
    Alt,
    Shift,
}

impl ShortcutModifier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mod => "Mod",
            Self::Meta => "Meta",
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
        }
    }

    fn parse_token(token: &str) -> Option<Self> {
        match token.trim().to_lowercase().as_str() {
            "mod" => Some(Self::Mod),
            "meta" | "cmd" | "command" => Some(Self::Meta),
            "ctrl" | "control" => Some(Self::Ctrl),
            "alt" | "option" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            _ => None,
        }
    }

    /// 이 플랫폼에서 `Mod`가 가리키는 실제 수식키
    pub fn platform_mod(platform: ShortcutPlatform) -> Self {
        match platform {
            ShortcutPlatform::Mac => Self::Meta,
            ShortcutPlatform::Linux => Self::Ctrl,
        }
    }
}

/// 단축키 문자열을 수식키 집합과 키 하나로 나눈 결과. 키를 읽지 못하면 `key`가 비어 있다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutParts {
    /// 정규 순서로 정렬되고 중복이 없다
    pub modifiers: Vec<ShortcutModifier>,
    pub key: String,
}

impl ShortcutParts {
    pub fn parse(shortcut: &str) -> Self {
        let mut modifiers = Vec::new();
        let mut key = String::new();

        let tokens = shortcut
            .split('+')
            .map(str::trim)
            .filter(|token| !token.is_empty());
        for token in tokens {
            match ShortcutModifier::parse_token(token) {
                Some(modifier) => modifiers.push(modifier),
                None => key = normalize_key_token(token),
            }
        }
        modifiers.sort();
        modifiers.dedup();

        Self { modifiers, key }
    }

    pub fn has(&self, modifier: ShortcutModifier) -> bool {
        self.modifiers.contains(&modifier)
    }
}

/// 키 이름을 정규 표기로 맞춘다. 한 글자는 대문자로, 여러 글자는 첫 글자만 대문자로 바꾼다.
pub(crate) fn normalize_key_token(token: &str) -> String {
    let trimmed_token = token.trim();
    let mut chars = trimmed_token.chars();
    let Some(first_char) = chars.next() else {
        return String::new();
    };
    if chars.as_str().is_empty() {
        return trimmed_token.to_uppercase();
    }

    match trimmed_token.to_lowercase().as_str() {
        "esc" => "Escape".to_string(),
        "space" => "Space".to_string(),
        _ => first_char.to_uppercase().chain(chars).collect(),
    }
}

pub(crate) fn join_shortcut(modifiers: &[ShortcutModifier], key: &str) -> String {
    let mut parts: Vec<&str> = modifiers.iter().map(|modifier| modifier.as_str()).collect();
    parts.push(key);
    parts.join("+")
}

/// 플랫폼에서 `Mod`가 가리키는 수식키(mac은 Meta, linux는 Ctrl)를 `Mod` 표기로 접는다.
/// 매칭은 두 표기를 같은 입력으로 보므로 저장과 중복 판정도 같은 기준을 써야 실제로 겹치는 조합이 따로 배정되지 않는다.
/// mac의 Ctrl과 linux의 Meta는 `Mod`와 별개 키라 접지 않는다. 키를 읽지 못하면 빈 문자열이다.
pub fn canonicalize_shortcut_for_platform(shortcut: &str, platform: ShortcutPlatform) -> String {
    let parts = ShortcutParts::parse(shortcut);
    if parts.key.is_empty() {
        return String::new();
    }

    let platform_mod = ShortcutModifier::platform_mod(platform);
    let mut canonical_modifiers: Vec<ShortcutModifier> = parts
        .modifiers
        .iter()
        .map(|&modifier| {
            if modifier == platform_mod {
                ShortcutModifier::Mod
            } else {
                modifier
            }
        })
        .collect();
    canonical_modifiers.sort();
    canonical_modifiers.dedup();

    join_shortcut(&canonical_modifiers, &parts.key)
}

/// 사용자에게 보여 줄 표기. macOS 키캡에 새겨진 이름(Cmd, Option)을 쓴다.
pub fn format_shortcut_for_display(shortcut: &str, platform: ShortcutPlatform) -> String {
    let parts = ShortcutParts::parse(shortcut);
    let is_mac = platform == ShortcutPlatform::Mac;
    let mut display_parts: Vec<&str> = parts
        .modifiers
        .iter()
        .map(|modifier| match modifier {
            ShortcutModifier::Mod => {
                if is_mac {
                    "Cmd"
                } else {
                    "Ctrl"
                }
            }
            ShortcutModifier::Meta => {
                if is_mac {
                    "Cmd"
                } else {
                    "Meta"
                }
            }
            ShortcutModifier::Alt => {
                if is_mac {
                    "Option"
                } else {
                    "Alt"
                }
            }
            other => other.as_str(),
        })
        .collect();

    if !parts.key.is_empty() {
        display_parts.push(&parts.key);
    }
    display_parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::defaults;

    // parity: TS-renderer-keyboardShortcut-2aa70b
    #[test]
    fn mod_is_displayed_as_cmd_on_macos() {
        assert_eq!(
            format_shortcut_for_display("Mod+Shift+o", ShortcutPlatform::Mac),
            "Cmd+Shift+O"
        );
    }

    // parity: TS-renderer-keyboardShortcut-3d63f0
    #[test]
    fn page_navigation_shortcuts_are_displayed_per_platform() {
        assert_eq!(
            format_shortcut_for_display(defaults::PAGE_BACK, ShortcutPlatform::Mac),
            "Cmd+["
        );
        assert_eq!(
            format_shortcut_for_display(defaults::PAGE_FORWARD, ShortcutPlatform::Mac),
            "Cmd+]"
        );
        assert_eq!(
            format_shortcut_for_display(defaults::PAGE_BACK, ShortcutPlatform::Linux),
            "Ctrl+["
        );
        assert_eq!(
            format_shortcut_for_display(defaults::PAGE_FORWARD, ShortcutPlatform::Linux),
            "Ctrl+]"
        );
    }

    // parity: TS-renderer-keyboardShortcut-a47d4e
    #[test]
    fn tab_shortcuts_are_displayed_with_the_platform_mod() {
        let mac = ShortcutPlatform::Mac;
        let linux = ShortcutPlatform::Linux;
        assert_eq!(
            format_shortcut_for_display(defaults::TERMINAL_TAB_NEW, mac),
            "Cmd+T"
        );
        assert_eq!(
            format_shortcut_for_display(defaults::TERMINAL_TAB_CLOSE, mac),
            "Cmd+W"
        );
        assert_eq!(
            format_shortcut_for_display(defaults::TERMINAL_WINDOW_CLOSE, mac),
            "Cmd+Shift+W"
        );
        assert_eq!(
            format_shortcut_for_display(defaults::TERMINAL_TAB_NEW, linux),
            "Ctrl+T"
        );
        assert_eq!(
            format_shortcut_for_display(defaults::TERMINAL_WINDOW_CLOSE, linux),
            "Ctrl+Shift+W"
        );
    }

    #[test]
    fn parsing_normalizes_aliases_order_and_key_names() {
        let parts = ShortcutParts::parse(" shift + command + esc ");

        assert_eq!(
            parts.modifiers,
            [ShortcutModifier::Meta, ShortcutModifier::Shift]
        );
        assert_eq!(parts.key, "Escape");
        assert_eq!(ShortcutParts::parse("Mod+Shift++").key, "");
        assert_eq!(ShortcutParts::parse("ctrl+arrowUp").key, "ArrowUp");
    }

    #[test]
    fn canonical_form_folds_only_the_platform_mod() {
        assert_eq!(
            canonicalize_shortcut_for_platform("Shift+Meta+o", ShortcutPlatform::Mac),
            "Mod+Shift+O"
        );
        assert_eq!(
            canonicalize_shortcut_for_platform("Ctrl+Shift+O", ShortcutPlatform::Mac),
            "Ctrl+Shift+O"
        );
        assert_eq!(
            canonicalize_shortcut_for_platform("Ctrl+Shift+++", ShortcutPlatform::Linux),
            ""
        );
    }
}
