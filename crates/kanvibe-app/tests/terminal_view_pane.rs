//! 공용 터미널 뷰(TS `Terminal`)를 헤드리스 GPUI로 띄워, PTY로 나간 요청과 화면에 보이는 상태로 TS 동작을 확인한다.
//! 입출력은 기록용 전송 길로 받고, 응답 지연·전송 실패는 TS 채널 이름의 QA 고장 주입으로 건다.

use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::gpui;
use gpui_kit::{
    Bounds, ClipboardItem, Entity, Focusable, Image, ImageFormat, Modifiers, MouseButton, Pixels,
    TestAppContext, VisualTestContext, WeakEntity,
};
use kanvibe_app::components::terminal_view::window::ViewTerminalWindow;
use kanvibe_app::components::terminal_view::{
    ImagePasteOutcome, TerminalPane, TerminalTarget, TerminalTransport,
};
use kanvibe_app::qa::faults::{FaultMode, ServiceFault};
use kanvibe_app::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions, QaOptions};
use kanvibe_app::qa::targets::{DescribedTarget, QaTargetRegistry, TargetQuery};
use kanvibe_app::services::AppServices;
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::terminal_tabs::HERDR_SERVER_REPLACED_CLOSE_REASON;
use kanvibe_terminal::terminal_bridge::{TerminalBridgeError, TerminalOpenResult, TerminalWindow};

const CONNECTING: &str = "터미널에 연결하고 있습니다…";
const RECONNECTING: &str = "SSH 연결을 복구하고 있습니다. 잠시만 기다려 주세요…";
const RECONNECT_PAUSED: &str = "자동 재연결이 일시 중지되었습니다. 다시 시도해 주세요.";
const RETRY: &str = "다시 연결";

/// TS `window.kanvibeDesktop` 목 자리. 열기 응답은 차례로 꺼내고, 비면 성공이다
#[derive(Default)]
struct RecordingTransport {
    open_results: Mutex<VecDeque<TerminalOpenResult>>,
    opens: Mutex<Vec<(String, Option<String>)>>,
    writes: Mutex<Vec<String>>,
    resizes: Mutex<Vec<(u16, u16)>>,
    closes: Mutex<Vec<(String, Option<String>)>>,
    paste_outcomes: Mutex<VecDeque<ImagePasteOutcome>>,
    paste_paths: Mutex<HashMap<String, String>>,
    pastes: Mutex<Vec<String>>,
    windows: Mutex<Vec<Arc<ViewTerminalWindow>>>,
}

impl RecordingTransport {
    fn with_open_results(results: Vec<TerminalOpenResult>) -> Arc<Self> {
        let transport = Self::default();
        *transport.open_results.lock().unwrap() = results.into();
        Arc::new(transport)
    }

    fn open_count(&self) -> usize {
        self.opens.lock().unwrap().len()
    }

    fn writes(&self) -> Vec<String> {
        self.writes.lock().unwrap().clone()
    }

    fn pastes(&self) -> Vec<String> {
        self.pastes.lock().unwrap().clone()
    }

    fn close_count(&self) -> usize {
        self.closes.lock().unwrap().len()
    }
}

impl TerminalTransport for RecordingTransport {
    fn open(
        &self,
        window: Arc<ViewTerminalWindow>,
        target: &TerminalTarget,
        _columns: u16,
        _rows: u16,
    ) -> TerminalOpenResult {
        self.windows.lock().unwrap().push(window);
        self.opens
            .lock()
            .unwrap()
            .push((target.task_id.clone(), target.tab_id.clone()));
        self.open_results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(()))
    }

    fn write(&self, _window_id: u64, _target: &TerminalTarget, data: &str) {
        self.writes.lock().unwrap().push(data.to_string());
    }

    fn resize(&self, _window_id: u64, _target: &TerminalTarget, columns: u16, rows: u16) {
        self.resizes.lock().unwrap().push((columns, rows));
    }

    fn close(&self, _window_id: u64, target: &TerminalTarget) {
        self.closes
            .lock()
            .unwrap()
            .push((target.task_id.clone(), target.tab_id.clone()));
    }

    fn paste_image(&self, _task_id: &str, image_data_url: &str) -> ImagePasteOutcome {
        self.pastes.lock().unwrap().push(image_data_url.to_string());
        if let Some(outcome) = self.paste_outcomes.lock().unwrap().pop_front() {
            return outcome;
        }
        let path = self
            .paste_paths
            .lock()
            .unwrap()
            .get(image_data_url)
            .cloned();
        ImagePasteOutcome::Pasted(path.unwrap_or_else(|| "/tmp/kanvibe-paste.png".into()))
    }
}

