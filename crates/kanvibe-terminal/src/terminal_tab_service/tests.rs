use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use kanvibe_db::repositories::tasks::{NewKanbanTask, insert_task};
use kanvibe_db::{Database, DatabaseLocation};
use kanvibe_domain::task::{SessionType, TaskStatus};
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_process::exec::ExecGitError;
use serde_json::{Value, json};

use super::{
    TerminalTabClosePreview, TerminalTabHerdr, TerminalTabListResult, TerminalTabMutationResult,
    TerminalTabRegistry, TerminalTabService, TerminalTabServiceDependencies,
};
use crate::herdr::HerdrError;
use crate::pane_mirror::MirrorPane;
use crate::terminal_tabs::ZellijIdTargetingSupportCache;
use crate::test_support::ScriptedShellRunner;

/// TS `@/lib/terminal` mock. 원격 목록·닫기도 로컬 응답을 그대로 쓴다
#[derive(Default)]
struct FakeRegistry {
    local_tabs: Mutex<Vec<TerminalTab>>,
    local_process_name: Mutex<Option<String>>,
    herdr_active_tab_id: Mutex<Option<String>>,
    calls: Mutex<Vec<String>>,
}

impl FakeRegistry {
    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn has_call_starting_with(&self, prefix: &str) -> bool {
        self.calls().iter().any(|call| call.starts_with(prefix))
    }
}

impl TerminalTabRegistry for FakeRegistry {
    fn list_local_terminal_tabs(&self, _task_id: &str) -> Vec<TerminalTab> {
        self.local_tabs.lock().unwrap().clone()
    }

    fn list_remote_terminal_tabs(
        &self,
        task_id: &str,
        _ssh_host: &str,
        _session_name: &str,
    ) -> Result<Vec<TerminalTab>, String> {
        Ok(self.list_local_terminal_tabs(task_id))
    }

    fn close_remote_terminal_tab(
        &self,
        task_id: &str,
        tab_id: &str,
        _ssh_host: &str,
        _session_name: &str,
    ) -> Result<usize, String> {
        Ok(self.close_local_terminal_tab(task_id, tab_id))
    }

    fn close_local_terminal_tab(&self, task_id: &str, tab_id: &str) -> usize {
        self.record(format!("close_local {task_id} {tab_id}"));
        2
    }

    fn local_terminal_tab_process(&self, task_id: &str, tab_id: &str) -> Option<String> {
        self.record(format!("local_process {task_id} {tab_id}"));
        self.local_process_name.lock().unwrap().clone()
    }

    fn create_local_terminal_tab(&self, task_id: &str) {
        self.record(format!("create_local {task_id}"));
    }

    fn select_local_terminal_tab(&self, task_id: &str, tab_id: &str) {
        self.record(format!("select_local {task_id} {tab_id}"));
    }

    fn rename_local_terminal_tab(&self, task_id: &str, tab_id: &str, name: &str) {
        self.record(format!("rename_local {task_id} {tab_id} {name}"));
    }

    fn move_local_terminal_tab(&self, task_id: &str, tab_id: &str, target_index: usize) {
        self.record(format!("move_local {task_id} {tab_id} {target_index}"));
    }

    fn herdr_task_active_tab_id(&self, _task_id: &str) -> Option<String> {
        self.herdr_active_tab_id.lock().unwrap().clone()
    }

    fn focus_herdr_task_tab(&self, task_id: &str, tab_id: &str) -> Result<(), String> {
        self.record(format!("focus_herdr {task_id} {tab_id}"));
        Ok(())
    }
}

/// Herdr 이동 요청 하나(세션, 탭, 삽입 경계, 호스트)
type HerdrMove = (String, String, i64, Option<String>);

/// TS 테스트가 execGit mock 뒤의 실제 `@/lib/herdr`로 확인하던 Herdr 동작의 가짜
#[derive(Default)]
struct FakeHerdr {
    tabs: Mutex<Vec<TerminalTab>>,
    mirror_panes: Mutex<Vec<MirrorPane>>,
    created_tab_id: Mutex<Option<String>>,
    tab_reads: Mutex<Vec<(String, Option<String>)>>,
    commands: Mutex<Vec<(Vec<String>, Option<String>)>>,
    moves: Mutex<Vec<HerdrMove>>,
}

impl FakeHerdr {
    fn command_arguments(&self) -> Vec<Vec<String>> {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .map(|(arguments, _)| arguments.clone())
            .collect()
    }

    fn has_command(&self, arguments: &[&str]) -> bool {
        self.command_arguments()
            .iter()
            .any(|recorded| recorded == arguments)
    }

    fn has_command_starting_with(&self, prefix: &[&str]) -> bool {
        self.command_arguments().iter().any(|recorded| {
            recorded.starts_with(&prefix.iter().map(ToString::to_string).collect::<Vec<_>>())
        })
    }

