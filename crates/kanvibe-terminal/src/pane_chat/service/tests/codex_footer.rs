//! TS `paneChatService.test.ts`의 "pane process lookup"과 "Codex displayed-thread footer binding".
//!
//! 이 절은 CLAUDE.md의 Codex `/statusline thread-id` 예외를 지킨다. 사용자가 누른 "대화 ID 표시"에서만, 확인된 버전·dim 빈 입력란에서만
//! 명령을 한 번 붙여넣고, 붙여넣기 전후와 Enter 직전에 같은 pane·프로세스를 다시 확인하며, 에코가 없으면 Enter를 보내지 않는다.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use kanvibe_ai::codex_thread::CodexUserInput;
use kanvibe_ai::javascript::utf16_length;
use kanvibe_ai::pane_chat::claude_prompt::PaneChatPromptKind;
use kanvibe_ai::pane_chat::model_settings::PaneChatModelSettings;
use kanvibe_ai::pane_chat::snapshot::{
    PaneChatInputState, PaneChatSnapshot, PaneChatTranscriptStatus,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_process::exec::ExecGitError;
use serde_json::json;

use super::super::dependencies::{BeforeSend, CodexThreadDelivery, PaneChatReaderScope};
use super::super::{PaneChatCodexThreadRevealOutcome, PaneChatError};
use super::*;
use crate::pane_chat::attachments::PaneChatAttachmentInput;
use crate::pane_chat::surface::{PaneChatSurface, PaneChatTask};

const THREAD_X: &str = "0190aaaa-0000-7000-8000-00000000000a";
const THREAD_Y: &str = "0190bbbb-0000-7000-8000-00000000000b";
const THREAD_Z: &str = "0190cccc-0000-7000-8000-00000000000c";
const STATUSLINE_PASTE: &str = "\x1b[200~/statusline thread-id\x1b[201~";
const PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aOK0AAAAASUVORK5CYII=";

fn rollout_file(thread_id: &str) -> String {
    format!("/home/.codex/sessions/2026/11/14/rollout-2026-11-14T10-00-00-{thread_id}.jsonl")
}

fn codex_process_table() -> String {
    [
        "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh".to_string(),
        format!(
            "200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 codex codex resume {THREAD_X} --yolo"
        ),
        "110 1 110 210 ttys011 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh".to_string(),
        "210 110 210 210 ttys011 Thu Nov 12 19:00:00 2026 codex codex --yolo".to_string(),
    ]
    .join("\n")
}

fn is_txt_lsof_command(command: &str) -> bool {
    command
        .strip_prefix("lsof -a -p ")
        .and_then(|rest| rest.strip_suffix(" -d txt -Fn"))
        .is_some_and(|pid| {
            !pid.is_empty() && pid.chars().all(|character| character.is_ascii_digit())
        })
}

fn rows(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| line.to_string()).collect()
}

const APPROVAL: [&str; 9] = [
    "  Would you like to run the following command?",
    "",
    "  $ git tag demo",
    "",
    "",
    "› 1. Yes, proceed (y)",
    "  2. No, and tell Codex what to do differently (esc)",
    "",
    "  Press enter to confirm or esc to cancel",
];
const RATE_LIMIT: [&str; 7] = [
    "  Approaching rate limits",
    "",
    "",
    "› 1. Switch to gpt-6-luna",
    "  2. Keep current model",
    "",
    "  enter select · esc back",
];

static CODEX_OBSERVATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CODEX_TASK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 실제 Codex TUI가 입력란과 footer를 그리고 입력을 받는 방식만 흉내 내는 터미널 경계 대역
struct CodexPaneState {
    composer: String,
    threads_showing_id: HashSet<String>,
    submissions: Vec<(String, String)>,
    commands: Vec<String>,
    show_footer: bool,
    initial_footer: String,
    /// 상태줄 명령을 실행해도 footer에 ID가 나타나지 않는 화면(예: 좁은 pane에서 잘림)
    ignores_status_line: bool,
    /// 붙여넣기를 입력란에 그리지 않는 화면
    drops_paste: bool,
    /// 사용자가 여러 줄 입력의 첫 줄로 커서를 올려 둔 상태
    is_cursor_on_first_composer_row: bool,
    /// 입력란과 footer 자리를 차지한 선택 화면의 줄. 숫자 키나 Esc를 받으면 닫힌다
    dialog: Option<Vec<String>>,
    dialog_keys: Vec<String>,
    /// 선택 화면 위에 보이는 대화 내용
    history: String,
    /// pane이 좁아져 줄이 다시 접히면서 화면 위로 밀려난 줄 수
    rows_scrolled_off: usize,
    header_directory_line: String,
    thread_id: String,
}

struct FakeCodexPane {
    pane_id: &'static str,
    root_pid: i64,
    state: Mutex<CodexPaneState>,
}

impl FakeCodexPane {
    fn new(pane_id: &'static str, root_pid: i64, thread_id: &str) -> Arc<Self> {
        Arc::new(Self {
            pane_id,
            root_pid,
            state: Mutex::new(CodexPaneState {
                composer: String::new(),
                threads_showing_id: HashSet::new(),
                submissions: Vec::new(),
                commands: Vec::new(),
                show_footer: true,
                initial_footer: "  gpt-sol high · ~/repo".to_string(),
                ignores_status_line: false,
                drops_paste: false,
                is_cursor_on_first_composer_row: false,
                dialog: None,
                dialog_keys: Vec::new(),
                history: "• Earlier reply".to_string(),
                rows_scrolled_off: 0,
                header_directory_line: "     ~/projects/qa/run/workspace".to_string(),
                thread_id: thread_id.to_string(),
            }),
        })
    }

    fn with<T>(&self, change: impl FnOnce(&mut CodexPaneState) -> T) -> T {
        change(&mut self.state.lock().unwrap())
    }

    /// `/new`나 `/resume`처럼 같은 process 안에서 입력을 받는 대화가 바뀐다. footer 설정은 새 대화로 따라가지 않는다
    fn switch_thread(&self, thread_id: &str) {
        self.with(|state| {
            state.thread_id = thread_id.to_string();
            state.composer.clear();
        });
    }

    fn submissions(&self) -> Vec<(String, String)> {
        self.with(|state| state.submissions.clone())
    }

    fn commands(&self) -> Vec<String> {
        self.with(|state| state.commands.clone())
    }

    fn composer(&self) -> String {
        self.with(|state| state.composer.clone())
    }

    fn write(&self, bytes: &str) {
        self.with(|state| {
            if state.dialog.is_some() {
                state.dialog_keys.push(bytes.to_string());
                state.dialog = None;
                return;
            }
            apply_codex_input(state, bytes);
        });
    }

    /// 화면을 읽지 않는 조회의 결과. 실제 pane 조회처럼 입력란·커서·화면 줄이 없다
    fn surface_without_screen(&self, task: &PaneChatTask) -> PaneChatSurface {
        PaneChatSurface {
            task: task.clone(),
            pane_id: self.pane_id.to_string(),
            root_pid: Some(self.root_pid),
            agent_session: None,
            screen: String::new(),
            input_line: None,
            cursor_column: None,
            cursor_row: None,
            viewport_lines: None,
            viewport_columns: None,
            is_input_placeholder: false,
            observation: None,
        }
    }

    fn surface(&self, task: &PaneChatTask) -> PaneChatSurface {
        let mut frame = self.surface_without_screen(task);
        frame.observation = Some(CODEX_OBSERVATION_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1);
        self.with(|state| match &state.dialog {
            Some(dialog) => draw_dialog(state, dialog, &mut frame),
            None => draw_composer(state, &mut frame),
        });
        frame
    }
}

