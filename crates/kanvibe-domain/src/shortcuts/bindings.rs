//! 사용자가 다시 배정할 수 있는 단축키 명령과 그 배정 표.
//! 저장소에는 기본값과 다른 재배정만 `{"명령":"조합"}` JSON으로 남는다.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use super::key_event::{
    KeyTransition, ShortcutKeyEvent, match_key_transition, match_shortcut_event,
};
use super::notation::canonicalize_shortcut_for_platform;
use super::{
    TASK_DETAIL_DOCK_SHORTCUT_INDEXES, TERMINAL_TAB_SHORTCUT_INDEXES, defaults,
    task_detail_dock_shortcut, terminal_tab_shortcut,
};
use crate::platform::ShortcutPlatform;
use crate::task_detail_dock::{TASK_DETAIL_DOCK_ITEM_IDS, TaskDetailDockItemId};
use crate::terminal_tabs::TerminalTabShortcutCommand;

/// 단축키 명령. 선언 순서가 명령 정의 순서이자 판정 우선순위이고, 저장 JSON의 키 순서다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutCommandId {
    TaskDetailDock1,
    TaskDetailDock2,
    TaskDetailDock3,
    TaskDetailDock4,
    TaskDetailDock5,
    TaskDetailDock6,
    TaskDetailUsage,
    TaskSearch,
    BoardNotification,
    BoardProjectFilter,
    BoardDesktopFilter,
    CommandPalette,
    BoardPageFind,
    CreateTask,
    NewWindow,
    PageBack,
    PageForward,
    TerminalTabNew,
    TerminalTabClose,
    TerminalWindowClose,
    TerminalTabPrevious,
    TerminalTabNext,
    TerminalToggleChat,
    TerminalTab1,
    TerminalTab2,
    TerminalTab3,
    TerminalTab4,
    TerminalTab5,
}

impl ShortcutCommandId {
    pub fn task_detail_dock(index: u8) -> Option<Self> {
        const DOCK_COMMANDS: [ShortcutCommandId; 6] = [
            ShortcutCommandId::TaskDetailDock1,
            ShortcutCommandId::TaskDetailDock2,
            ShortcutCommandId::TaskDetailDock3,
            ShortcutCommandId::TaskDetailDock4,
            ShortcutCommandId::TaskDetailDock5,
            ShortcutCommandId::TaskDetailDock6,
        ];
        DOCK_COMMANDS
            .get(usize::from(index).checked_sub(1)?)
            .copied()
    }

    pub fn terminal_tab(index: u8) -> Option<Self> {
        const TAB_COMMANDS: [ShortcutCommandId; 5] = [
            ShortcutCommandId::TerminalTab1,
            ShortcutCommandId::TerminalTab2,
            ShortcutCommandId::TerminalTab3,
            ShortcutCommandId::TerminalTab4,
            ShortcutCommandId::TerminalTab5,
        ];
        TAB_COMMANDS
            .get(usize::from(index).checked_sub(1)?)
            .copied()
    }

    /// dock 명령이면 그 dock 자리 번호
    pub fn task_detail_dock_index(self) -> Option<u8> {
        TASK_DETAIL_DOCK_SHORTCUT_INDEXES
            .into_iter()
            .find(|&index| Self::task_detail_dock(index) == Some(self))
    }

    /// 터미널 탭 이동 명령이면 그 탭 자리 번호
    pub fn terminal_tab_position(self) -> Option<u8> {
        TERMINAL_TAB_SHORTCUT_INDEXES
            .into_iter()
            .find(|&index| Self::terminal_tab(index) == Some(self))
    }
}