    fn moves(&self) -> Vec<HerdrMove> {
        self.moves.lock().unwrap().clone()
    }
}

impl TerminalTabHerdr for FakeHerdr {
    fn read_tabs(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<TerminalTab>, HerdrError> {
        self.tab_reads
            .lock()
            .unwrap()
            .push((session_name.to_string(), ssh_host.map(str::to_string)));
        Ok(self.tabs.lock().unwrap().clone())
    }

    fn run_command(
        &self,
        _session_name: &str,
        arguments: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<Value, HerdrError> {
        let recorded: Vec<String> = arguments.iter().map(ToString::to_string).collect();
        self.commands
            .lock()
            .unwrap()
            .push((recorded, ssh_host.map(str::to_string)));
        match (arguments, self.created_tab_id.lock().unwrap().clone()) {
            (["tab", "create", ..], Some(tab_id)) => Ok(json!({ "tab": { "tab_id": tab_id } })),
            _ => Ok(Value::Null),
        }
    }

    fn move_tab(
        &self,
        session_name: &str,
        tab_id: &str,
        insert_index: i64,
        ssh_host: Option<&str>,
    ) -> Result<(), HerdrError> {
        let herdr_move = (
            session_name.to_string(),
            tab_id.to_string(),
            insert_index,
            ssh_host.map(str::to_string),
        );
        self.moves.lock().unwrap().push(herdr_move);
        Ok(())
    }

    fn list_mirror_panes(
        &self,
        _session_name: &str,
        _ssh_host: Option<&str>,
    ) -> Result<Vec<MirrorPane>, HerdrError> {
        Ok(self.mirror_panes.lock().unwrap().clone())
    }
}

/// 한 스레드가 들어왔음을 알리고 풀려날 때까지 기다리는 관문
#[derive(Default)]
struct Gate {
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl Gate {
    fn enter_and_wait(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).unwrap();
        }
    }

    fn wait_until_entered(&self) {
        let mut state = self.state.lock().unwrap();
        while !state.0 {
            state = self.changed.wait(state).unwrap();
        }
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

struct Harness {
    service: Arc<TerminalTabService>,
    runner: Arc<ScriptedShellRunner>,
    registry: Arc<FakeRegistry>,
    herdr: Arc<FakeHerdr>,
    database: Arc<Database>,
    _directory: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let location = DatabaseLocation {
            database_path: directory.path().join("kanvibe.db"),
            seed_database_path: None,
        };
        let database = Arc::new(Database::open(&location).unwrap());
        let runner = ScriptedShellRunner::new();
        let registry = Arc::new(FakeRegistry::default());
        let herdr = Arc::new(FakeHerdr::default());
        let service = Arc::new(TerminalTabService::new(TerminalTabServiceDependencies {
            database: Arc::clone(&database),
            runner: runner.clone(),
            registry: registry.clone(),
            herdr: herdr.clone(),
            zellij_id_targeting_support: Arc::new(ZellijIdTargetingSupportCache::new()),
        }));
        Self {
            service,
            runner,
            registry,
            herdr,
            database,
            _directory: directory,
        }
    }

    /// TS `stubTask`. 기본은 로컬 tmux 세션 `proj-main`과 worktree `/work/tree`다
    fn stub_task(
        &self,
        session_type: Option<SessionType>,
        session_name: Option<&str>,
        ssh_host: Option<&str>,
    ) -> String {
        let task = NewKanbanTask {
            title: "task".to_string(),
            description: None,
            status: TaskStatus::Todo,
            branch_name: None,
            worktree_path: Some("/work/tree".to_string()),
            session_type,
            session_name: session_name.map(str::to_string),
            ssh_host: ssh_host.map(str::to_string),
            agent_type: None,
            project_id: None,
            base_branch: None,
            pr_url: None,
            priority: None,
        };
        insert_task(&self.database.connection(), &task).unwrap().id
    }

    fn stub_session(&self, session_type: SessionType, ssh_host: Option<&str>) -> String {
        self.stub_task(Some(session_type), Some("proj-main"), ssh_host)
    }

    /// 실행한 명령 가운데 `fragment`가 든 첫 명령
    fn find_executed_command(&self, fragment: &str) -> Option<String> {
        self.runner
            .commands()
            .into_iter()
            .find(|command| command.contains(fragment))
    }

    fn respond_to_all(&self, stdout: &str) {
        let stdout = stdout.to_string();
        self.runner.respond(move |_, _| Some(Ok(stdout.clone())));
    }

    fn fail_all(&self, message: &str) {
        let message = message.to_string();
        self.runner
            .respond(move |_, _| Some(Err(ExecGitError::new(message.clone()))));
    }
}

fn tab(id: &str, native_index: i64, name: &str, is_active: bool) -> TerminalTab {
    TerminalTab {
        id: id.to_string(),
        native_index,
        name: name.to_string(),
        is_active,
    }
}

fn listed(tabs: Vec<TerminalTab>) -> TerminalTabListResult {
    TerminalTabListResult {
        is_ok: true,
        tabs,
        error: None,
    }
}

fn success() -> TerminalTabMutationResult {
    TerminalTabMutationResult {
        is_ok: true,
        should_close_window: None,
        error: None,
    }
}

fn closed(should_close_window: bool) -> TerminalTabMutationResult {
    TerminalTabMutationResult {
        is_ok: true,
        should_close_window: Some(should_close_window),
        error: None,
    }
}

fn close_preview(
    should_close_window: bool,
    process_names: Option<&[&str]>,
) -> TerminalTabClosePreview {
    TerminalTabClosePreview {
        is_ok: true,
        should_close_window,
        process_names: process_names.map(|names| names.iter().map(ToString::to_string).collect()),
        error: None,
    }
}

const TWO_TMUX_WINDOWS: &str = "@1\t0\t1\tzsh\n@2\t1\t0\tvim";

#[test]
fn lists_tmux_windows_as_tabs() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness.respond_to_all(TWO_TMUX_WINDOWS);

    let result = harness.service.list_terminal_tabs(&task_id).unwrap();

    assert_eq!(
        result,
        listed(vec![tab("@1", 0, "zsh", true), tab("@2", 1, "vim", false)])
    );
}

#[test]
fn multiplexer_command_failure_becomes_failed_list_result() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness.fail_all("tmux: command not found");