fn apply_codex_input(state: &mut CodexPaneState, bytes: &str) {
    if let Some(pasted) = bytes
        .strip_prefix("\x1b[200~")
        .and_then(|rest| rest.strip_suffix("\x1b[201~"))
    {
        if state.drops_paste {
            return;
        }
        // 실제 TUI는 이미지 경로를 순번 자리표시로 바꾸고, 긴 글은 글자 수만 적힌 자리표시로 접는다.
        if pasted.ends_with(".png") {
            let images = state.composer.matches("[Image #").count();
            state
                .composer
                .push_str(&format!("[Image #{}] ", images + 1));
        } else if utf16_length(pasted) > 1000 {
            state
                .composer
                .push_str(&format!("[Pasted Content {} chars]", utf16_length(pasted)));
        } else {
            state.composer.push_str(pasted);
        }
        return;
    }
    if bytes != "\r" {
        state.composer.push_str(bytes);
        return;
    }
    let text = std::mem::take(&mut state.composer);
    if !text.starts_with('/') {
        let thread_id = state.thread_id.clone();
        state.submissions.push((thread_id, text));
        return;
    }
    // 실제 0.169.0 TUI는 붙여넣은 `/statusline thread-id`를 실행해 이 세션의 footer를 대화 ID로 바꾼다.
    if text == "/statusline thread-id" && !state.ignores_status_line {
        let thread_id = state.thread_id.clone();
        state.threads_showing_id.insert(thread_id);
    }
    state.commands.push(text);
}

/// 실제 TUI는 선택 화면을 화면 아래에 붙이고 커서를 마지막 줄 끝에 세워 둔다. 대화 ID가 있던 footer는 가려진다
fn draw_dialog(state: &CodexPaneState, dialog: &[String], frame: &mut PaneChatSurface) {
    let mut lines = vec![
        "  >_ OpenAI Codex (v0.169.0)".to_string(),
        state.header_directory_line.clone(),
        String::new(),
    ];
    lines.extend(state.history.split('\n').map(str::to_string));
    lines.extend([String::new(), String::new()]);
    lines.extend(dialog.iter().cloned());
    let lines: Vec<String> = lines.into_iter().skip(state.rows_scrolled_off).collect();
    frame.screen = lines.join("\n");
    frame.input_line = lines.last().cloned();
    frame.cursor_column = Some(120);
    frame.cursor_row = Some(lines.len() - 1);
    frame.viewport_columns = Some(120);
    frame.is_input_placeholder = false;
    frame.viewport_lines = Some(lines);
}

fn draw_composer(state: &CodexPaneState, frame: &mut PaneChatSurface) {
    let composer = match state.composer.is_empty() {
        true => "Ask Codex to do anything",
        false => state.composer.as_str(),
    };
    let mut composer_lines = composer.split('\n');
    let first_line = composer_lines.next().unwrap_or_default();
    let more_lines: Vec<&str> = composer_lines.collect();
    let mut lines = vec!["  >_ OpenAI Codex (v0.169.0)".to_string(), String::new()];
    // 명령 이름을 치는 동안에는 선택 표시가 붙은 자동완성 줄이 입력란 위에 뜬다.
    let is_command_name = state.composer.starts_with('/')
        && !state
            .composer
            .chars()
            .any(kanvibe_ai::javascript::is_js_whitespace);
    if is_command_name {
        lines.extend([
            "› /model  choose what model and reasoning effort to use".to_string(),
            String::new(),
        ]);
    }
    lines.push(format!("› {first_line}"));
    lines.extend(more_lines.iter().map(|line| match line.is_empty() {
        true => String::new(),
        false => format!("  {line}"),
    }));
    let cursor_row = match state.is_cursor_on_first_composer_row {
        true => lines.len() - 1 - more_lines.len(),
        false => lines.len() - 1,
    };
    let footer = match state.show_footer || state.threads_showing_id.contains(&state.thread_id) {
        true => format!("  {}", state.thread_id),
        false => state.initial_footer.clone(),
    };
    lines.push(String::new());
    lines.extend(footer.split('\n').map(str::to_string));
    if state.composer.is_empty() {
        lines.push("  ← for agents · ? for shortcuts".to_string());
    }
    frame.screen = lines.join("\n");
    frame.input_line = Some(lines[cursor_row].clone());
    frame.cursor_column = Some(match state.composer.is_empty() {
        true => 2,
        false => utf16_length(&lines[cursor_row]),
    });
    frame.cursor_row = Some(cursor_row);
    frame.viewport_columns = Some(160);
    frame.is_input_placeholder = state.composer.is_empty();
    frame.viewport_lines = Some(lines);
}

/// 같은 cwd에서 공유 서버에 붙은 Codex TUI 두 개를 띄운 태스크
struct CodexFixture {
    harness: Harness,
    task_id: String,
    a: Arc<FakeCodexPane>,
    b: Arc<FakeCodexPane>,
    rollout_calls: Arc<Mutex<Vec<(PaneChatReaderScope, String)>>>,
}

fn codex_exec_responder(
    table: String,
    version: String,
) -> impl Fn(&str, Option<&str>) -> Option<Result<String, ExecGitError>> + Send + Sync + 'static {
    move |command, _| {
        Some(if is_pane_process_table_command(command) {
            Ok(table.clone())
        } else if is_txt_lsof_command(command) {
            Ok("p200\nftxt\nn/opt/codex/bin/codex\nftxt\nn/usr/lib/dyld".to_string())
        } else if command == "'/opt/codex/bin/codex' --version" {
            Ok(version.clone())
        } else {
            Err(ExecGitError::new("No open transcript"))
        })
    }
}

/// 이미지 자리표시와 글을 합쳐 TUI가 받은 것처럼 그 대화의 pane에 남긴다
fn record_api_submission(
    panes: &HashMap<&'static str, Arc<FakeCodexPane>>,
    delivery: &CodexThreadDelivery,
) {
    let thread_id = &delivery.address.thread_id;
    let mut text = String::new();
    let mut image_count = 0;
    for entry in &delivery.input {
        match entry {
            CodexUserInput::Text { text: part } => text.push_str(part),
            CodexUserInput::LocalImage { .. } => image_count += 1,
        }
    }
    let images: String = (1..=image_count)
        .map(|index| format!("[Image #{index}] "))
        .collect();
    let pane = panes
        .values()
        .find(|pane| pane.with(|state| &state.thread_id == thread_id))
        .unwrap_or(&panes["%A"]);
    pane.with(|state| {
        state
            .submissions
            .push((thread_id.clone(), format!("{images}{text}")))
    });
}

fn create_codex_task(version: &str) -> CodexFixture {
    let harness = Harness::new();
    let task_id = format!(
        "task-codex-footer-{}",
        CODEX_TASK_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1
    );
    let a = FakeCodexPane::new("%A", 100, THREAD_X);
    let b = FakeCodexPane::new("%B", 110, THREAD_Z);
    let fixture = CodexFixture {
        harness,
        task_id,
        a,
        b,
        rollout_calls: Arc::new(Mutex::new(Vec::new())),
    };
    fixture.serve_frames(|frame| frame);
    let panes = fixture.panes();
    fixture.harness.bridge.respond(move |_, pane_id, bytes| {
        panes[pane_id].write(bytes);
        Ok(())
    });
    fixture.harness.runner.respond(codex_exec_responder(
        codex_process_table(),
        version.to_string(),
    ));
    fixture.install_codex_sessions();
    fixture
}

impl CodexFixture {
    /// 공유 서버 대화 기록·app-server 응답. 메인 대화 기록은 대화 ID마다 하나다
    fn install_codex_sessions(&self) {
        // 공유 app-server의 hook은 pane을 증명하지 못해 연결을 남기지 않는다.
        self.harness.sessions.set_binding(None);
        let rollout_calls = self.rollout_calls.clone();
        self.harness
            .sessions
            .respond_rollout(move |scope, thread_id| {
                rollout_calls
                    .lock()
                    .unwrap()
                    .push((scope.clone(), thread_id.to_string()));
                Ok(Some(rollout_file(thread_id)))
            });
        self.harness.sessions.respond_detail(|call| {
            Ok(Some(detail(
                &call.session_id,
                AiSessionProvider::Codex,
                Vec::new(),
                None,
            )))
        });
        let panes = self.panes();
        self.harness
            .sessions
            .respond_api_input(move |delivery, before_send, on_write| {
                before_send()?;
                on_write();
                record_api_submission(&panes, delivery);
                Ok(())
            });
    }

