//! 단축키가 `Mod`를 어떤 키로 읽을지 정하는 플랫폼 판정.
//! 기능 코드가 `cfg!(target_os)`나 수식키 이름을 직접 보지 않고 이 판정 하나를 거치게 한다.

/// `Mod`가 macOS에서는 Command, 그 밖의 플랫폼에서는 Control을 뜻한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutPlatform {
    Mac,
    Linux,
}

impl ShortcutPlatform {
    /// 지금 실행 중인 OS의 단축키 플랫폼
    pub fn current() -> Self {
        Self::from_os_name(std::env::consts::OS)
    }

    /// `std::env::consts::OS` 값으로 판정한다. macOS가 아니면 모두 Control을 `Mod`로 쓴다.
    pub fn from_os_name(os_name: &str) -> Self {
        if os_name == "macos" {
            Self::Mac
        } else {
            Self::Linux
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // parity: TS-renderer-keyboardShortcut-9bc682
    #[test]
    fn only_macos_uses_command_as_mod() {
        assert_eq!(
            ShortcutPlatform::from_os_name("macos"),
            ShortcutPlatform::Mac
        );
        assert_eq!(
            ShortcutPlatform::from_os_name("linux"),
            ShortcutPlatform::Linux
        );
        assert_eq!(
            ShortcutPlatform::from_os_name("windows"),
            ShortcutPlatform::Linux
        );
        assert_eq!(
            ShortcutPlatform::current(),
            ShortcutPlatform::from_os_name(std::env::consts::OS)
        );
    }
}