    let result = harness.service.list_terminal_tabs(&task_id).unwrap();

    let expected = TerminalTabListResult {
        is_ok: false,
        tabs: Vec::new(),
        error: Some("tmux: command not found".to_string()),
    };
    assert_eq!(result, expected);
}

#[test]
fn polling_tmux_window_list_asks_not_to_log_failures() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, Some("remote-mac"));
    harness.respond_to_all("@1\t0\t1\tzsh");

    harness.service.list_terminal_tabs(&task_id).unwrap();

    let calls = harness.runner.calls();
    let list_windows_call = calls
        .iter()
        .find(|call| call.command.contains("list-windows"))
        .unwrap();
    assert_eq!(list_windows_call.ssh_host.as_deref(), Some("remote-mac"));
    assert!(list_windows_call.options.is_failure_expected);
    assert_eq!(
        list_windows_call.options.timeout,
        Some(Duration::from_millis(5_000))
    );
}

#[test]
fn user_close_command_failures_stay_logged() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness
        .runner
        .respond_when_contains("list-windows", TWO_TMUX_WINDOWS);

    harness.service.close_terminal_tab(&task_id, "@1").unwrap();

    let calls = harness.runner.calls();
    let kill_window_call = calls
        .iter()
        .find(|call| call.command.contains("kill-window"))
        .unwrap();
    assert!(!kill_window_call.options.is_failure_expected);
}

#[test]
fn task_without_session_gives_failed_list_without_commands() {
    let harness = Harness::new();
    let task_id = harness.stub_task(None, None, None);

    let result = harness.service.list_terminal_tabs(&task_id).unwrap();
    let missing_task_result = harness.service.list_terminal_tabs("missing-task").unwrap();

    assert!(!result.is_ok);
    assert_eq!(
        result.error.as_deref(),
        Some("작업에 연결된 세션이 없습니다.")
    );
    assert!(!missing_task_result.is_ok);
    assert!(harness.runner.commands().is_empty());
}

#[test]
fn terminal_session_reads_registry_without_multiplexer() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Terminal, None);
    *harness.registry.local_tabs.lock().unwrap() = vec![tab("task-1-1", 0, "zsh", true)];

    let result = harness.service.list_terminal_tabs(&task_id).unwrap();

    assert_eq!(result.tabs.len(), 1);
    assert!(harness.runner.commands().is_empty());
}

#[test]
fn tmux_new_window_gets_worktree_directory() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);

    harness.service.create_terminal_tab(&task_id).unwrap();

    assert!(
        harness
            .find_executed_command("new-window")
            .unwrap()
            .contains("-c '/work/tree'")
    );
}

#[test]
fn zellij_new_tab_gets_worktree_directory() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Zellij, None);

    harness.service.create_terminal_tab(&task_id).unwrap();

    assert!(
        harness
            .find_executed_command("new-tab")
            .unwrap()
            .contains("--cwd '/work/tree'")
    );
}

#[test]
fn terminal_session_creates_tab_only_in_registry() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Terminal, None);

    let result = harness.service.create_terminal_tab(&task_id).unwrap();

    assert_eq!(result, success());
    assert_eq!(
        harness.registry.calls(),
        vec![format!("create_local {task_id}")]
    );
    assert!(harness.runner.commands().is_empty());
}