    fn panes(&self) -> HashMap<&'static str, Arc<FakeCodexPane>> {
        HashMap::from([("%A", self.a.clone()), ("%B", self.b.clone())])
    }

    fn task(&self) -> PaneChatTask {
        PaneChatTask {
            id: self.task_id.clone(),
            ..task()
        }
    }

    /// TS createCodexTask의 surface 응답. 화면을 읽는 조회의 프레임만 `edit`으로 바꿀 수 있다
    fn serve_frames(
        &self,
        edit: impl Fn(PaneChatSurface) -> PaneChatSurface + Send + Sync + 'static,
    ) {
        let panes = self.panes();
        let task = self.task();
        self.harness
            .surfaces
            .respond(move |_, pane_id, should_capture| {
                let Some(pane) = panes.get(pane_id.unwrap_or("%A")) else {
                    return Ok(None);
                };
                Ok(Some(match should_capture {
                    true => edit(pane.surface(&task)),
                    false => pane.surface_without_screen(&task),
                }))
            });
    }

    /// TS `mocks.surface.mockImplementation(async () => edit(a.surface(task)))`. pane과 캡처 여부와 상관없이 A의 화면을 준다
    fn serve_edited_a_frames(
        &self,
        edit: impl Fn(PaneChatSurface) -> PaneChatSurface + Send + Sync + 'static,
    ) {
        let a = self.a.clone();
        let task = self.task();
        self.harness
            .surfaces
            .respond(move |_, _, _| Ok(Some(edit(a.surface(&task)))));
    }

    fn snapshot(&self, pane_id: &str) -> PaneChatSnapshot {
        self.harness
            .service
            .get_pane_chat_snapshot(&self.task_id, Some(pane_id))
            .unwrap()
    }

    fn send(
        &self,
        token: &str,
        text: &str,
        attachment_ids: &[String],
        request_id: Option<&str>,
    ) -> Result<(), PaneChatError> {
        self.harness.service.send_pane_chat_prompt(
            &self.task_id,
            token,
            text,
            attachment_ids,
            request_id,
        )
    }

    fn reveal(&self) -> Result<PaneChatCodexThreadRevealOutcome, PaneChatError> {
        self.harness
            .service
            .reveal_pane_chat_codex_thread(&self.task_id, Some("%A"))
    }

    fn written_bytes(&self) -> Vec<String> {
        self.harness.bridge.written_bytes()
    }

    fn rollout_call_count(&self) -> usize {
        self.rollout_calls.lock().unwrap().len()
    }

    /// TS `mocks.apiInput.mockImplementationOnce(...)`. 첫 호출만 `first`가 받고, 그다음은 원래 응답기로 간다
    fn once_api_input(
        &self,
        first: impl Fn(
            &CodexThreadDelivery,
            BeforeSend<'_>,
            &mut dyn FnMut(),
            &ApiInputFallback,
        ) -> Result<(), PaneChatError>
        + Send
        + Sync
        + 'static,
    ) {
        let fallback: ApiInputFallback = self.harness.sessions.api_input.lock().unwrap().clone();
        let is_used = AtomicBool::new(false);
        self.harness.sessions.respond_api_input(
            move |delivery, before_send, on_write| match is_used.swap(true, Ordering::SeqCst) {
                true => fallback(delivery, before_send, on_write),
                false => first(delivery, before_send, on_write, &fallback),
            },
        );
    }

    fn stage(&self, token: &str, input: &PaneChatAttachmentInput) -> String {
        self.harness
            .service
            .stage_pane_chat_attachment(&self.task_id, token, input)
            .unwrap()
            .id
    }
}

type ApiInputFallback = Arc<
    dyn Fn(&CodexThreadDelivery, BeforeSend<'_>, &mut dyn FnMut()) -> Result<(), PaneChatError>
        + Send
        + Sync,
>;

fn assert_unbound(snapshot: &PaneChatSnapshot) {
    assert!(!snapshot.can_send, "{snapshot:?}");
    assert_eq!(snapshot.target_token, None);
}

fn assert_bound_to(snapshot: &PaneChatSnapshot, thread_id: &str) {
    assert!(snapshot.can_send, "{snapshot:?}");
    assert_eq!(
        snapshot.transcript.status,
        PaneChatTranscriptStatus::Available
    );
    assert_eq!(snapshot.transcript.session_id.as_deref(), Some(thread_id));
}

fn error_of<T: std::fmt::Debug>(result: Result<T, PaneChatError>) -> &'static str {
    error_name(&result.expect_err("동작이 거절돼야 한다"))
}

// ---- pane process lookup ----

const SCOPED_PROCESS_COMMAND: &str = "pane_tty=$(ps -o tty= -p 100 | tr -d ' '); case \"$pane_tty\" in \"\"|\"?\"|\"??\") ;; *) ps -wwo pid=,ppid=,pgid=,tpgid=,tty=,lstart=,ucomm=,args= -t \"$pane_tty\" ;; esac";

// parity: TS-main-paneChatService-fb80d3
#[test]
fn process_lookup_reads_only_the_pane_root_tty_with_one_scoped_command() {
    for ssh_host in [None, Some("remote-host")] {
        let harness = Harness::new();
        let mut remote = surface();
        remote.task.ssh_host = ssh_host.map(str::to_string);
        harness.surfaces.set(Some(remote));
        harness
            .service
            .get_pane_chat_snapshot("task-chat", Some("%3"))
            .unwrap();
        let lookups = harness.process_lookups();
        assert!(!lookups.is_empty());
        for (command, host) in lookups {
            assert_eq!(command, SCOPED_PROCESS_COMMAND);
            assert_eq!(host.as_deref(), ssh_host);
        }
        assert!(
            !harness
                .runner
                .commands()
                .iter()
                .any(|command| command.contains("-wwaxo"))
        );
    }
}

// parity: TS-main-paneChatService-e02701
#[test]
fn empty_tty_lookup_means_no_cli_and_no_connection() {
    let harness = Harness::new();
    harness.respond_process_table("");
    let snapshot = harness
        .service
        .get_pane_chat_snapshot("task-chat", Some("%3"))
        .unwrap();
    assert_unbound(&snapshot);
    assert_eq!(snapshot.reason.as_deref(), Some("no-foreground-ai-cli"));
    assert_eq!(snapshot.provider, None);
}

// ---- Codex displayed-thread footer binding ----

// parity: TS-main-paneChatService-5099d4
#[test]
fn older_transcript_page_reads_only_the_verified_thread_and_rejects_a_changed_cli() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let bound = fixture.snapshot("%A");
    let token = bound.target_token.clone().unwrap();
    let is_used = AtomicBool::new(false);
    fixture.harness.sessions.respond_detail(move |call| {
        let older = vec![message(kanvibe_ai::types::AiMessageRole::User, "Older X")];
        Ok(Some(match is_used.swap(true, Ordering::SeqCst) {
            false => detail(THREAD_X, AiSessionProvider::Codex, older, Some("200")),
            true => detail(&call.session_id, AiSessionProvider::Codex, Vec::new(), None),
        }))
    });
    let page = fixture
        .harness
        .service
        .get_pane_chat_transcript_page(&fixture.task_id, &token, "100")
        .unwrap();
    assert_eq!(page.session_id.as_deref(), Some(THREAD_X));
    assert_eq!(page.next_cursor.as_deref(), Some("200"));
    assert_eq!(page.messages[0].text, "Older X");
    let last = fixture.harness.sessions.detail_calls().pop().unwrap();
    assert_eq!(
        (
            last.session_id.as_str(),
            last.source_ref,
            last.cursor.as_deref(),
            last.limit
        ),
        (THREAD_X, rollout_file(THREAD_X), Some("100"), 100)
    );
    fixture.a.switch_thread(THREAD_Y);
    let changed =
        fixture
            .harness
            .service
            .get_pane_chat_transcript_page(&fixture.task_id, &token, "200");
    assert_eq!(error_of(changed), "PaneChatTargetChangedError");
}

