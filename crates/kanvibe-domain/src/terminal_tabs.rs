//! 터미널 탭. tmux window, zellij tab, KanVibe가 직접 소유하는 plain terminal 탭이 모두 이 모양이다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalTab {
    /// 멀티플렉서가 부여한 안정 식별자(tmux window_id, zellij tab id) 또는 KanVibe가 만든 탭 식별자
    pub id: String,
    /// 멀티플렉서의 원본 인덱스. 세션 종류마다 좌표계가 달라 표시 순서와 `Mod+Alt+{n}` 번호는 배열 위치를 쓴다
    pub native_index: i64,
    pub name: String,
    pub is_active: bool,
}

/// 터미널보다 먼저 가로챈 탭 단축키의 의미. 키 판정은 끝났고 실행할 동작만 남은 상태다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum TerminalTabShortcutCommand {
    NewTab,
    CloseTab,
    CloseWindow,
    PreviousTab,
    NextTab,
    GoToTab { position: u8 },
}

/// 원격 Herdr 서버를 교체하느라 터미널을 닫을 때 보내는 닫힘 사유.
/// 이 사유로 닫힌 화면은 오류를 그리지 않고 곧바로 다시 연결한다.
pub const HERDR_SERVER_REPLACED_CLOSE_REASON: &str = "kanvibe:herdr-server-replaced";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_commands_serialize_with_a_type_tag() {
        let go_to_tab = TerminalTabShortcutCommand::GoToTab { position: 3 };

        assert_eq!(
            serde_json::to_string(&go_to_tab).unwrap(),
            r#"{"type":"go-to-tab","position":3}"#
        );
        assert_eq!(
            serde_json::to_string(&TerminalTabShortcutCommand::CloseWindow).unwrap(),
            r#"{"type":"close-window"}"#
        );
    }
}