fn cooldown(message: &str, wait_ms: u32) -> TerminalOpenResult {
    Err(TerminalBridgeError {
        message: message.into(),
        retry_after_ms: NonZeroU32::new(wait_ms),
    })
}

fn init_app(cx: &mut TestAppContext, data_dir: PathBuf) {
    let launch = LaunchOptions {
        data_dir: Some(data_dir),
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: Some(QaOptions {
            fixture_root: None,
            fixed_now_ms: 1_768_445_820_000,
            network_responses_path: None,
            faults_path: None,
            control_socket_path: None,
            platform_override: None,
            loading_holds: Default::default(),
        }),
    };
    cx.update(|cx| {
        kanvibe_app::init_app(&launch, cx).expect("app init");
        if !cx.has_global::<QaTargetRegistry>() {
            cx.set_global(QaTargetRegistry::default());
        }
    });
}

fn add_fault(cx: &mut TestAppContext, fault: ServiceFault) {
    cx.update(|cx| {
        cx.global::<AppServices>()
            .faults()
            .expect("qa faults")
            .add(fault)
    });
}

fn fault(target: &str, mode: FaultMode, ms: Option<u64>) -> ServiceFault {
    ServiceFault {
        target: target.into(),
        mode,
        message: None,
        ms,
        value: None,
        after_calls: None,
        argument_match: None,
    }
}

struct Pane {
    view: WeakEntity<TerminalPane>,
    cx: VisualTestContext,
}

fn target(task_id: &str, tab_id: Option<&str>, is_remote: bool) -> TerminalTarget {
    TerminalTarget {
        task_id: task_id.into(),
        tab_id: tab_id.map(str::to_string),
        is_remote,
    }
}

/// 창에 터미널 하나를 띄운다. 뷰는 창만 쥐므로 창을 닫으면 뷰도 사라진다(언마운트)
fn open_pane(
    cx: &mut TestAppContext,
    target: TerminalTarget,
    transport: &Arc<RecordingTransport>,
) -> Pane {
    let transport: Arc<dyn TerminalTransport> = transport.clone();
    let translator = Translator::new(Locale::Ko, "taskDetail");
    let (view, visual) = cx.add_window_view(|_, cx| {
        TerminalPane::with_transport(target, translator, Some(transport), cx)
    });
    let mut pane = Pane {
        view: view.downgrade(),
        cx: visual.clone(),
    };
    drop(view);
    pane.draw();
    pane
}

impl Pane {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| {
            let window_id = window.window_handle().window_id();
            cx.global_mut::<QaTargetRegistry>().begin_frame(window_id);
            window.refresh();
        });
        self.cx.run_until_parked();
    }

    fn advance(&mut self, duration: Duration) {
        self.cx.executor().advance_clock(duration);
        self.draw();
    }

    fn entity(&self) -> Entity<TerminalPane> {
        self.view.upgrade().expect("pane is alive")
    }

    fn find(&mut self, query: TargetQuery) -> Option<Bounds<Pixels>> {
        self.cx.update(|window, cx| {
            cx.global::<QaTargetRegistry>()
                .find(window.window_handle().window_id(), &query)
        })
    }

    fn has_role_text(&mut self, role: &str, text: &str) -> bool {
        self.find(TargetQuery::Described(DescribedTarget {
            role: Some(role.into()),
            name: Some(text.into()),
            exact: Some(false),
            ..DescribedTarget::default()
        }))
        .is_some()
    }

    fn has_status(&mut self) -> bool {
        self.find(TargetQuery::Described(DescribedTarget {
            role: Some("status".into()),
            ..DescribedTarget::default()
        }))
        .is_some()
    }

    fn has_alert(&mut self) -> bool {
        self.find(TargetQuery::Described(DescribedTarget {
            role: Some("alert".into()),
            ..DescribedTarget::default()
        }))
        .is_some()
    }

    fn click_button(&mut self, name: &str) {
        let bounds = self
            .find(TargetQuery::Described(DescribedTarget {
                role: Some("button".into()),
                name: Some(name.into()),
                exact: Some(true),
                ..DescribedTarget::default()
            }))
            .unwrap_or_else(|| panic!("button {name} not found"));
        self.cx
            .simulate_click(bounds.center(), Modifiers::default());
        self.draw();
    }

    fn focus(&mut self) {
        let pane = self.entity();
        self.cx.update(|window, cx| {
            let handle = pane.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        });
        self.draw();
    }

    fn keys(&mut self, keystrokes: &str) {
        self.focus();
        self.cx.simulate_keystrokes(keystrokes);
        self.draw();
    }

    fn middle_click(&mut self) {
        let center = self.cx.update(|window, _| {
            let size = window.viewport_size();
            gpui_kit::point(size.width / 2.0, size.height / 2.0)
        });
        self.cx
            .simulate_mouse_down(center, MouseButton::Middle, Modifiers::default());
        self.cx
            .simulate_mouse_up(center, MouseButton::Middle, Modifiers::default());
        self.draw();
    }

    fn close_window(&mut self) {
        self.cx.update(|window, _| window.remove_window());
        self.cx.run_until_parked();
    }
}