// parity: TS-main-paneChatService-5cfd8d
#[test]
fn reading_api_model_settings_keeps_the_bound_codex_conversation() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let api_settings = PaneChatModelSettings {
        model: Some("gpt-sol".to_string()),
        effort: Some("low".to_string()),
        is_fast: None,
        models: Vec::new(),
    };
    let returned = api_settings.clone();
    fixture
        .harness
        .sessions
        .respond_thread_settings(move |_| Ok(returned.clone()));
    let bound = fixture.snapshot("%A");
    let token = bound.target_token.clone().unwrap();
    let settings = fixture
        .harness
        .service
        .get_pane_chat_model_settings(&fixture.task_id, &token)
        .unwrap();
    assert_eq!(settings, api_settings);
    let addresses = fixture
        .harness
        .sessions
        .thread_settings_calls
        .lock()
        .unwrap()
        .clone();
    assert!(addresses.iter().any(
        |address| address.thread_id == THREAD_X && address.source_ref == rollout_file(THREAD_X)
    ));
    let again = fixture.snapshot("%A");
    assert!(again.can_send);
    assert_eq!(again.target_token, bound.target_token);
}

// ---- Codex가 선택 화면을 띄운 동안 ----

// parity: TS-main-paneChatService-438ae6
#[test]
fn prompt_covering_the_footer_keeps_the_just_confirmed_thread_and_carries_the_prompt() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let bound = fixture.snapshot("%A");
    fixture.a.with(|state| state.dialog = Some(rows(&APPROVAL)));
    let asking = fixture.snapshot("%A");
    assert_bound_to(&asking, THREAD_X);
    assert_eq!(asking.target_token, bound.target_token);
    let prompt = asking.prompt.expect("선택 화면").screen;
    assert_eq!(prompt.kind, PaneChatPromptKind::Permission);
    assert_eq!(prompt.title, "Would you like to run the following command?");
    assert_eq!(prompt.detail, "$ git tag demo");
    assert_eq!(
        (prompt.options[0].number, prompt.options[0].label.as_str()),
        (1, "Yes, proceed (y)")
    );
    assert_eq!(prompt.options[1].number, 2);
    fixture.a.with(|state| state.dialog = None);
    let closed = fixture.snapshot("%A");
    assert!(closed.can_send);
    assert_eq!(closed.target_token, bound.target_token);
}

// parity: TS-main-paneChatService-7e6408
#[test]
fn prompt_of_a_never_confirmed_codex_is_not_connected() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| {
        state.show_footer = false;
        state.dialog = Some(rows(&APPROVAL));
    });
    assert_unbound(&fixture.snapshot("%A"));
}

// parity: TS-main-paneChatService-466948
#[test]
fn another_thread_id_after_the_prompt_closes_drops_the_held_thread() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let bound = fixture.snapshot("%A");
    fixture
        .a
        .with(|state| state.dialog = Some(rows(&RATE_LIMIT)));
    fixture.snapshot("%A");
    fixture.a.with(|state| state.dialog = None);
    fixture.a.switch_thread(THREAD_Y);
    let after = fixture.snapshot("%A");
    assert_ne!(after.target_token, bound.target_token);
    let interrupted = fixture
        .harness
        .service
        .interrupt_pane_chat(&fixture.task_id, bound.target_token.as_deref().unwrap());
    assert_eq!(error_of(interrupted), "PaneChatTargetChangedError");
}

fn answer(
    fixture: &CodexFixture,
    snapshot: &PaneChatSnapshot,
    answer: serde_json::Value,
) -> Result<(), PaneChatError> {
    fixture.harness.service.answer_pane_chat_prompt(
        &fixture.task_id,
        snapshot.target_token.as_deref().unwrap(),
        &snapshot.prompt.as_ref().unwrap().id,
        &answer,
    )
}

// parity: TS-main-paneChatService-c89687
#[test]
fn card_answers_send_one_number_key_and_cancel_sends_escape() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.snapshot("%A");
    fixture.a.with(|state| state.dialog = Some(rows(&APPROVAL)));
    let asking = fixture.snapshot("%A");
    answer(&fixture, &asking, json!({ "type": "option", "number": 2 })).unwrap();
    fixture.snapshot("%A");
    fixture
        .a
        .with(|state| state.dialog = Some(rows(&RATE_LIMIT)));
    let warned = fixture.snapshot("%A");
    answer(&fixture, &warned, json!({ "type": "cancel" })).unwrap();
    assert_eq!(
        fixture.a.with(|state| state.dialog_keys.clone()),
        vec!["2", "\x1b"]
    );
}

// parity: TS-main-paneChatService-0700e1
#[test]
fn card_answer_is_not_sent_after_the_prompt_changed() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.snapshot("%A");
    fixture.a.with(|state| state.dialog = Some(rows(&APPROVAL)));
    let asking = fixture.snapshot("%A");
    fixture
        .a
        .with(|state| state.dialog = Some(rows(&RATE_LIMIT)));
    let result = answer(&fixture, &asking, json!({ "type": "option", "number": 1 }));
    assert_eq!(error_of(result), "PaneChatTargetChangedError");
    assert!(fixture.a.with(|state| state.dialog_keys.is_empty()));
}

// parity: TS-main-paneChatService-d6bc20
#[test]
fn watching_the_same_prompt_keeps_the_thread_however_long_it_stays() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let bound = fixture.snapshot("%A");
    fixture.a.with(|state| state.dialog = Some(rows(&APPROVAL)));
    for _ in 0..12 {
        fixture.harness.clock.advance(1_500);
        let polled = fixture.snapshot("%A");
        assert!(polled.can_send);
        assert_eq!(polled.target_token, bound.target_token);
    }
}

// parity: TS-main-paneChatService-b85f6c
#[test]
fn stale_thread_confirmation_is_not_carried_behind_a_prompt() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.snapshot("%A");
    fixture.harness.clock.advance(60_000);
    fixture.a.switch_thread(THREAD_Y);
    fixture
        .a
        .with(|state| state.dialog = Some(rows(&RATE_LIMIT)));
    let reopened = fixture.snapshot("%A");
    assert_unbound(&reopened);
    assert_eq!(reopened.prompt, None);
}

// parity: TS-main-paneChatService-13f343
#[test]
fn same_prompt_text_over_a_new_conversation_ends_the_hold() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.snapshot("%A");
    fixture
        .a
        .with(|state| state.dialog = Some(rows(&RATE_LIMIT)));
    fixture.snapshot("%A");
    fixture.a.switch_thread(THREAD_Y);
    fixture
        .a
        .with(|state| state.history = "  >_ New conversation".to_string());
    let reopened = fixture.snapshot("%A");
    assert_unbound(&reopened);
    assert_eq!(reopened.prompt, None);
}

// parity: TS-main-paneChatService-5f88c6
#[test]
fn prompt_refolded_by_a_narrower_pane_keeps_the_thread() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let bound = fixture.snapshot("%A");
    fixture.a.with(|state| {
        state.history =
            "• Earlier reply that is long enough to be folded when the pane gets narrower"
                .to_string();
        state.dialog = Some(rows(&RATE_LIMIT));
    });
    fixture.snapshot("%A");
    fixture.a.with(|state| {
        state.history =
            "• Earlier reply that is long enough to be folded\n  when the pane gets narrower"
                .to_string();
        state.dialog = Some(rows(&[
            "  Approaching rate limits",
            "",
            "",
            "› 1. Switch to",
            "     gpt-6-luna",
            "  2. Keep current model",
            "",
            "  enter select · esc back",
        ]));
        state.rows_scrolled_off = 2;
    });
    let refolded = fixture.snapshot("%A");
    assert!(refolded.can_send, "{refolded:?}");
    assert_eq!(refolded.target_token, bound.target_token);
    assert_eq!(
        refolded.prompt.unwrap().screen.title,
        "Approaching rate limits"
    );
}