#[test]
fn closes_tmux_window_and_keeps_window_when_tabs_remain() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness
        .runner
        .respond_when_contains("list-windows", TWO_TMUX_WINDOWS);

    let result = harness.service.close_terminal_tab(&task_id, "@1").unwrap();

    assert_eq!(
        harness.find_executed_command("kill-window").as_deref(),
        Some("tmux kill-window -t '@1'")
    );
    assert_eq!(result, closed(false));
}

#[test]
fn last_tmux_window_is_kept_and_only_app_window_closes() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness
        .runner
        .respond_when_contains("list-windows", "@1\t0\t1\tzsh");

    let result = harness.service.close_terminal_tab(&task_id, "@1").unwrap();

    assert_eq!(harness.find_executed_command("kill-window"), None);
    assert_eq!(result, closed(true));
}

/// 남은 탭 ID 목록을 들고 tmux·zellij 목록과 닫기 명령에 답하는 가짜 멀티플렉서
fn script_closable_tabs(harness: &Harness, list_gate: Arc<Gate>) -> Arc<Mutex<Vec<String>>> {
    let tabs = Arc::new(Mutex::new(vec!["1".to_string(), "2".to_string()]));
    let closed_id_pattern =
        regex::Regex::new(r"(?:kill-window -t '@|close-tab-by-id ')(\d+)'").unwrap();
    let scripted_tabs = Arc::clone(&tabs);
    let first_list = Mutex::new(true);
    harness.runner.respond(move |command, _| {
        if command == "zellij --version" {
            return Some(Ok("zellij 0.44.0".to_string()));
        }
        if command.contains("list-windows") || command.contains("list-tabs") {
            if std::mem::replace(&mut *first_list.lock().unwrap(), false) {
                list_gate.enter_and_wait();
            }
            let tab_ids = scripted_tabs.lock().unwrap().clone();
            return Some(Ok(render_tab_list(command, &tab_ids)));
        }
        if let Some(captures) = closed_id_pattern.captures(command) {
            scripted_tabs
                .lock()
                .unwrap()
                .retain(|tab_id| tab_id != &captures[1]);
        }
        Some(Ok(String::new()))
    });
    tabs
}

fn render_tab_list(command: &str, tab_ids: &[String]) -> String {
    if command.contains("list-windows") {
        let lines: Vec<String> = tab_ids
            .iter()
            .enumerate()
            .map(|(index, tab_id)| format!("@{tab_id}\t{index}\t1\tshell"))
            .collect();
        return lines.join("\n");
    }
    let zellij_tabs: Vec<Value> = tab_ids
        .iter()
        .enumerate()
        .map(|(position, tab_id)| {
            json!({ "tab_id": tab_id.parse::<i64>().unwrap(), "position": position, "name": "shell", "active": true })
        })
        .collect();
    Value::Array(zellij_tabs).to_string()
}

fn close_two_tabs_concurrently(session_type: SessionType) {
    let harness = Harness::new();
    let task_id = harness.stub_session(session_type, None);
    let list_gate = Arc::new(Gate::default());
    let tabs = script_closable_tabs(&harness, Arc::clone(&list_gate));
    let (first_tab_id, second_tab_id) = match session_type {
        SessionType::Tmux => ("@1", "@2"),
        _ => ("1", "2"),
    };

    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            harness
                .service
                .close_terminal_tab(&task_id, first_tab_id)
                .unwrap()
        });
        list_gate.wait_until_entered();
        let second = scope.spawn(|| {
            harness
                .service
                .close_terminal_tab(&task_id, second_tab_id)
                .unwrap()
        });
        std::thread::sleep(Duration::from_millis(100));
        let list_count = harness
            .runner
            .commands()
            .iter()
            .filter(|command| command.contains("list-"))
            .count();
        list_gate.release();
        assert_eq!(
            list_count, 1,
            "두 번째 닫기는 첫 닫기가 끝날 때까지 목록을 읽지 않는다"
        );
        (first.join().unwrap(), second.join().unwrap())
    });

    assert_eq!(*tabs.lock().unwrap(), vec!["2".to_string()]);
    assert_eq!((first, second), (closed(false), closed(true)));
}

#[test]
fn concurrent_closes_keep_last_tmux_and_zellij_tab() {
    close_two_tabs_concurrently(SessionType::Tmux);
    close_two_tabs_concurrently(SessionType::Zellij);
}