fn copy_image(cx: &mut TestAppContext, bytes: &[u8]) {
    let image = Image::from_bytes(ImageFormat::Png, bytes.to_vec());
    cx.write_to_clipboard(ClipboardItem::new_image(&image));
}

fn copy_text(cx: &mut TestAppContext, text: &str) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.into()));
}

fn data_url(bytes: &[u8]) -> String {
    let encoded: String = {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut text = String::new();
        for chunk in bytes.chunks(3) {
            let value = chunk.iter().enumerate().fold(0u32, |value, (index, byte)| {
                value | u32::from(*byte) << (16 - 8 * index)
            });
            for index in 0..4 {
                if index <= chunk.len() {
                    text.push(char::from(
                        ALPHABET[((value >> (18 - 6 * index)) & 0x3f) as usize],
                    ));
                } else {
                    text.push('=');
                }
            }
        }
        text
    };
    format!("data:image/png;base64,{encoded}")
}

// ───────────── 연결 ─────────────

#[gpui::test]
fn shows_a_connecting_status_until_the_open_answer_arrives(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    add_fault(
        cx,
        fault("kanvibe:terminal-open", FaultMode::Delay, Some(1_000)),
    );
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("loading", None, false), &transport);
    assert!(pane.has_role_text("status", CONNECTING));
    pane.advance(Duration::from_millis(1_000));
    assert!(!pane.has_status());
    assert_eq!(transport.open_count(), 1);
}

#[gpui::test]
fn cooldown_shows_reconnecting_and_opens_once_more_after_the_wait(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = RecordingTransport::with_open_results(vec![cooldown("SSH cooldown", 1_200)]);
    let mut pane = open_pane(cx, target("cooldown", None, false), &transport);
    assert!(pane.has_role_text("status", RECONNECTING));
    assert!(!pane.has_alert());
    pane.advance(Duration::from_millis(1_199));
    assert_eq!(transport.open_count(), 1);
    pane.advance(Duration::from_millis(1));
    assert_eq!(transport.open_count(), 2);
    assert!(!pane.has_status());
}

#[gpui::test]
fn a_failed_reconnect_stops_and_offers_a_manual_retry(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = RecordingTransport::with_open_results(vec![
        cooldown("SSH cooldown", 1),
        cooldown("SSH cooldown", 1),
    ]);
    let mut pane = open_pane(cx, target("exhausted", None, false), &transport);
    pane.advance(Duration::from_millis(5));
    assert!(pane.has_role_text("alert", "SSH cooldown"));
    assert_eq!(transport.open_count(), 2);
    assert!(!pane.has_status());
    pane.click_button(RETRY);
    assert_eq!(transport.open_count(), 3);
    assert!(!pane.has_alert());
    assert_eq!(transport.close_count(), 1);
}

#[gpui::test]
fn a_real_error_is_shown_at_once_without_an_automatic_retry(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport =
        RecordingTransport::with_open_results(vec![Err(TerminalBridgeError::new("tmux missing"))]);
    let mut pane = open_pane(cx, target("permanent", None, false), &transport);
    pane.advance(Duration::from_secs(5));
    assert!(pane.has_role_text("alert", "tmux missing"));
    assert_eq!(transport.open_count(), 1);
}

#[gpui::test]
fn closing_during_the_cooldown_cancels_the_retry_and_cleans_up_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = RecordingTransport::with_open_results(vec![cooldown("SSH cooldown", 1_000)]);
    let mut pane = open_pane(cx, target("cancel", None, false), &transport);
    pane.close_window();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(pane.view.upgrade().is_none());
    assert_eq!(transport.open_count(), 1);
    assert_eq!(transport.close_count(), 1);
}