/// 설정 화면에서 명령을 묶어 보여 주는 단위
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutCommandGroup {
    TaskDetailDock,
    TaskDetail,
    Board,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutCommandDefinition {
    pub id: ShortcutCommandId,
    pub group: ShortcutCommandGroup,
    pub default_shortcut: String,
    /// messages의 `settings.shortcuts.commands` 아래 라벨 키
    pub label_key: &'static str,
    /// 번호로만 구분되는 명령이 라벨에 끼워 넣을 숫자
    pub label_index: Option<u8>,
    /// 도크 명령이 가리키는 항목. 설정 화면이 번호 대신 항목 이름을 보여 주는 근거다
    pub dock_item_id: Option<TaskDetailDockItemId>,
}

/// 번호 없이 하나씩 정의된 명령: (명령, 묶음, 기본 단축키, 라벨 키)
const FIXED_COMMANDS: [(ShortcutCommandId, ShortcutCommandGroup, &str, &str); 17] = {
    use ShortcutCommandGroup::{Board, TaskDetail, Terminal};
    use ShortcutCommandId as Id;
    [
        (
            Id::TaskDetailUsage,
            TaskDetail,
            defaults::TASK_DETAIL_USAGE,
            "taskDetailUsage",
        ),
        (Id::TaskSearch, Board, defaults::TASK_SEARCH, "taskSearch"),
        (
            Id::BoardNotification,
            Board,
            defaults::BOARD_NOTIFICATION,
            "boardNotification",
        ),
        (
            Id::BoardProjectFilter,
            Board,
            defaults::BOARD_PROJECT_FILTER,
            "boardProjectFilter",
        ),
        (
            Id::BoardDesktopFilter,
            Board,
            defaults::BOARD_DESKTOP_FILTER,
            "boardDesktopFilter",
        ),
        (
            Id::CommandPalette,
            Board,
            defaults::COMMAND_PALETTE,
            "commandPalette",
        ),
        (
            Id::BoardPageFind,
            Board,
            defaults::BOARD_PAGE_FIND,
            "boardPageFind",
        ),
        (Id::CreateTask, Board, defaults::CREATE_TASK, "createTask"),
        (Id::NewWindow, Board, defaults::NEW_WINDOW, "newWindow"),
        (Id::PageBack, Board, defaults::PAGE_BACK, "pageBack"),
        (
            Id::PageForward,
            Board,
            defaults::PAGE_FORWARD,
            "pageForward",
        ),
        (
            Id::TerminalTabNew,
            Terminal,
            defaults::TERMINAL_TAB_NEW,
            "terminalTabNew",
        ),
        (
            Id::TerminalTabClose,
            Terminal,
            defaults::TERMINAL_TAB_CLOSE,
            "terminalTabClose",
        ),
        (
            Id::TerminalWindowClose,
            Terminal,
            defaults::TERMINAL_WINDOW_CLOSE,
            "terminalWindowClose",
        ),
        (
            Id::TerminalTabPrevious,
            Terminal,
            defaults::TERMINAL_TAB_PREVIOUS,
            "terminalTabPrevious",
        ),
        (
            Id::TerminalTabNext,
            Terminal,
            defaults::TERMINAL_TAB_NEXT,
            "terminalTabNext",
        ),
        (
            Id::TerminalToggleChat,
            Terminal,
            defaults::TERMINAL_TOGGLE_CHAT,
            "terminalToggleChat",
        ),
    ]
};

fn dock_command_definitions() -> impl Iterator<Item = ShortcutCommandDefinition> {
    let dock_slots = TASK_DETAIL_DOCK_SHORTCUT_INDEXES
        .into_iter()
        .zip(TASK_DETAIL_DOCK_ITEM_IDS);
    dock_slots.filter_map(|(index, dock_item_id)| {
        Some(ShortcutCommandDefinition {
            id: ShortcutCommandId::task_detail_dock(index)?,
            group: ShortcutCommandGroup::TaskDetailDock,
            default_shortcut: task_detail_dock_shortcut(index),
            label_key: "taskDetailDock",
            label_index: Some(index),
            dock_item_id: Some(dock_item_id),
        })
    })
}

fn fixed_command_definitions() -> impl Iterator<Item = ShortcutCommandDefinition> {
    FIXED_COMMANDS
        .into_iter()
        .map(
            |(id, group, default_shortcut, label_key)| ShortcutCommandDefinition {
                id,
                group,
                default_shortcut: default_shortcut.to_string(),
                label_key,
                label_index: None,
                dock_item_id: None,
            },
        )
}

fn terminal_tab_command_definitions() -> impl Iterator<Item = ShortcutCommandDefinition> {
    TERMINAL_TAB_SHORTCUT_INDEXES
        .into_iter()
        .filter_map(|index| {
            Some(ShortcutCommandDefinition {
                id: ShortcutCommandId::terminal_tab(index)?,
                group: ShortcutCommandGroup::Terminal,
                default_shortcut: terminal_tab_shortcut(index),
                label_key: "terminalTab",
                label_index: Some(index),
                dock_item_id: None,
            })
        })
}

fn build_command_definitions() -> Vec<ShortcutCommandDefinition> {
    dock_command_definitions()
        .chain(fixed_command_definitions())
        .chain(terminal_tab_command_definitions())
        .collect()
}

static SHORTCUT_COMMAND_DEFINITIONS: LazyLock<Vec<ShortcutCommandDefinition>> =
    LazyLock::new(build_command_definitions);

/// 명령 정의 목록. 같은 조합이 여러 명령에 걸리면 이 순서에서 앞선 명령이 이긴다
pub fn shortcut_command_definitions() -> &'static [ShortcutCommandDefinition] {
    &SHORTCUT_COMMAND_DEFINITIONS
}

