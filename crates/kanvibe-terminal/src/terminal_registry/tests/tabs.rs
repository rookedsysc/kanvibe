//! TS `terminalSessionLifecycle.test.ts` "terminal 세션 탭 레지스트리"

use kanvibe_domain::task::SessionType;
use kanvibe_domain::terminal_tabs::TerminalTab;

use super::fakes::*;

fn attach_tab(harness: &Harness, tab_id: &str) {
    let request = local_request(
        "task-1",
        Some(tab_id),
        SessionType::Terminal,
        "proj",
        Some("/work"),
    );
    harness.attach_local(request, &FakeClient::new()).unwrap();
}

fn tab_ids(harness: &Harness) -> Vec<String> {
    harness
        .registry
        .list_local_terminal_tabs("task-1")
        .into_iter()
        .map(|tab| tab.id)
        .collect()
}

// parity: TS-lib-terminalSessionLifecycle-454795
#[test]
fn first_listing_creates_one_active_tab() {
    // Given
    let harness = Harness::new();

    // When
    let tabs = harness.registry.list_local_terminal_tabs("task-1");

    // Then
    assert_eq!(
        tabs,
        vec![TerminalTab {
            id: "task-1-1".to_string(),
            native_index: 0,
            name: "zsh".to_string(),
            is_active: true,
        }]
    );
}

// parity: TS-lib-terminalSessionLifecycle-eec74d
#[test]
fn new_tab_is_appended_and_becomes_active() {
    // Given
    let harness = Harness::new();

    // When
    let created_tab = harness.registry.create_local_terminal_tab("task-1");
    let tabs = harness.registry.list_local_terminal_tabs("task-1");

    // Then
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[1].id, created_tab.id);
    let active_ids: Vec<&str> = tabs
        .iter()
        .filter(|tab| tab.is_active)
        .map(|tab| tab.id.as_str())
        .collect();
    assert_eq!(active_ids, vec![created_tab.id.as_str()]);
}

// parity: TS-lib-terminalSessionLifecycle-e8e3cf
#[test]
fn closing_a_tab_kills_only_its_pty_and_reports_remaining_count() {
    // Given
    let harness = Harness::new();
    let first_tab = harness.registry.list_local_terminal_tabs("task-1")[0].clone();
    let second_tab = harness.registry.create_local_terminal_tab("task-1");
    attach_tab(&harness, &first_tab.id);
    attach_tab(&harness, &second_tab.id);

    // When
    let remaining_count = harness
        .registry
        .close_local_terminal_tab("task-1", &first_tab.id);

    // Then
    assert_eq!(remaining_count, 1);
    assert_eq!(harness.ptys.pty(0).kill_count(), 1);
    assert_eq!(harness.ptys.pty(1).kill_count(), 0);
}

// parity: TS-lib-terminalSessionLifecycle-07c0d3
#[test]
fn closing_the_active_tab_activates_its_successor() {
    // Given
    let harness = Harness::new();
    let first_tab = harness.registry.list_local_terminal_tabs("task-1")[0].clone();
    let second_tab = harness.registry.create_local_terminal_tab("task-1");
    harness
        .registry
        .select_local_terminal_tab("task-1", &first_tab.id);

    // When
    harness
        .registry
        .close_local_terminal_tab("task-1", &first_tab.id);

    // Then
    assert_eq!(
        harness.registry.list_local_terminal_tabs("task-1"),
        vec![TerminalTab {
            id: second_tab.id,
            native_index: 0,
            name: second_tab.name,
            is_active: true,
        }]
    );
}

// parity: TS-lib-terminalSessionLifecycle-f33724
#[test]
fn tab_disappears_when_its_shell_exits_by_itself() {
    // Given
    let harness = Harness::new();
    let first_tab = harness.registry.list_local_terminal_tabs("task-1")[0].clone();
    let second_tab = harness.registry.create_local_terminal_tab("task-1");
    attach_tab(&harness, &first_tab.id);
    attach_tab(&harness, &second_tab.id);

    // When
    harness.ptys.pty(0).emit_exit(0, false);

    // Then
    assert_eq!(tab_ids(&harness), vec![second_tab.id]);
}

// parity: TS-lib-terminalSessionLifecycle-37ab53
#[test]
fn rename_and_move_are_reflected_in_the_listing() {
    // Given
    let harness = Harness::new();
    let first_tab = harness.registry.list_local_terminal_tabs("task-1")[0].clone();
    let second_tab = harness.registry.create_local_terminal_tab("task-1");

    // When
    harness
        .registry
        .rename_local_terminal_tab("task-1", &first_tab.id, "빌드");
    harness
        .registry
        .move_local_terminal_tab("task-1", &first_tab.id, 1);

    // Then
    let listed: Vec<(String, i64, String)> = harness
        .registry
        .list_local_terminal_tabs("task-1")
        .into_iter()
        .map(|tab| (tab.id, tab.native_index, tab.name))
        .collect();
    assert_eq!(
        listed,
        vec![
            (second_tab.id, 0, second_tab.name),
            (first_tab.id, 1, "빌드".to_string()),
        ]
    );
}

// parity: TS-lib-terminalSessionLifecycle-17bd5a
#[test]
fn moving_to_an_out_of_range_index_keeps_the_order() {
    // Given
    let harness = Harness::new();
    let first_tab = harness.registry.list_local_terminal_tabs("task-1")[0].clone();

    // When
    harness
        .registry
        .move_local_terminal_tab("task-1", &first_tab.id, 5);

    // Then
    assert_eq!(tab_ids(&harness), vec![first_tab.id]);
}