#[gpui::test]
fn hiding_a_tab_pauses_the_retry_and_showing_it_offers_a_manual_retry(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = RecordingTransport::with_open_results(vec![cooldown("SSH cooldown", 1_000)]);
    let mut pane = open_pane(cx, target("hidden", Some("tab-1"), false), &transport);
    let entity = pane.entity();
    pane.cx
        .update(|_, cx| entity.update(cx, |pane, cx| pane.set_hidden(true, cx)));
    pane.advance(Duration::from_secs(2));
    assert_eq!(transport.open_count(), 1);
    assert!(!pane.has_status());
    pane.cx
        .update(|_, cx| entity.update(cx, |pane, cx| pane.set_hidden(false, cx)));
    pane.draw();
    assert!(pane.has_role_text("alert", RECONNECT_PAUSED));
    pane.click_button(RETRY);
    assert_eq!(transport.open_count(), 2);
    assert!(!pane.has_alert());
}

#[gpui::test]
fn switching_task_tab_or_session_cancels_the_previous_retry(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let next_targets = [
        target("other-task", Some("tab-1"), false),
        target("switch", Some("tab-2"), false),
        target("switch", Some("tab-1"), false),
    ];
    for next in next_targets {
        let transport =
            RecordingTransport::with_open_results(vec![cooldown("SSH cooldown", 1_000)]);
        let mut previous = open_pane(cx, target("switch", Some("tab-1"), false), &transport);
        previous.close_window();
        let mut replacement = open_pane(cx, next.clone(), &transport);
        replacement.advance(Duration::from_secs(2));
        let opens = transport.opens.lock().unwrap().clone();
        assert_eq!(
            opens,
            vec![
                ("switch".to_string(), Some("tab-1".to_string())),
                (next.task_id.clone(), next.tab_id.clone()),
            ]
        );
        assert!(
            transport
                .closes
                .lock()
                .unwrap()
                .contains(&("switch".to_string(), Some("tab-1".to_string())))
        );
    }
}

#[gpui::test]
fn coming_back_to_the_window_reports_the_terminal_size_again(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, false), &transport);
    transport.resizes.lock().unwrap().clear();
    pane.cx.deactivate_window();
    pane.cx.update(|window, _| window.activate_window());
    pane.draw();
    assert_eq!(transport.resizes.lock().unwrap().len(), 1);
}

#[gpui::test]
fn herdr_server_replacement_reconnects_without_an_error_line(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("handoff", None, false), &transport);
    assert_eq!(transport.open_count(), 1);
    let window = transport.windows.lock().unwrap()[0].clone();
    window.send_terminal_closed("handoff", None, Some(HERDR_SERVER_REPLACED_CLOSE_REASON));
    pane.draw();
    assert_eq!(transport.open_count(), 2);
    assert!(!pane.has_role_text("row", "연결이 종료되었습니다."));
    assert!(!pane.has_alert());
}

#[gpui::test]
fn closing_while_the_open_is_pending_releases_the_terminal_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    add_fault(
        cx,
        fault("kanvibe:terminal-open", FaultMode::Delay, Some(1_000)),
    );
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("pending", None, false), &transport);
    pane.close_window();
    assert!(pane.view.upgrade().is_none());
    assert_eq!(transport.close_count(), 1);
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(transport.open_count(), 0);
    assert_eq!(transport.close_count(), 1);
}

// ───────────── 원격 이미지 붙여넣기 ─────────────

#[gpui::test]
fn remote_paste_with_an_image_sends_it_and_types_the_remote_path(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    transport.paste_paths.lock().unwrap().insert(
        data_url(b"fake"),
        "/tmp/kanvibe-paste-fixed-uuid.png".into(),
    );
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_image(cx, b"fake");
    pane.keys("ctrl-shift-v");
    assert_eq!(transport.pastes(), vec![data_url(b"fake")]);
    assert_eq!(
        transport.writes(),
        vec!["/tmp/kanvibe-paste-fixed-uuid.png"]
    );
}

#[gpui::test]
fn a_rejected_remote_image_shows_the_reason_in_the_terminal(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    transport
        .paste_outcomes
        .lock()
        .unwrap()
        .push_back(ImagePasteOutcome::Rejected("원격 세션이 아닙니다.".into()));
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_image(cx, b"fake");
    pane.keys("ctrl-shift-v");
    assert!(pane.has_role_text("row", "원격 세션이 아닙니다."));
    assert!(transport.writes().is_empty());
}