// parity: TS-main-paneChatService-6ff4d5
#[test]
fn prompt_with_a_shortened_header_path_keeps_the_thread() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let bound = fixture.snapshot("%A");
    fixture
        .a
        .with(|state| state.dialog = Some(rows(&RATE_LIMIT)));
    fixture.snapshot("%A");
    fixture
        .a
        .with(|state| state.header_directory_line = "     ~/projects/…/run/workspace".to_string());
    let redrawn = fixture.snapshot("%A");
    assert!(redrawn.can_send);
    assert_eq!(redrawn.target_token, bound.target_token);
    assert_eq!(
        redrawn.prompt.unwrap().screen.title,
        "Approaching rate limits"
    );
}

// parity: TS-main-paneChatService-8481bd
#[test]
fn new_conversation_header_right_above_the_prompt_ends_the_hold() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.snapshot("%A");
    fixture
        .a
        .with(|state| state.dialog = Some(rows(&RATE_LIMIT)));
    fixture.snapshot("%A");
    fixture.a.switch_thread(THREAD_Y);
    fixture.a.with(|state| {
        state.history =
            "• Earlier reply\n\n  >_ OpenAI Codex (v0.169.0)\n     ~/projects/qa/run/workspace"
                .to_string();
    });
    assert_unbound(&fixture.snapshot("%A"));
}

// parity: TS-main-paneChatService-8246ff
#[test]
fn same_prompt_with_only_the_highlight_moved_keeps_the_thread() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let bound = fixture.snapshot("%A");
    fixture
        .a
        .with(|state| state.dialog = Some(rows(&RATE_LIMIT)));
    fixture.snapshot("%A");
    let moved: Vec<String> = RATE_LIMIT
        .iter()
        .map(|line| match line {
            _ if line.starts_with("› 1.") => line.replacen('›', " ", 1),
            _ if line.starts_with("  2.") => format!("›{}", &line[1..]),
            _ => line.to_string(),
        })
        .collect();
    fixture.a.with(|state| state.dialog = Some(moved));
    let shown = fixture.snapshot("%A");
    assert!(shown.can_send);
    assert_eq!(shown.target_token, bound.target_token);
    assert_eq!(shown.prompt.unwrap().screen.highlighted_number, 2);
}

// parity: TS-main-paneChatService-fd5868
#[test]
fn switching_to_another_prompt_without_seeing_the_id_ends_the_hold() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.snapshot("%A");
    fixture.a.with(|state| state.dialog = Some(rows(&APPROVAL)));
    fixture.snapshot("%A");
    fixture.a.switch_thread(THREAD_Y);
    fixture
        .a
        .with(|state| state.dialog = Some(rows(&RATE_LIMIT)));
    assert_unbound(&fixture.snapshot("%A"));
}

// ---- 대화 ID 표시(`/statusline thread-id`) ----

// parity: TS-main-paneChatService-342caf
#[test]
fn reveal_pastes_the_command_once_into_the_empty_composer_and_binds_only_that_pane() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| state.show_footer = false);
    fixture.b.with(|state| state.show_footer = false);
    let before = fixture.snapshot("%A");
    assert!(!before.can_send);
    assert_eq!(before.reason.as_deref(), Some("codex-thread-unconfirmed"));
    assert_eq!(
        fixture.reveal().unwrap(),
        PaneChatCodexThreadRevealOutcome::Revealed
    );
    let writes: Vec<(String, String)> = fixture
        .harness
        .writes()
        .into_iter()
        .map(|(_, pane_id, bytes)| (pane_id, bytes))
        .collect();
    assert_eq!(
        writes,
        vec![
            ("%A".to_string(), STATUSLINE_PASTE.to_string()),
            ("%A".to_string(), "\r".to_string())
        ]
    );
    assert_eq!(fixture.a.commands(), vec!["/statusline thread-id"]);
    assert!(fixture.b.commands().is_empty());
    assert_bound_to(&fixture.snapshot("%A"), THREAD_X);
    let sibling = fixture.snapshot("%B");
    assert!(!sibling.can_send);
    assert_eq!(sibling.reason.as_deref(), Some("codex-thread-unconfirmed"));
}

// parity: TS-main-paneChatService-3f73a3
#[test]
fn reveal_writes_nothing_while_the_composer_holds_user_text() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| {
        state.show_footer = false;
        state.composer = "draft in progress".to_string();
    });
    assert_eq!(
        fixture.reveal().unwrap(),
        PaneChatCodexThreadRevealOutcome::InputNotEmpty
    );
    assert!(fixture.harness.writes().is_empty());
    assert_eq!(fixture.a.composer(), "draft in progress");
}

// parity: TS-main-paneChatService-32a655
#[test]
fn reveal_writes_nothing_for_an_unverified_codex_version() {
    let fixture = create_codex_task("codex-cli 0.170.0");
    fixture.a.with(|state| state.show_footer = false);
    assert_eq!(
        fixture.reveal().unwrap(),
        PaneChatCodexThreadRevealOutcome::UnsupportedVersion
    );
    assert!(fixture.harness.writes().is_empty());
}

// parity: TS-main-paneChatService-f5987e
#[test]
fn reveal_never_presses_enter_when_the_pasted_command_is_not_echoed() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| {
        state.show_footer = false;
        state.drops_paste = true;
    });
    assert_eq!(error_of(fixture.reveal()), "PaneChatInputError");
    assert_eq!(fixture.written_bytes(), vec![STATUSLINE_PASTE]);
    assert!(fixture.a.commands().is_empty());
}

// parity: TS-main-paneChatService-c68a4b
#[test]
fn reveal_reports_thread_not_found_when_the_footer_still_has_no_id() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| {
        state.show_footer = false;
        state.ignores_status_line = true;
    });
    assert_eq!(
        fixture.reveal().unwrap(),
        PaneChatCodexThreadRevealOutcome::ThreadNotFound
    );
    assert_eq!(fixture.a.commands(), vec!["/statusline thread-id"]);
}

// parity: TS-main-paneChatService-326f03
#[test]
fn reveal_never_presses_enter_after_the_pane_cli_process_changed() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| state.show_footer = false);
    let panes = fixture.panes();
    let runner = fixture.harness.runner.clone();
    let is_used = AtomicBool::new(false);
    fixture.harness.bridge.respond(move |_, pane_id, bytes| {
        panes[pane_id].write(bytes);
        if !is_used.swap(true, Ordering::SeqCst) {
            let restarted = codex_process_table().replace(
                "Thu Nov 12 19:00:00 2026 codex codex resume",
                "Thu Nov 12 19:30:00 2026 codex codex resume",
            );
            runner.respond(codex_exec_responder(
                restarted,
                "codex-cli 0.169.0".to_string(),
            ));
        }
        Ok(())
    });
    assert_eq!(error_of(fixture.reveal()), "PaneChatTargetChangedError");
    assert_eq!(fixture.harness.writes().len(), 1);
    assert!(fixture.a.commands().is_empty());
}

// parity: TS-main-paneChatService-c6ed4a
#[test]
fn passive_reads_of_an_unconfigured_codex_never_write_statusline() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| state.show_footer = false);
    fixture.snapshot("%A");
    assert!(fixture.harness.writes().is_empty());
}

// parity: TS-main-paneChatService-84ef3b
#[test]
fn configured_footer_is_read_without_writing_into_the_composer() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let snapshot = fixture.snapshot("%A");
    assert!(fixture.harness.writes().is_empty());
    assert!(fixture.a.commands().is_empty());
    assert_bound_to(&snapshot, THREAD_X);
    assert_eq!(snapshot.provider, Some(AiSessionProvider::Codex));
    assert_eq!(snapshot.input_state, PaneChatInputState::Ready);
    assert_eq!(snapshot.reason, None);
    assert!(snapshot.target_token.is_some());
    let calls = fixture.rollout_calls.lock().unwrap().clone();
    assert!(calls.iter().any(|(scope, thread_id)| {
        scope.worktree_path.as_deref() == Some("/repo")
            && scope.ssh_host.is_none()
            && thread_id == THREAD_X
    }));
}