/// 기본값과 다른 재배정만 담은 표. 키 순서가 명령 정의 순서라 저장 JSON도 그 순서를 따른다
pub type ShortcutOverrides = BTreeMap<ShortcutCommandId, String>;

/// 모든 명령의 최종 단축키 표
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutBindings {
    shortcuts: BTreeMap<ShortcutCommandId, String>,
}

impl ShortcutBindings {
    pub fn defaults() -> Self {
        Self::with_overrides(&ShortcutOverrides::new())
    }

    /// 기본값 위에 재배정을 얹은 최종 표
    pub fn with_overrides(overrides: &ShortcutOverrides) -> Self {
        let mut shortcuts: BTreeMap<ShortcutCommandId, String> = shortcut_command_definitions()
            .iter()
            .map(|definition| (definition.id, definition.default_shortcut.clone()))
            .collect();
        shortcuts.extend(
            overrides
                .iter()
                .map(|(&id, shortcut)| (id, shortcut.clone())),
        );
        Self { shortcuts }
    }

    pub fn shortcut(&self, command_id: ShortcutCommandId) -> &str {
        self.shortcuts
            .get(&command_id)
            .map(String::as_str)
            .unwrap_or_default()
    }

    pub fn set_shortcut(&mut self, command_id: ShortcutCommandId, shortcut: impl Into<String>) {
        self.shortcuts.insert(command_id, shortcut.into());
    }

    fn find_command(&self, is_match: impl Fn(&str) -> bool) -> Option<ShortcutCommandId> {
        shortcut_command_definitions()
            .iter()
            .map(|definition| definition.id)
            .find(|&command_id| is_match(self.shortcut(command_id)))
    }

    /// 화면 안 키 입력이 어떤 명령인지 판정한다
    pub fn find_command_for_event(
        &self,
        event: &ShortcutKeyEvent,
        platform: ShortcutPlatform,
    ) -> Option<ShortcutCommandId> {
        self.find_command(|shortcut| match_shortcut_event(event, shortcut, platform))
    }

    /// 터미널보다 먼저 가로챈 입력이 어떤 명령인지 판정한다
    pub fn find_command_for_transition(
        &self,
        transition: &KeyTransition,
        platform: ShortcutPlatform,
    ) -> Option<ShortcutCommandId> {
        self.find_command(|shortcut| match_key_transition(transition, shortcut, platform))
    }