/// `boundary`가 host면 호스트만, session이면 세션 이름만 다른 두 작업의 닫기
fn close_in_other_boundary_without_waiting(is_host_boundary: bool) {
    let harness = Harness::new();
    let task_for = |name: &str| match is_host_boundary {
        true => harness.stub_task(Some(SessionType::Tmux), Some("same-session"), Some(name)),
        false => harness.stub_task(Some(SessionType::Tmux), Some(name), Some("same-host")),
    };
    let (first_task_id, second_task_id) = (task_for("first"), task_for("second"));
    let first_gate = Arc::new(Gate::default());
    let gate = Arc::clone(&first_gate);
    harness.runner.respond(move |command, ssh_host| {
        if !command.contains("list-windows") {
            return None;
        }
        if ssh_host == Some("first") || command.contains("'first'") {
            gate.enter_and_wait();
        }
        Some(Ok(TWO_TMUX_WINDOWS.to_string()))
    });

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            harness
                .service
                .close_terminal_tab(&first_task_id, "@1")
                .unwrap()
        });
        first_gate.wait_until_entered();
        let second = harness
            .service
            .close_terminal_tab(&second_task_id, "@1")
            .unwrap();
        first_gate.release();
        assert_eq!(second, closed(false));
        assert_eq!(first.join().unwrap(), closed(false));
    });
}

#[test]
fn close_in_other_host_or_session_does_not_wait() {
    close_in_other_boundary_without_waiting(true);
    close_in_other_boundary_without_waiting(false);
}

#[test]
fn failed_close_does_not_block_next_close_of_same_session() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    let kill_gate = Arc::new(Gate::default());
    let gate = Arc::clone(&kill_gate);
    harness.runner.respond(move |command, _| {
        if command.contains("list-windows") {
            return Some(Ok(TWO_TMUX_WINDOWS.to_string()));
        }
        command.contains("'@1'").then(|| {
            gate.enter_and_wait();
            Err(ExecGitError::new("close failed"))
        })
    });

    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| harness.service.close_terminal_tab(&task_id, "@1").unwrap());
        kill_gate.wait_until_entered();
        let second = scope.spawn(|| harness.service.close_terminal_tab(&task_id, "@2").unwrap());
        kill_gate.release();
        (first.join().unwrap(), second.join().unwrap())
    });

    assert_eq!(first, TerminalTabMutationResult::failure("close failed"));
    assert_eq!(second, closed(false));
    assert!(
        harness
            .find_executed_command("kill-window -t '@2'")
            .is_some()
    );
}

#[test]
fn last_zellij_tab_is_kept_and_only_app_window_closes() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Zellij, None);
    harness.runner.respond_when_contains(
        "list-tabs",
        r#"[{"tab_id":0,"position":0,"name":"Tab #1","active":true}]"#,
    );
    harness.runner.respond(|command, _| {
        (command == "zellij --version").then(|| Ok("zellij 0.44.0".to_string()))
    });

    let result = harness.service.close_terminal_tab(&task_id, "0").unwrap();

    assert_eq!(harness.find_executed_command("close-tab"), None);
    assert_eq!(result, closed(true));
}

#[test]
fn unreadable_session_list_deletes_nothing_and_closes_app_window() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness
        .runner
        .fail_when_contains("list-windows", "no server running");

    let result = harness.service.close_terminal_tab(&task_id, "@1").unwrap();

    assert_eq!(harness.find_executed_command("kill-window"), None);
    assert_eq!(result, closed(true));
}

#[test]
fn zellij_0_44_selects_tab_by_id() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Zellij, None);
    harness.respond_to_all("[]");
    harness.runner.respond(|command, _| {
        (command == "zellij --version").then(|| Ok("zellij 0.44.0".to_string()))
    });

    harness.service.select_terminal_tab(&task_id, "2").unwrap();

    assert_eq!(
        harness.find_executed_command("go-to-tab-by-id").as_deref(),
        Some("zellij --session 'proj-main' action go-to-tab-by-id '2'")
    );
}

#[test]
fn zellij_before_0_44_selects_tab_by_name() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Zellij, None);
    harness.runner.respond(|command, _| {
        (command == "zellij --version").then(|| Ok("zellij 0.43.1".to_string()))
    });
    harness
        .runner
        .respond_when_contains("query-tab-names", "shell\nlogs");

    harness.service.select_terminal_tab(&task_id, "1").unwrap();

    assert_eq!(
        harness.find_executed_command("go-to-tab-name").as_deref(),
        Some("zellij --session 'proj-main' action go-to-tab-name 'logs'")
    );
    assert_eq!(harness.find_executed_command("go-to-tab-by-id"), None);
}

#[test]
fn zellij_version_is_checked_once_per_host() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Zellij, None);
    harness.respond_to_all("[]");
    harness.runner.respond(|command, _| {
        (command == "zellij --version").then(|| Ok("zellij 0.44.0".to_string()))
    });

    harness.service.list_terminal_tabs(&task_id).unwrap();
    harness.service.list_terminal_tabs(&task_id).unwrap();

    let version_calls = harness
        .runner
        .commands()
        .into_iter()
        .filter(|command| command == "zellij --version")
        .count();
    assert_eq!(version_calls, 1);
}

