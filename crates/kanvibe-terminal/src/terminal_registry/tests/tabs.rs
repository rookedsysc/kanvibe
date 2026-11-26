//! TS `terminalSessionLifecycle.test.ts` "terminal 세션 탭 레지스트리"

use std::sync::{Arc, Mutex, OnceLock, Weak};

use kanvibe_domain::task::SessionType;
use kanvibe_domain::terminal_tabs::TerminalTab;

use super::super::entry::{self, TerminalEntry};
use super::super::{ClientClosure, TerminalClient, TerminalRegistry};
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

/// 붙는 도중(화면 직렬화 뒤, 등록 직전)에 레지스트리를 들여다보는 모바일 클라이언트
struct AttachProbeClient {
    registry: OnceLock<Weak<TerminalRegistry>>,
    should_remove_entry_while_attaching: bool,
    was_registry_lock_free: Mutex<Option<bool>>,
    removed_entry: Mutex<Option<Arc<TerminalEntry>>>,
}

impl AttachProbeClient {
    fn new(
        registry: &Arc<TerminalRegistry>,
        should_remove_entry_while_attaching: bool,
    ) -> Arc<Self> {
        let client = Arc::new(Self {
            registry: OnceLock::new(),
            should_remove_entry_while_attaching,
            was_registry_lock_free: Mutex::new(None),
            removed_entry: Mutex::new(None),
        });
        let _ = client.registry.set(Arc::downgrade(registry));
        client
    }
}

impl TerminalClient for AttachProbeClient {
    fn is_open(&self) -> bool {
        let registry = self.registry.get().and_then(Weak::upgrade);
        let Some(registry) = registry else {
            return true;
        };
        let state = registry.state.try_lock();
        let is_lock_free = state.is_ok();
        if let Ok(mut state) = state
            && self.should_remove_entry_while_attaching
        {
            *self.removed_entry.lock().unwrap() = state.active_terminals.remove("task-1#task-1-1");
        }
        self.was_registry_lock_free
            .lock()
            .unwrap()
            .get_or_insert(is_lock_free);
        true
    }

    fn send(&self, _data: &str) {}

    fn buffered_amount(&self) -> usize {
        0
    }

    fn close(&self, _closure: Option<ClientClosure>) {}
}

#[test]
fn mobile_attach_serializes_the_screen_without_holding_the_registry_lock() {
    // Given
    let harness = Harness::new();
    attach_tab(&harness, "task-1-1");
    harness.ptys.pty(0).emit_output("scrollback");
    let probe = AttachProbeClient::new(&harness.registry, false);

    // When
    let attached =
        harness
            .registry
            .attach_client_to_terminal_tab("task-1", "task-1-1", probe.clone());

    // Then
    let (_, screen_until_now) = attached.unwrap();
    assert!(screen_until_now.contains("scrollback"));
    assert_eq!(*probe.was_registry_lock_free.lock().unwrap(), Some(true));
}

#[test]
fn mobile_attach_is_undone_when_the_tab_entry_is_replaced_while_attaching() {
    // Given
    let harness = Harness::new();
    attach_tab(&harness, "task-1-1");
    let probe = AttachProbeClient::new(&harness.registry, true);

    // When
    let attached =
        harness
            .registry
            .attach_client_to_terminal_tab("task-1", "task-1-1", probe.clone());

    // Then
    assert!(attached.is_none());
    let removed_entry = probe.removed_entry.lock().unwrap().clone().unwrap();
    let remaining_clients = entry::lock(&removed_entry.output.sink).clients.len();
    assert_eq!(remaining_clients, 1, "탭을 연 첫 클라이언트만 남는다");
}