// parity: TS-main-paneChatService-1125f3
#[test]
fn racing_snapshot_reads_never_write_to_the_cli() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.snapshot("%A"));
        let second = scope.spawn(|| fixture.snapshot("%A"));
        first.join().unwrap();
        second.join().unwrap();
    });
    assert!(fixture.a.commands().is_empty());
    assert_bound_to(&fixture.snapshot("%A"), THREAD_X);
    assert!(fixture.a.commands().is_empty());
}

// parity: TS-main-paneChatService-28155d
#[test]
fn idle_codex_with_a_three_row_wrapped_footer_connects_through_the_hook() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| {
        state.show_footer = false;
        // 실제 69칸 pane에서 관측한 초기 상태줄이다.
        state.initial_footer = [
            "  GPT-6.1-Sol high · ~/Documents/github/rookedsysc/naive-first/more-",
            "  naive__worktrees/feat-k3s-settings · 열린 이슈를 워크트리별로 해결",
            "  · Main [default]",
        ]
        .join("\n");
    });
    fixture
        .harness
        .sessions
        .set_binding(Some(binding(THREAD_X, &rollout_file(THREAD_X))));
    let snapshot = fixture.snapshot("%A");
    assert!(fixture.a.commands().is_empty());
    assert_bound_to(&snapshot, THREAD_X);
    assert_eq!(snapshot.input_state, PaneChatInputState::Ready);
    assert!(fixture.a.submissions().is_empty());
}

// parity: TS-main-paneChatService-b5f068
#[test]
fn old_snapshot_whose_lookup_finishes_after_a_newer_thread_is_discarded() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let original = fixture.snapshot("%A");
    let (started_sender, started) = mpsc::channel::<()>();
    let (release, released) = mpsc::channel::<String>();
    let released = Mutex::new(released);
    let started_sender = Mutex::new(started_sender);
    let is_used = AtomicBool::new(false);
    fixture
        .harness
        .sessions
        .respond_rollout(move |_, thread_id| {
            if !is_used.swap(true, Ordering::SeqCst) {
                started_sender.lock().unwrap().send(()).unwrap();
                return Ok(Some(released.lock().unwrap().recv().unwrap()));
            }
            Ok(Some(rollout_file(thread_id)))
        });
    let current = std::thread::scope(|scope| {
        let older_read = scope.spawn(|| fixture.snapshot("%A"));
        started.recv().unwrap();
        fixture.a.switch_thread(THREAD_Y);
        let current = fixture.snapshot("%A");
        assert_eq!(current.transcript.session_id.as_deref(), Some(THREAD_Y));
        release.send(rollout_file(THREAD_X)).unwrap();
        let older = older_read.join().unwrap();
        assert!(!older.can_send);
        assert_eq!(older.reason.as_deref(), Some("target-changed"));
        assert_eq!(older.transcript.session_id, None);
        current
    });
    let stale = fixture.send(original.target_token.as_deref().unwrap(), "Old", &[], None);
    assert_eq!(error_of(stale), "PaneChatTargetChangedError");
    fixture
        .send(
            current.target_token.as_deref().unwrap(),
            "Current Y",
            &[],
            None,
        )
        .unwrap();
    assert_eq!(
        fixture.a.submissions(),
        vec![(THREAD_Y.to_string(), "Current Y".to_string())]
    );
}

// parity: TS-main-paneChatService-1d2575
#[test]
fn fresh_frame_is_checked_after_the_api_lookup_before_admitting_input() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let target = fixture.snapshot("%A");
    let (started_sender, started) = mpsc::channel::<()>();
    let (release, released) = mpsc::channel::<()>();
    let started_sender = Mutex::new(started_sender);
    let released = Mutex::new(released);
    fixture.once_api_input(move |delivery, before_send, on_write, send| {
        started_sender.lock().unwrap().send(()).unwrap();
        released.lock().unwrap().recv().unwrap();
        send(delivery, before_send, on_write)
    });
    fixture.harness.bridge.clear();
    let token = target.target_token.clone().unwrap();
    let result = std::thread::scope(|scope| {
        let sending = scope.spawn(|| fixture.send(&token, "Must not reach Y", &[], None));
        started.recv().unwrap();
        fixture.a.switch_thread(THREAD_Y);
        fixture
            .a
            .with(|state| state.threads_showing_id.insert(THREAD_Y.to_string()));
        release.send(()).unwrap();
        sending.join().unwrap()
    });
    assert_eq!(error_of(result), "PaneChatTargetChangedError");
    assert!(fixture.harness.writes().is_empty());
    assert!(fixture.a.submissions().is_empty());
}

// parity: TS-main-paneChatService-483528
#[test]
fn bound_codex_accepts_api_input_while_working_and_leaves_the_pty_untouched() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let bound = fixture.snapshot("%A");
    fixture.serve_edited_a_frames(|mut frame| {
        if let Some(lines) = frame.viewport_lines.as_mut() {
            *lines.last_mut().unwrap() = "  tab to queue message".to_string();
        }
        frame
    });
    let busy = fixture.snapshot("%A");
    assert_bound_to(&busy, THREAD_X);
    assert_eq!(busy.input_state, PaneChatInputState::Occupied);
    assert_eq!(busy.target_token, bound.target_token);
    fixture.harness.bridge.clear();
    fixture
        .send(busy.target_token.as_deref().unwrap(), "Busy", &[], None)
        .unwrap();
    assert_eq!(fixture.harness.sessions.api_input_calls().len(), 1);
    assert_eq!(
        fixture.a.submissions(),
        vec![(THREAD_X.to_string(), "Busy".to_string())]
    );
    assert!(fixture.harness.writes().is_empty());
}

// parity: TS-main-paneChatService-d08b93
#[test]
fn side_view_hint_is_never_bootstrapped_even_on_a_dim_empty_composer() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.serve_edited_a_frames(|mut frame| {
        let input_line = "› Ask a follow-up question".to_string();
        let cursor_row = frame.cursor_row.unwrap();
        let lines = frame.viewport_lines.as_mut().unwrap();
        lines[cursor_row] = input_line.clone();
        *lines.last_mut().unwrap() =
            "  Side from main thread · ⌃/ to switch · ⌃c to close".to_string();
        frame.input_line = Some(input_line);
        frame
    });
    let side = fixture.snapshot("%A");
    assert_unbound(&side);
    assert_eq!(side.input_state, PaneChatInputState::Occupied);
    assert!(fixture.harness.writes().is_empty());
    assert_eq!(fixture.rollout_call_count(), 0);
}

// parity: TS-main-paneChatService-8dd2a1
#[test]
fn unconfigured_wrapped_footer_without_session_evidence_is_left_untouched() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| state.show_footer = false);
    let a = fixture.a.clone();
    fixture.serve_edited_a_frames(move |mut frame| {
        let is_unconfigured = a.with(|state| {
            state.composer.is_empty() && !state.threads_showing_id.contains(&state.thread_id)
        });
        if is_unconfigured {
            let insert_at = frame.cursor_row.unwrap() + 3;
            let lines = frame.viewport_lines.as_mut().unwrap();
            lines.insert(
                insert_at,
                "  long-worktree-path-wrapped-from-default-footer".to_string(),
            );
        }
        frame
    });
    let snapshot = fixture.snapshot("%A");
    assert!(!snapshot.can_send);
    assert_eq!(snapshot.transcript.session_id, None);
    assert!(fixture.a.commands().is_empty());
}