#[test]
fn blank_tab_name_is_rejected_without_commands() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);

    let result = harness
        .service
        .rename_terminal_tab(&task_id, "@1", "   ")
        .unwrap();

    assert_eq!(
        result,
        TerminalTabMutationResult::failure("탭 이름은 비워 둘 수 없습니다.")
    );
    assert!(harness.runner.commands().is_empty());
}

#[test]
fn tmux_rename_puts_name_after_double_dash() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);

    harness
        .service
        .rename_terminal_tab(&task_id, "@1", "-build")
        .unwrap();

    assert_eq!(
        harness.find_executed_command("rename-window").as_deref(),
        Some("tmux rename-window -t '@1' -- '-build'")
    );
}

#[test]
fn tmux_move_inserts_before_target_then_renumbers() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness.respond_to_all("@1\t0\t0\tzsh\n@2\t1\t0\tvim\n@3\t2\t1\ttop");

    harness
        .service
        .move_terminal_tab(&task_id, "@3", 0)
        .unwrap();

    assert_eq!(
        harness.runner.commands()[1..],
        [
            "tmux move-window -b -s '@3' -t 'proj-main':0",
            "tmux move-window -r -t 'proj-main'"
        ]
    );
}

#[test]
fn tmux_move_targets_native_index_of_destination_window() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness.respond_to_all("@1\t1\t0\tzsh\n@2\t2\t0\tvim\n@3\t3\t1\ttop");

    harness
        .service
        .move_terminal_tab(&task_id, "@3", 0)
        .unwrap();

    assert_eq!(
        harness.find_executed_command("move-window -b").as_deref(),
        Some("tmux move-window -b -s '@3' -t 'proj-main':1")
    );
}

#[test]
fn tab_missing_from_list_is_not_moved() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness.respond_to_all("@1\t0\t1\tzsh");

    let result = harness
        .service
        .move_terminal_tab(&task_id, "@9", 0)
        .unwrap();

    assert_eq!(
        result,
        TerminalTabMutationResult::failure("탭을 찾을 수 없습니다.")
    );
    assert_eq!(harness.find_executed_command("move-window"), None);
}

#[test]
fn negative_move_index_is_rejected() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);

    let result = harness
        .service
        .move_terminal_tab(&task_id, "@3", -1)
        .unwrap();

    assert_eq!(
        result,
        TerminalTabMutationResult::failure("이동할 위치가 올바르지 않습니다.")
    );
    assert!(harness.runner.commands().is_empty());
}

#[test]
fn herdr_tabs_keep_native_ids_and_active_state() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Herdr, None);
    let herdr_tabs = vec![
        tab("w1:t1", 0, "shell", true),
        tab("w1:t2", 1, "shell", false),
    ];
    *harness.herdr.tabs.lock().unwrap() = herdr_tabs.clone();

    let result = harness.service.list_terminal_tabs(&task_id).unwrap();

    assert_eq!(result, listed(herdr_tabs));
    assert_eq!(
        *harness.herdr.tab_reads.lock().unwrap(),
        vec![("proj-main".to_string(), None)]
    );
}

/// TS "Herdr 탭 조작" 묶음의 beforeEach. 두 워크스페이스에 같은 이름의 탭이 둘씩 있다
fn herdr_harness_with_two_workspaces() -> (Harness, String) {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Herdr, Some("remote-herdr"));
    *harness.herdr.tabs.lock().unwrap() = vec![
        tab("w1:t1", 0, "same", false),
        tab("w1:t2", 1, "same", false),
        tab("w2:t1", 2, "same", false),
        tab("w2:t2", 3, "same", true),
    ];
    (harness, task_id)
}

#[test]
fn herdr_new_tab_goes_to_active_workspace() {
    let (harness, task_id) = herdr_harness_with_two_workspaces();

    let result = harness.service.create_terminal_tab(&task_id).unwrap();

    assert_eq!(result, success());
    let expected_arguments = [
        "tab",
        "create",
        "--workspace",
        "w2",
        "--cwd",
        "/work/tree",
        "--focus",
    ];
    assert!(harness.herdr.has_command(&expected_arguments));
    assert_eq!(
        harness.herdr.commands.lock().unwrap()[0].1.as_deref(),
        Some("remote-herdr")
    );
}