#[gpui::test]
fn a_progress_toast_stays_while_the_image_is_sent(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    add_fault(
        cx,
        fault(
            "kanvibe:terminal-paste-image",
            FaultMode::Delay,
            Some(1_000),
        ),
    );
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_image(cx, b"fake");
    pane.keys("ctrl-shift-v");
    assert!(pane.has_role_text("button", "닫기"));
    pane.advance(Duration::from_millis(1_000));
    assert!(!pane.has_role_text("button", "닫기"));
    assert_eq!(transport.writes(), vec!["/tmp/kanvibe-paste.png"]);
}

#[gpui::test]
fn a_failed_image_request_offers_resending_the_same_image(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    transport.paste_outcomes.lock().unwrap().extend([
        ImagePasteOutcome::RequestFailed("scp failed".into()),
        ImagePasteOutcome::Pasted("/tmp/retry.png".into()),
    ]);
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_image(cx, b"fake");
    pane.keys("ctrl-shift-v");
    pane.click_button("다시 보내기");
    assert_eq!(transport.writes(), vec!["/tmp/retry.png"]);
    assert_eq!(
        transport.pastes(),
        vec![data_url(b"fake"), data_url(b"fake")]
    );
}

#[gpui::test]
fn two_images_type_their_paths_in_paste_order(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let mut slow_first = fault(
        "kanvibe:terminal-paste-image",
        FaultMode::Delay,
        Some(1_000),
    );
    slow_first.argument_match = Some(data_url(b"first"));
    add_fault(cx, slow_first);
    let transport = Arc::new(RecordingTransport::default());
    transport.paste_paths.lock().unwrap().extend([
        (data_url(b"first"), "/tmp/first.png".to_string()),
        (data_url(b"second"), "/tmp/second.png".to_string()),
    ]);
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_image(cx, b"first");
    pane.keys("ctrl-shift-v");
    copy_image(cx, b"second");
    pane.keys("ctrl-shift-v");
    pane.advance(Duration::from_millis(1_000));
    assert_eq!(
        transport.writes(),
        vec!["/tmp/first.png", "/tmp/second.png"]
    );
}

#[gpui::test]
fn remote_text_paste_goes_to_the_terminal_as_text(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_text(cx, "echo hi");
    pane.keys("ctrl-shift-v");
    assert!(transport.pastes().is_empty());
    assert_eq!(transport.writes(), vec!["echo hi"]);
}

#[gpui::test]
fn local_paste_never_sends_images(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, false), &transport);
    copy_image(cx, b"fake");
    pane.keys("ctrl-shift-v");
    assert!(transport.pastes().is_empty());
}

#[gpui::test]
fn remote_ctrl_v_with_an_image_sends_the_image_instead_of_the_key(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_image(cx, b"fake");
    pane.keys("ctrl-v");
    assert_eq!(transport.pastes(), vec![data_url(b"fake")]);
    assert!(!transport.writes().contains(&"\x16".to_string()));
}

#[gpui::test]
fn remote_ctrl_v_without_an_image_keeps_the_terminal_key(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_text(cx, "not an image");
    pane.keys("ctrl-v");
    assert!(transport.pastes().is_empty());
    assert_eq!(transport.writes(), vec!["\x16"]);
}

#[gpui::test]
fn local_ctrl_v_keeps_the_terminal_key_even_with_an_image(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, false), &transport);
    copy_image(cx, b"fake");
    pane.keys("ctrl-v");
    assert!(transport.pastes().is_empty());
    assert_eq!(transport.writes(), vec!["\x16"]);
}

#[gpui::test]
fn remote_ctrl_shift_v_and_shift_insert_also_send_images(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_image(cx, b"fake");
    pane.keys("ctrl-shift-v");
    pane.keys("shift-insert");
    assert_eq!(transport.pastes().len(), 2);
}

#[gpui::test]
fn remote_middle_click_sends_the_clipboard_image(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_image(cx, b"fake");
    pane.middle_click();
    assert_eq!(transport.pastes(), vec![data_url(b"fake")]);
}

#[gpui::test]
fn remote_middle_click_without_an_image_does_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, true), &transport);
    copy_text(cx, "plain");
    pane.middle_click();
    assert!(transport.pastes().is_empty());
    assert!(transport.writes().is_empty());
}

#[gpui::test]
fn local_middle_click_ignores_clipboard_images(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    init_app(cx, dir.path().to_path_buf());
    let transport = Arc::new(RecordingTransport::default());
    let mut pane = open_pane(cx, target("task-1", None, false), &transport);
    copy_image(cx, b"fake");
    pane.middle_click();
    assert!(transport.pastes().is_empty());
}