    /// 이미 같은 조합을 쓰고 있는 다른 명령
    pub fn find_conflict(
        &self,
        command_id: ShortcutCommandId,
        shortcut: &str,
        platform: ShortcutPlatform,
    ) -> Option<ShortcutCommandId> {
        let canonical_shortcut = canonicalize_shortcut_for_platform(shortcut, platform);
        shortcut_command_definitions()
            .iter()
            .map(|definition| definition.id)
            .filter(|&other_id| other_id != command_id)
            .find(|&other_id| {
                canonicalize_shortcut_for_platform(self.shortcut(other_id), platform)
                    == canonical_shortcut
            })
    }

    /// 기본값과 같아진 재배정은 빼고 남긴다. 그래야 나중에 기본값을 바꿔도 옛 값이 발목을 잡지 않는다
    pub fn collect_overrides(&self, platform: ShortcutPlatform) -> ShortcutOverrides {
        shortcut_command_definitions()
            .iter()
            .filter_map(|definition| {
                let canonical_shortcut =
                    canonicalize_shortcut_for_platform(self.shortcut(definition.id), platform);
                let canonical_default =
                    canonicalize_shortcut_for_platform(&definition.default_shortcut, platform);
                let is_override =
                    !canonical_shortcut.is_empty() && canonical_shortcut != canonical_default;
                is_override.then_some((definition.id, canonical_shortcut))
            })
            .collect()
    }
}

/// 저장된 재배정을 읽는다. 모르는 명령, 문자열이 아닌 값, 키가 없는 조합은 버려 기본값이 남게 한다.
/// 저장 형식이 깨져 있어도 단축키를 통째로 잃지 않도록 파싱 실패는 빈 재배정으로 본다.
/// 옛 버전이 남긴 `Meta+…`/`Ctrl+…` 표기는 읽는 순간 정규 표기로 접는다.
pub fn parse_shortcut_overrides(
    raw_value: Option<&str>,
    platform: ShortcutPlatform,
) -> ShortcutOverrides {
    let Some(raw_value) = raw_value.filter(|value| !value.is_empty()) else {
        return ShortcutOverrides::new();
    };
    let Ok(serde_json::Value::Object(stored_entries)) = serde_json::from_str(raw_value) else {
        return ShortcutOverrides::new();
    };

    stored_entries
        .into_iter()
        .filter_map(|(command_name, stored_shortcut)| {
            let command_id: ShortcutCommandId =
                serde_json::from_value(serde_json::Value::String(command_name)).ok()?;
            let canonical_shortcut =
                canonicalize_shortcut_for_platform(stored_shortcut.as_str()?, platform);
            (!canonical_shortcut.is_empty()).then_some((command_id, canonical_shortcut))
        })
        .collect()
}