// parity: TS-main-paneChatService-16f0b3
#[test]
fn same_process_conversation_change_rebinds_without_reviving_the_earlier_token() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let first = fixture.snapshot("%A");
    let sibling = fixture.snapshot("%B");
    assert_eq!(sibling.transcript.session_id.as_deref(), Some(THREAD_Z));
    let first_token = first.target_token.clone().unwrap();

    fixture.a.switch_thread(THREAD_Y);
    assert_eq!(
        error_of(fixture.send(&first_token, "Stale X", &[], None)),
        "PaneChatTargetChangedError"
    );
    let second = fixture.snapshot("%A");
    assert_bound_to(&second, THREAD_Y);
    assert_ne!(second.target_token, first.target_token);
    assert!(fixture.a.commands().is_empty());

    fixture.a.switch_thread(THREAD_X);
    let returned = fixture.snapshot("%A");
    assert_eq!(returned.transcript.session_id.as_deref(), Some(THREAD_X));
    assert_ne!(returned.target_token, first.target_token);
    assert_ne!(returned.target_token, second.target_token);
    assert_eq!(returned.conversation_key, first.conversation_key);
    // X로 돌아오면 이전에 설정한 footer가 복원되므로 명령을 다시 치지 않는다.
    assert!(fixture.a.commands().is_empty());
    let revived = fixture.send(&first_token, "Stale X after return", &[], None);
    assert_eq!(error_of(revived), "PaneChatTargetChangedError");

    fixture
        .send(
            returned.target_token.as_deref().unwrap(),
            "For X",
            &[],
            None,
        )
        .unwrap();
    assert_eq!(
        fixture.a.submissions(),
        vec![(THREAD_X.to_string(), "For X".to_string())]
    );
    assert_eq!(fixture.snapshot("%B").target_token, sibling.target_token);
    assert!(fixture.b.commands().is_empty());
    assert!(fixture.b.submissions().is_empty());
}

// parity: TS-main-paneChatService-e99db0
#[test]
fn multi_line_api_text_reaches_the_bound_thread_unchanged() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let target = fixture.snapshot("%A");
    fixture
        .send(
            target.target_token.as_deref().unwrap(),
            "First line\n\nThird line",
            &[],
            None,
        )
        .unwrap();
    assert_eq!(
        fixture.a.submissions(),
        vec![(THREAD_X.to_string(), "First line\n\nThird line".to_string())]
    );
}

// parity: TS-main-paneChatService-c6d0dc
#[test]
fn modern_codex_message_never_touches_the_pty_composer() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let target = fixture.snapshot("%A");
    fixture.harness.bridge.clear();
    fixture
        .send(
            target.target_token.as_deref().unwrap(),
            "Addressed message",
            &[],
            Some("thread-api-once"),
        )
        .unwrap();
    assert_eq!(
        fixture.a.submissions(),
        vec![(THREAD_X.to_string(), "Addressed message".to_string())]
    );
    assert!(fixture.harness.writes().is_empty());
}

// parity: TS-main-paneChatService-f7546a
#[test]
fn repeated_api_operation_is_admitted_once_and_an_uncertain_result_is_never_retried() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let target = fixture.snapshot("%A");
    let token = target.target_token.clone().unwrap();
    fixture.harness.bridge.clear();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.send(&token, "Once", &[], Some("api-once")));
        let second = scope.spawn(|| fixture.send(&token, "Once", &[], Some("api-once")));
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
    });
    assert_eq!(fixture.harness.sessions.api_input_calls().len(), 1);
    assert_eq!(
        fixture.a.submissions(),
        vec![(THREAD_X.to_string(), "Once".to_string())]
    );
    fixture.once_api_input(|_, before_send, on_write, _| {
        before_send()?;
        on_write();
        Err(PaneChatError::Other("response lost".to_string()))
    });
    for _ in 0..2 {
        let unknown = fixture.send(&token, "Unknown", &[], Some("api-unknown"));
        assert_eq!(error_of(unknown), "PaneChatDeliveryUncertainError");
    }
    assert_eq!(fixture.harness.sessions.api_input_calls().len(), 2);
    assert!(fixture.harness.writes().is_empty());
}

fn text_upload() -> PaneChatAttachmentInput {
    PaneChatAttachmentInput {
        name: "same-note.txt".to_string(),
        mime_type: "text/plain".to_string(),
        data_url: "data:text/plain;base64,aGVsbG8=".to_string(),
    }
}

// parity: TS-main-paneChatService-d3789d
#[test]
fn uncertain_operation_survives_x_y_x_token_renewal_and_attachment_restaging() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let original = fixture.snapshot("%A");
    let original_token = original.target_token.clone().unwrap();
    let attachment = fixture.stage(&original_token, &text_upload());
    fixture.once_api_input(|delivery, before_send, on_write, send| {
        send(delivery, before_send, on_write)?;
        Err(PaneChatError::Other("accepted response lost".to_string()))
    });
    let first = fixture.send(
        &original_token,
        "Once with file",
        std::slice::from_ref(&attachment),
        Some("across-renewal"),
    );
    assert_eq!(error_of(first), "PaneChatDeliveryUncertainError");
    fixture.a.switch_thread(THREAD_Y);
    fixture.snapshot("%A");
    fixture.a.switch_thread(THREAD_X);
    let renewed = fixture.snapshot("%A");
    let renewed_token = renewed.target_token.clone().unwrap();
    let restaged = fixture.stage(&renewed_token, &text_upload());
    assert_ne!(restaged, attachment);
    let retried = fixture.send(
        &renewed_token,
        "Once with file",
        &[restaged],
        Some("across-renewal"),
    );
    assert_eq!(error_of(retried), "PaneChatDeliveryUncertainError");
    assert_eq!(fixture.harness.sessions.api_input_calls().len(), 1);
    assert_eq!(fixture.a.submissions().len(), 1);
}

// parity: TS-main-paneChatService-a2a353
#[test]
fn native_slash_commands_stay_on_the_cli_and_never_reach_the_api() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let target = fixture.snapshot("%A");
    fixture
        .send(target.target_token.as_deref().unwrap(), "/help", &[], None)
        .unwrap();
    assert_eq!(fixture.a.commands(), vec!["/help"]);
    assert!(fixture.harness.sessions.api_input_calls().is_empty());
}

// parity: TS-main-paneChatService-a6c515
#[test]
fn admitted_api_input_stays_pinned_to_x_when_the_terminal_switches_at_the_write_boundary() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture
        .harness
        .sessions
        .set_binding(Some(binding(THREAD_X, &rollout_file(THREAD_X))));
    fixture
        .a
        .with(|state| state.threads_showing_id.insert(THREAD_X.to_string()));
    let target = fixture.snapshot("%A");
    let a = fixture.a.clone();
    fixture.once_api_input(move |delivery, before_send, on_write, _| {
        before_send()?;
        a.switch_thread(THREAD_Y);
        on_write();
        let thread_id = delivery.address.thread_id.clone();
        a.with(|state| {
            state
                .submissions
                .push((thread_id, "Pinned to X".to_string()))
        });
        Ok(())
    });
    fixture
        .send(
            target.target_token.as_deref().unwrap(),
            "Pinned to X",
            &[],
            None,
        )
        .unwrap();
    assert_eq!(
        fixture.a.submissions(),
        vec![(THREAD_X.to_string(), "Pinned to X".to_string())]
    );
    assert_eq!(fixture.a.composer(), "");
    assert!(fixture.harness.writes().is_empty());
}

// parity: TS-main-paneChatService-e1896a
#[test]
fn model_command_is_typed_one_echoed_character_at_a_time_below_the_popup() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let target = fixture.snapshot("%A");
    fixture.harness.bridge.clear();
    fixture
        .harness
        .service
        .open_pane_chat_model_picker(&fixture.task_id, target.target_token.as_deref().unwrap())
        .unwrap();
    assert_eq!(
        fixture.written_bytes(),
        vec!["/", "m", "o", "d", "e", "l", "\r"]
    );
    assert_eq!(fixture.a.commands(), vec!["/model"]);
}