#[test]
fn herdr_move_converts_position_to_workspace_insert_boundary() {
    for (tab_id, destination, insert_index) in [("w2:t2", 2, 0), ("w2:t1", 3, 2)] {
        let (harness, task_id) = herdr_harness_with_two_workspaces();

        let result = harness
            .service
            .move_terminal_tab(&task_id, tab_id, destination)
            .unwrap();

        assert_eq!(result, success());
        let expected_move = (
            "proj-main".to_string(),
            tab_id.to_string(),
            insert_index,
            Some("remote-herdr".to_string()),
        );
        assert_eq!(harness.herdr.moves(), vec![expected_move]);
    }
}

#[test]
fn herdr_move_across_workspaces_is_rejected() {
    let (harness, task_id) = herdr_harness_with_two_workspaces();

    let result = harness
        .service
        .move_terminal_tab(&task_id, "w2:t2", 0)
        .unwrap();

    assert_eq!(
        result,
        TerminalTabMutationResult::failure(
            "Herdr 탭은 같은 워크스페이스 안에서 이동할 수 있습니다."
        )
    );
    assert!(harness.herdr.moves().is_empty());
}

#[test]
fn last_herdr_tab_is_kept_and_only_app_window_closes() {
    let (harness, task_id) = herdr_harness_with_two_workspaces();
    harness.herdr.tabs.lock().unwrap().truncate(1);

    let result = harness
        .service
        .close_terminal_tab(&task_id, "w1:t1")
        .unwrap();

    assert_eq!(result, closed(true));
    assert!(!harness.herdr.has_command_starting_with(&["tab", "close"]));
}

#[test]
fn herdr_select_rename_close_target_tab_by_id_despite_duplicate_names() {
    let (harness, task_id) = herdr_harness_with_two_workspaces();

    assert_eq!(
        harness
            .service
            .select_terminal_tab(&task_id, "w2:t2")
            .unwrap(),
        success()
    );
    assert_eq!(
        harness
            .service
            .rename_terminal_tab(&task_id, "w2:t2", "new name")
            .unwrap(),
        success()
    );
    assert_eq!(
        harness
            .service
            .close_terminal_tab(&task_id, "w2:t2")
            .unwrap(),
        closed(false)
    );

    assert!(harness.herdr.has_command(&["tab", "focus", "w2:t2"]));
    assert!(
        harness
            .herdr
            .has_command(&["tab", "rename", "w2:t2", "new name"])
    );
    assert!(harness.herdr.has_command(&["tab", "close", "w2:t2"]));
}

#[test]
fn close_preview_for_last_multiplexer_tab_only_closes_app_window() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness.respond_to_all("@1\t0\t1\tzsh");

    let preview = harness.service.preview_terminal_tab_close(&task_id, "@1");

    assert_eq!(preview, close_preview(true, None));
    assert_eq!(harness.find_executed_command("kill-window"), None);
    assert_eq!(harness.find_executed_command("list-panes"), None);
}

#[test]
fn close_preview_collects_process_names_from_all_panes_of_the_tab_only() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, Some("dev-host"));
    harness.respond_to_all("%1\t@1\tclaude\t0\t0\t80\t24\twork\n%2\t@1\tnode\t80\t0\t80\t24\twork\n%3\t@2\tvim\t0\t0\t80\t24\tother");
    harness
        .runner
        .respond_when_contains("list-windows", "@1\t0\t1\twork\n@2\t1\t0\tother");

    let preview = harness.service.preview_terminal_tab_close(&task_id, "@1");

    assert_eq!(preview, close_preview(false, Some(&["claude", "node"])));
    assert!(
        harness
            .runner
            .calls()
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("dev-host"))
    );
    assert_eq!(harness.find_executed_command("kill-window"), None);
}

fn mirror_pane(id: &str, tab_id: &str, command: &str) -> MirrorPane {
    MirrorPane {
        id: id.to_string(),
        tab_id: tab_id.to_string(),
        tab_name: String::new(),
        command: command.to_string(),
        left: 0,
        top: 0,
        width: 80,
        height: 38,
    }
}

#[test]
fn close_preview_collects_herdr_process_names_of_the_tab_only() {
    let harness = Harness::new();
    let task_id = harness.stub_task(Some(SessionType::Herdr), Some("herdr-session"), None);
    *harness.herdr.tabs.lock().unwrap() = vec![
        tab("w1:t1", 0, "work", true),
        tab("w1:t2", 1, "other", false),
    ];
    *harness.herdr.mirror_panes.lock().unwrap() = vec![
        mirror_pane("w1:p1", "w1:t1", "claude"),
        mirror_pane("w1:p2", "w1:t2", "vim"),
    ];

    let preview = harness
        .service
        .preview_terminal_tab_close(&task_id, "w1:t1");

    assert_eq!(preview, close_preview(false, Some(&["claude"])));
}