/// 단축키 명령을 터미널 탭 명령으로 바꾼다. 창 닫기는 어느 화면에서나, 나머지는 터미널이 있는 작업 상세에서만 의미가 있다.
/// 탭 닫기는 터미널 밖에서는 일반 앱처럼 창 닫기로 동작한다.
pub fn resolve_terminal_tab_command(
    command_id: Option<ShortcutCommandId>,
    is_task_detail_route: bool,
) -> Option<TerminalTabShortcutCommand> {
    use ShortcutCommandId as Id;
    use TerminalTabShortcutCommand as Command;

    let command_id = command_id?;
    match command_id {
        Id::TerminalWindowClose => return Some(Command::CloseWindow),
        Id::TerminalTabClose if is_task_detail_route => return Some(Command::CloseTab),
        Id::TerminalTabClose => return Some(Command::CloseWindow),
        _ if !is_task_detail_route => return None,
        _ => {}
    }

    match command_id {
        Id::TerminalTabNew => Some(Command::NewTab),
        Id::TerminalTabPrevious => Some(Command::PreviousTab),
        Id::TerminalTabNext => Some(Command::NextTab),
        other => other
            .terminal_tab_position()
            .map(|position| Command::GoToTab { position }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::key_event::test_support::{key_down, key_event};

    const MAC: ShortcutPlatform = ShortcutPlatform::Mac;
    const LINUX: ShortcutPlatform = ShortcutPlatform::Linux;

    fn dock_index_for(event: &ShortcutKeyEvent, platform: ShortcutPlatform) -> Option<u8> {
        ShortcutBindings::defaults()
            .find_command_for_event(event, platform)?
            .task_detail_dock_index()
    }

    fn tab_position_for(event: &ShortcutKeyEvent, platform: ShortcutPlatform) -> Option<u8> {
        ShortcutBindings::defaults()
            .find_command_for_event(event, platform)?
            .terminal_tab_position()
    }

    fn tab_command_for_transition(
        transition: &KeyTransition,
        platform: ShortcutPlatform,
        is_task_detail_route: bool,
    ) -> Option<TerminalTabShortcutCommand> {
        let command_id =
            ShortcutBindings::defaults().find_command_for_transition(transition, platform);
        resolve_terminal_tab_command(command_id, is_task_detail_route)
    }

    fn tab_command_for_event(
        event: &ShortcutKeyEvent,
        platform: ShortcutPlatform,
        is_task_detail_route: bool,
    ) -> Option<TerminalTabShortcutCommand> {
        let command_id = ShortcutBindings::defaults().find_command_for_event(event, platform);
        resolve_terminal_tab_command(command_id, is_task_detail_route)
    }

    fn overrides_from(entries: &[(ShortcutCommandId, &str)]) -> ShortcutOverrides {
        entries
            .iter()
            .map(|&(id, shortcut)| (id, shortcut.to_string()))
            .collect()
    }

    fn defaults_with(command_id: ShortcutCommandId, shortcut: &str) -> ShortcutBindings {
        let mut bindings = ShortcutBindings::defaults();
        bindings.set_shortcut(command_id, shortcut);
        bindings
    }

    #[test]
    fn tab_number_shortcuts_return_the_position_without_hitting_the_dock() {
        for index in TERMINAL_TAB_SHORTCUT_INDEXES {
            let mac_event = key_event(&index.to_string(), "meta+alt");
            assert_eq!(tab_position_for(&mac_event, MAC), Some(index));
            assert_eq!(dock_index_for(&mac_event, MAC), None);
        }
    }

    #[test]
    fn dock_number_input_is_not_a_tab_number() {
        let dock_event = key_event("1", "ctrl");

        assert_eq!(dock_index_for(&dock_event, LINUX), Some(1));
        assert_eq!(tab_position_for(&dock_event, LINUX), None);
    }

    #[test]
    fn macos_dock_input_without_option_is_not_a_tab_number() {
        let dock_event = key_event("1", "meta");

        assert_eq!(dock_index_for(&dock_event, MAC), Some(1));
        assert_eq!(tab_position_for(&dock_event, MAC), None);
    }

    #[test]
    fn transitions_resolve_tab_numbers() {
        let command_id = ShortcutBindings::defaults()
            .find_command_for_transition(&key_down("3", "ctrl+alt"), LINUX);

        assert_eq!(
            command_id.and_then(ShortcutCommandId::terminal_tab_position),
            Some(3)
        );
    }

    #[test]
    fn dock_matcher_returns_the_dock_number() {
        assert_eq!(dock_index_for(&key_event("4", "meta"), MAC), Some(4));
        assert_eq!(dock_index_for(&key_event("4", "ctrl"), LINUX), Some(4));
        assert_eq!(dock_index_for(&key_event("4", "alt"), LINUX), None);
    }

    #[test]
    fn task_detail_creates_new_previous_and_next_tab_commands() {
        use TerminalTabShortcutCommand::{NewTab, NextTab, PreviousTab};
        assert_eq!(
            tab_command_for_transition(&key_down("t", "meta"), MAC, true),
            Some(NewTab)
        );
        assert_eq!(
            tab_command_for_transition(&key_down("{", "meta+shift"), MAC, true),
            Some(PreviousTab)
        );
        assert_eq!(
            tab_command_for_transition(&key_down("}", "meta+shift"), MAC, true),
            Some(NextTab)
        );
    }

    #[test]
    fn number_shortcut_carries_the_tab_position() {
        assert_eq!(
            tab_command_for_transition(&key_down("3", "meta+alt"), MAC, true),
            Some(TerminalTabShortcutCommand::GoToTab { position: 3 })
        );
    }

    #[test]
    fn tab_close_closes_the_window_outside_task_detail() {
        assert_eq!(
            tab_command_for_transition(&key_down("w", "meta"), MAC, true),
            Some(TerminalTabShortcutCommand::CloseTab)
        );
        assert_eq!(
            tab_command_for_transition(&key_down("w", "meta"), MAC, false),
            Some(TerminalTabShortcutCommand::CloseWindow)
        );
    }

    #[test]
    fn window_close_works_on_every_screen() {
        assert_eq!(
            tab_command_for_transition(&key_down("w", "meta+shift"), MAC, false),
            Some(TerminalTabShortcutCommand::CloseWindow)
        );
    }

    #[test]
    fn tab_commands_need_task_detail() {
        assert_eq!(
            tab_command_for_transition(&key_down("t", "meta"), MAC, false),
            None
        );
        assert_eq!(
            tab_command_for_transition(&key_down("3", "meta+shift"), MAC, false),
            None
        );
    }

    #[test]
    fn page_navigation_does_not_leak_into_tab_commands() {
        assert_eq!(
            tab_command_for_transition(&key_down("[", "meta"), MAC, true),
            None
        );
        assert_eq!(
            tab_command_for_transition(&key_down("]", "meta"), MAC, true),
            None
        );
    }

    #[test]
    fn events_and_transitions_create_the_same_command() {
        let new_tab = Some(TerminalTabShortcutCommand::NewTab);

        assert_eq!(
            tab_command_for_event(&key_event("t", "ctrl"), LINUX, true),
            new_tab
        );
        assert_eq!(
            tab_command_for_transition(&key_down("t", "ctrl"), LINUX, true),
            new_tab
        );
    }

    #[test]
    fn event_path_creates_no_tab_command_outside_task_detail() {
        assert_eq!(
            tab_command_for_event(&key_event("t", "ctrl"), LINUX, false),
            None
        );
    }

    #[test]
    fn event_path_resolves_tab_numbers_and_window_close() {
        assert_eq!(
            tab_command_for_event(&key_event("2", "ctrl+alt"), LINUX, true),
            Some(TerminalTabShortcutCommand::GoToTab { position: 2 })
        );
        assert_eq!(
            tab_command_for_event(&key_event("w", "ctrl+shift"), LINUX, false),
            Some(TerminalTabShortcutCommand::CloseWindow)
        );
    }

    #[test]
    fn mod_zero_is_the_usage_shortcut() {
        let bindings = ShortcutBindings::defaults();

        assert_eq!(
            bindings.find_command_for_event(&key_event("0", "ctrl"), LINUX),
            Some(ShortcutCommandId::TaskDetailUsage)
        );
        assert_eq!(
            bindings.find_command_for_event(&key_event("0", "meta"), MAC),
            Some(ShortcutCommandId::TaskDetailUsage)
        );
    }

    #[test]
    fn usage_shortcut_does_not_overlap_the_dock() {
        assert!(!TASK_DETAIL_DOCK_SHORTCUT_INDEXES.contains(&0));
        assert_eq!(dock_index_for(&key_event("0", "ctrl"), LINUX), None);
    }

    #[test]
    fn usage_shortcut_does_not_overlap_tab_numbers() {
        let command_id =
            ShortcutBindings::defaults().find_command_for_event(&key_event("0", "ctrl+alt"), LINUX);

        assert_ne!(command_id, Some(ShortcutCommandId::TaskDetailUsage));
    }

    #[test]
    fn command_palette_and_project_filter_shortcuts() {
        let bindings = ShortcutBindings::defaults();

        assert_eq!(
            bindings.find_command_for_event(&key_event("p", "meta+shift"), MAC),
            Some(ShortcutCommandId::CommandPalette)
        );
        assert_eq!(
            bindings.find_command_for_event(&key_event("f", "meta+shift"), MAC),
            Some(ShortcutCommandId::BoardProjectFilter)
        );
    }

    #[test]
    fn desktop_filter_opens_on_both_platforms() {
        let bindings = ShortcutBindings::defaults();

        assert_eq!(
            bindings.shortcut(ShortcutCommandId::BoardDesktopFilter),
            "Mod+Shift+D"
        );
        assert_eq!(
            bindings.find_command_for_event(&key_event("d", "meta+shift"), MAC),
            Some(ShortcutCommandId::BoardDesktopFilter)
        );
        assert_eq!(
            bindings.find_command_for_event(&key_event("D", "ctrl+shift"), LINUX),
            Some(ShortcutCommandId::BoardDesktopFilter)
        );
    }

    #[test]
    fn dock_commands_exist_only_for_real_dock_items() {
        let dock_item_ids: Vec<Option<TaskDetailDockItemId>> = shortcut_command_definitions()
            .iter()
            .filter(|definition| definition.group == ShortcutCommandGroup::TaskDetailDock)
            .map(|definition| definition.dock_item_id)
            .collect();
        let dock_count = TASK_DETAIL_DOCK_ITEM_IDS.len();

        assert_eq!(dock_item_ids, TASK_DETAIL_DOCK_ITEM_IDS.map(Some));
        assert_eq!(
            dock_index_for(&key_event(&dock_count.to_string(), "ctrl"), LINUX),
            Some(6)
        );
        let beyond_last = key_event(&(dock_count + 1).to_string(), "ctrl");
        assert_eq!(
            ShortcutBindings::defaults().find_command_for_event(&beyond_last, LINUX),
            None
        );
    }

    #[test]
    fn default_table_has_no_duplicate_combinations() {
        let mut used_shortcuts = std::collections::HashSet::new();

        for definition in shortcut_command_definitions() {
            assert!(
                used_shortcuts.insert(definition.default_shortcut.clone()),
                "{definition:?}"
            );
        }
        assert_eq!(used_shortcuts.len(), 28);
    }

    #[test]
    fn reassigned_combination_resolves_the_command() {
        let bindings = ShortcutBindings::with_overrides(&overrides_from(&[(
            ShortcutCommandId::TaskDetailDock4,
            "Mod+Shift+K",
        )]));
        let command_id = bindings.find_command_for_event(&key_event("k", "ctrl+shift"), LINUX);

        assert_eq!(
            command_id.and_then(ShortcutCommandId::task_detail_dock_index),
            Some(4)
        );
        assert_eq!(dock_index_for(&key_event("4", "ctrl"), LINUX), Some(4));
    }

    #[test]
    fn unknown_commands_and_non_string_values_are_dropped() {
        let stored =
            r#"{"taskDetailDock1":"Mod+Shift+K","unknownCommand":"Mod+J","taskDetailUsage":7}"#;

        assert_eq!(
            parse_shortcut_overrides(Some(stored), LINUX),
            overrides_from(&[(ShortcutCommandId::TaskDetailDock1, "Mod+Shift+K")])
        );
    }

    #[test]
    fn broken_storage_falls_back_to_defaults() {
        assert!(parse_shortcut_overrides(Some("{not json"), LINUX).is_empty());
        assert_eq!(
            ShortcutBindings::with_overrides(&parse_shortcut_overrides(None, LINUX)),
            ShortcutBindings::defaults()
        );
    }

    #[test]
    fn entries_equal_to_the_default_are_not_stored() {
        let bindings = defaults_with(ShortcutCommandId::TaskDetailDock1, "Mod+Shift+K");

        assert_eq!(
            bindings.collect_overrides(LINUX),
            overrides_from(&[(ShortcutCommandId::TaskDetailDock1, "Mod+Shift+K")])
        );
    }

    #[test]
    fn combination_used_by_another_command_is_a_conflict() {
        let bindings = ShortcutBindings::defaults();
        use ShortcutCommandId::{CreateTask, TaskDetailDock1};

        assert_eq!(
            bindings.find_conflict(TaskDetailDock1, "Mod+N", LINUX),
            Some(CreateTask)
        );
        assert_eq!(bindings.find_conflict(CreateTask, "Mod+N", LINUX), None);
        assert_eq!(
            bindings.find_conflict(TaskDetailDock1, "Mod+Shift+K", LINUX),
            None
        );
    }

    #[test]
    fn legacy_platform_notation_is_the_same_combination() {
        use ShortcutCommandId::{BoardProjectFilter, TaskSearch};
        let mac_bindings = ShortcutBindings::with_overrides(&parse_shortcut_overrides(
            Some(r#"{"taskSearch":"Meta+Shift+O"}"#),
            MAC,
        ));
        assert_eq!(mac_bindings.shortcut(TaskSearch), "Mod+Shift+O");
        assert_eq!(
            mac_bindings.find_conflict(BoardProjectFilter, "Mod+Shift+O", MAC),
            Some(TaskSearch)
        );

        let linux_bindings = ShortcutBindings::with_overrides(&parse_shortcut_overrides(
            Some(r#"{"taskSearch":"Ctrl+Shift+O"}"#),
            LINUX,
        ));
        assert_eq!(linux_bindings.shortcut(TaskSearch), "Mod+Shift+O");
        assert_eq!(
            linux_bindings.find_conflict(BoardProjectFilter, "Mod+Shift+O", LINUX),
            Some(TaskSearch)
        );
    }

    #[test]
    fn non_mod_platform_modifiers_are_not_folded() {
        use ShortcutCommandId::BoardProjectFilter;
        let mac_bindings = ShortcutBindings::with_overrides(&parse_shortcut_overrides(
            Some(r#"{"taskSearch":"Ctrl+Shift+O"}"#),
            MAC,
        ));
        let linux_bindings = ShortcutBindings::with_overrides(&parse_shortcut_overrides(
            Some(r#"{"taskSearch":"Meta+Shift+O"}"#),
            LINUX,
        ));

        assert_eq!(
            mac_bindings.find_conflict(BoardProjectFilter, "Mod+Shift+O", MAC),
            None
        );
        assert_eq!(
            linux_bindings.find_conflict(BoardProjectFilter, "Mod+Shift+O", LINUX),
            None
        );
    }

    #[test]
    fn default_written_in_platform_notation_is_not_an_override() {
        let mac_bindings = defaults_with(ShortcutCommandId::TaskSearch, "Meta+Shift+O");
        let linux_bindings = defaults_with(ShortcutCommandId::TaskSearch, "Ctrl+Shift+O");

        assert!(mac_bindings.collect_overrides(MAC).is_empty());
        assert!(linux_bindings.collect_overrides(LINUX).is_empty());
    }

    #[test]
    fn chat_toggle_matches_the_same_way_for_events_and_transitions() {
        let bindings = ShortcutBindings::defaults();
        for (platform, mod_name) in [(MAC, "meta"), (LINUX, "ctrl")] {
            let toggle = Some(ShortcutCommandId::TerminalToggleChat);
            let with_shift = format!("{mod_name}+shift");

            assert_eq!(
                bindings.find_command_for_event(&key_event("J", &with_shift), platform),
                toggle
            );
            assert_eq!(
                bindings.find_command_for_transition(&key_down("J", &with_shift), platform),
                toggle
            );
            assert_ne!(
                bindings.find_command_for_event(&key_event("j", mod_name), platform),
                toggle
            );
        }
    }

    #[test]
    fn overrides_serialize_in_command_definition_order() {
        let overrides = overrides_from(&[
            (ShortcutCommandId::CreateTask, "Mod+Alt+N"),
            (ShortcutCommandId::TaskDetailDock4, "Mod+Shift+K"),
        ]);

        assert_eq!(
            serde_json::to_string(&overrides).unwrap(),
            r#"{"taskDetailDock4":"Mod+Shift+K","createTask":"Mod+Alt+N"}"#
        );
    }
}