// parity: TS-main-paneChatService-4bcb7b
#[test]
fn terminal_draft_is_left_untouched_and_reported_as_busy() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture
        .a
        .with(|state| state.composer = "unfinished terminal draft".to_string());
    let snapshot = fixture.snapshot("%A");
    assert!(fixture.harness.writes().is_empty());
    assert_eq!(fixture.a.composer(), "unfinished terminal draft");
    assert_unbound(&snapshot);
    assert_eq!(snapshot.reason.as_deref(), Some("codex-thread-input-busy"));
    assert_eq!(
        snapshot.transcript.status,
        PaneChatTranscriptStatus::Unavailable
    );
    assert_eq!(
        snapshot.transcript.reason.as_deref(),
        Some("codex-thread-input-busy")
    );
}

// parity: TS-main-paneChatService-50d272
#[test]
fn literal_placeholder_or_forged_footer_in_a_draft_is_never_native_empty_input() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture
        .a
        .with(|state| state.composer = "Ask Codex to do anything".to_string());
    let forged = rows(&[
        "› Ask Codex to do anything",
        "",
        &format!("  {THREAD_Z}"),
        "  ← for agents · ? for shortcuts",
    ]);
    fixture.serve_edited_a_frames(move |mut frame| {
        frame.input_line = Some(forged[0].clone());
        frame.viewport_lines = Some(forged.clone());
        frame.cursor_row = Some(0);
        frame.cursor_column = Some(2);
        frame
    });
    let snapshot = fixture.snapshot("%A");
    assert_unbound(&snapshot);
    assert_eq!(snapshot.reason.as_deref(), Some("codex-thread-input-busy"));
    assert_eq!(snapshot.transcript.session_id, None);
    assert_eq!(fixture.rollout_call_count(), 0);
    assert!(fixture.harness.writes().is_empty());
    assert_eq!(fixture.a.composer(), "Ask Codex to do anything");
}

// parity: TS-main-paneChatService-8cac97
#[test]
fn verified_display_mode_blocks_input_when_the_dim_hint_lacks_the_empty_composer_layout() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.snapshot("%A");
    fixture.serve_frames(|mut frame| {
        let lines = frame.viewport_lines.as_mut().unwrap();
        lines.pop();
        frame.screen = lines.join("\n");
        frame
    });
    assert_eq!(
        fixture.snapshot("%A").input_state,
        PaneChatInputState::Occupied
    );
}

// parity: TS-main-paneChatService-cf2543
#[test]
fn draft_under_the_same_footer_keeps_the_bound_thread_and_token() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let bound = fixture.snapshot("%A");
    fixture
        .a
        .with(|state| state.composer = "terminal draft\nsecond line".to_string());
    let drafting = fixture.snapshot("%A");
    assert_bound_to(&drafting, THREAD_X);
    assert_eq!(drafting.input_state, PaneChatInputState::Occupied);
    assert_eq!(drafting.target_token, bound.target_token);
    assert!(fixture.a.commands().is_empty());
}

// parity: TS-main-paneChatService-a04ff9
#[test]
fn uuid_in_a_multi_line_draft_is_not_adopted_as_the_displayed_thread() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    // 표시 줄을 꺼 둔 CLI에서 입력 본문의 마지막 줄이 footer와 같은 모양으로 화면 맨 아래에 놓인다.
    fixture.a.with(|state| {
        state.composer = format!("draft\n\n{THREAD_Z}");
        state.is_cursor_on_first_composer_row = true;
    });
    fixture.serve_edited_a_frames(|mut frame| {
        let lines = frame.viewport_lines.as_mut().unwrap();
        lines.truncate(lines.len() - 2);
        frame.screen = lines.join("\n");
        frame.input_line = lines.get(frame.cursor_row.unwrap()).cloned();
        frame
    });
    let snapshot = fixture.snapshot("%A");
    assert_unbound(&snapshot);
    assert_eq!(snapshot.reason.as_deref(), Some("codex-thread-input-busy"));
    assert_eq!(
        snapshot.transcript.status,
        PaneChatTranscriptStatus::Unavailable
    );
    assert_eq!(snapshot.transcript.session_id, None);
    assert_eq!(fixture.rollout_call_count(), 0);
    assert!(fixture.harness.writes().is_empty());
}

// parity: TS-main-paneChatService-ebc9ad
#[test]
fn repeated_unconfirmed_reads_never_attempt_the_footer_command() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture.a.with(|state| state.show_footer = false);
    let reads: Vec<PaneChatSnapshot> = (0..5).map(|_| fixture.snapshot("%A")).collect();
    assert!(fixture.a.commands().is_empty());
    let last = reads.last().unwrap();
    assert_unbound(last);
    assert_eq!(last.reason.as_deref(), Some("codex-thread-unconfirmed"));
}

// parity: TS-main-paneChatService-94acf8
#[test]
fn displayed_thread_without_a_main_record_in_this_task_is_refused() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    fixture
        .a
        .with(|state| state.threads_showing_id.insert(THREAD_X.to_string()));
    // 하위 에이전트 화면이거나 다른 작업에서 시작한 대화다.
    fixture.harness.sessions.respond_rollout(|_, _| Ok(None));
    let snapshot = fixture.snapshot("%A");
    assert!(fixture.harness.writes().is_empty());
    assert_unbound(&snapshot);
    assert_eq!(
        snapshot.reason.as_deref(),
        Some("codex-thread-record-missing")
    );
    assert_eq!(
        snapshot.transcript.status,
        PaneChatTranscriptStatus::Unavailable
    );
    assert!(snapshot.transcript.messages.is_empty());
    assert!(fixture.harness.sessions.detail_calls().is_empty());
}

// parity: TS-main-paneChatService-a32694
#[test]
fn staged_image_is_submitted_as_addressed_input() {
    for caption in ["caption after image", ""] {
        let fixture = create_codex_task("codex-cli 0.169.0");
        let target = fixture.snapshot("%A");
        let token = target.target_token.clone().unwrap();
        let image = PaneChatAttachmentInput {
            name: "image.png".to_string(),
            mime_type: "image/png".to_string(),
            data_url: format!("data:image/png;base64,{PNG_BASE64}"),
        };
        let attachment = fixture.stage(&token, &image);
        fixture
            .send(&token, caption, std::slice::from_ref(&attachment), None)
            .unwrap();
        let first_input = fixture.harness.sessions.api_input_calls()[0].input[0].clone();
        assert!(
            matches!(&first_input, CodexUserInput::LocalImage { path } if path.ends_with(".png"))
        );
        assert_eq!(
            fixture.a.submissions(),
            vec![(THREAD_X.to_string(), format!("[Image #1] {caption}"))]
        );
        fixture
            .harness
            .service
            .remove_pane_chat_attachment(&fixture.task_id, &token, &attachment)
            .unwrap();
    }
}

// parity: TS-main-paneChatService-a8896f
#[test]
fn long_prompt_is_sent_as_exact_api_text_without_placeholders() {
    let fixture = create_codex_task("codex-cli 0.169.0");
    let target = fixture.snapshot("%A");
    let long = "L".repeat(1500);
    fixture
        .send(target.target_token.as_deref().unwrap(), &long, &[], None)
        .unwrap();
    assert_eq!(fixture.a.submissions(), vec![(THREAD_X.to_string(), long)]);
}

// parity: TS-main-paneChatService-b05e4e
#[test]
fn unverified_codex_versions_keep_the_hook_path_and_type_nothing() {
    for version in ["codex-cli 0.168.0", "codex-cli 0.170.0"] {
        let fixture = create_codex_task(version);
        fixture
            .harness
            .sessions
            .set_binding(Some(binding("hook-session", "/allowed/hook.jsonl")));
        let snapshot = fixture.snapshot("%A");
        assert!(fixture.harness.writes().is_empty());
        assert_eq!(
            snapshot.transcript.status,
            PaneChatTranscriptStatus::Available
        );
        assert_eq!(
            snapshot.transcript.session_id.as_deref(),
            Some("hook-session")
        );
    }
}