#[test]
fn failed_process_lookup_is_not_reported_as_no_running_work() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness.fail_all("process lookup failed");
    harness
        .runner
        .respond_when_contains("list-windows", "@1\t0\t1\twork\n@2\t1\t0\tother");

    let preview = harness.service.preview_terminal_tab_close(&task_id, "@1");

    assert_eq!(preview, close_preview(false, None));
}

#[test]
fn failed_tab_list_is_not_turned_into_close_permission() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness.fail_all("offline");

    let preview = harness.service.preview_terminal_tab_close(&task_id, "@1");

    assert_eq!(preview, TerminalTabClosePreview::failure("offline"));
}

#[test]
fn empty_process_lookup_is_not_assumed_safe() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness
        .runner
        .respond_when_contains("list-windows", "@1\t0\t1\twork\n@2\t1\t0\tother");

    let preview = harness.service.preview_terminal_tab_close(&task_id, "@1");

    assert_eq!(preview.process_names, None);
}

#[test]
fn remote_terminal_does_not_show_local_ssh_process_as_remote_work() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Terminal, Some("dev-host"));
    *harness.registry.local_tabs.lock().unwrap() = vec![tab("tab-1", 0, "", true)];
    *harness.registry.local_process_name.lock().unwrap() = Some("ssh".to_string());

    let preview = harness
        .service
        .preview_terminal_tab_close(&task_id, "tab-1");

    assert_eq!(preview.process_names, None);
    assert!(!harness.registry.has_call_starting_with("local_process"));
}

#[test]
fn missing_tab_is_not_accepted_as_close_target() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Tmux, None);
    harness.respond_to_all("@1\t0\t1\tzsh");

    let preview = harness.service.preview_terminal_tab_close(&task_id, "@999");

    assert_eq!(
        preview,
        TerminalTabClosePreview::failure("탭을 찾을 수 없습니다.")
    );
}

#[test]
fn plain_terminal_warns_about_current_pty_process_even_for_last_tab() {
    let harness = Harness::new();
    let task_id = harness.stub_session(SessionType::Terminal, None);
    *harness.registry.local_tabs.lock().unwrap() = vec![tab("tab-1", 0, "", true)];
    *harness.registry.local_process_name.lock().unwrap() = Some("node".to_string());

    let preview = harness
        .service
        .preview_terminal_tab_close(&task_id, "tab-1");

    assert_eq!(preview, close_preview(false, Some(&["node"])));
    assert!(!harness.registry.has_call_starting_with("close_local"));
}

/// TS "공유 Space 탭 서비스" 묶음의 beforeEach. 공유 서버 Space `w1`의 탭 둘이 보인다
fn shared_space_harness() -> (Harness, String) {
    let harness = Harness::new();
    let task_id = harness.stub_task(Some(SessionType::Herdr), Some("kanvibe:task-a"), None);
    *harness.herdr.tabs.lock().unwrap() =
        vec![tab("w1:t1", 0, "one", true), tab("w1:t2", 1, "two", false)];
    *harness.herdr.created_tab_id.lock().unwrap() = Some("w1:t3".to_string());
    (harness, task_id)
}

#[test]
fn shared_space_marks_task_client_selection_as_active() {
    let (harness, task_id) = shared_space_harness();
    *harness.registry.herdr_active_tab_id.lock().unwrap() = Some("w1:t2".to_string());

    let result = harness.service.list_terminal_tabs(&task_id).unwrap();

    let active_states: Vec<bool> = result.tabs.iter().map(|tab| tab.is_active).collect();
    assert_eq!(active_states, vec![false, true]);
}

#[test]
fn shared_space_create_and_select_go_to_task_client_without_global_focus() {
    let (harness, task_id) = shared_space_harness();

    assert_eq!(
        harness.service.create_terminal_tab(&task_id).unwrap(),
        success()
    );
    assert!(
        harness
            .herdr
            .command_arguments()
            .iter()
            .any(|arguments| arguments.last().map(String::as_str) == Some("--no-focus"))
    );
    assert!(
        harness
            .registry
            .calls()
            .contains(&format!("focus_herdr {task_id} w1:t3"))
    );

    assert_eq!(
        harness
            .service
            .select_terminal_tab(&task_id, "w1:t2")
            .unwrap(),
        success()
    );
    assert!(
        harness
            .registry
            .calls()
            .contains(&format!("focus_herdr {task_id} w1:t2"))
    );
    assert!(!harness.herdr.has_command_starting_with(&["tab", "focus"]));
}

#[test]
fn shared_space_rejects_selecting_tab_of_other_space() {
    let (harness, task_id) = shared_space_harness();

    let result = harness
        .service
        .select_terminal_tab(&task_id, "w2:t1")
        .unwrap();

    assert_eq!(
        result,
        TerminalTabMutationResult::failure("탭을 찾을 수 없습니다.")
    );
    assert!(!harness.registry.has_call_starting_with("focus_herdr"));
}
