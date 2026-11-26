//! diff 화면 동작 테스트. 서비스는 응답 순서를 쥘 수 있는 가짜로 바꾸고, 화면에 그려진 글자(QA 대상 기록)와 서비스 호출을 본다.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::{
    App, AppContext, Entity, Modifiers, Task, TestAppContext, VisualTestContext, WeakEntity, gpui,
};
use kanvibe_domain::i18n::Locale;
use kanvibe_domain::task::{KanbanTask, TaskStatus};
use kanvibe_domain::task_diff_stats::{DiffFile, DiffFileStatus};

use super::TaskDiffView;
use super::file_editor::{DiffFileEditor, SaveHandler};
use super::file_tree::{
    FileRowState, file_name_is_struck_through, file_row_icon, file_row_text_color,
};
use super::icons::DiffIcon;
use super::page::DiffPage;
use super::source::{DiffSource, SharedDiffSource, SourceResult};
use crate::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};
use crate::qa::targets::{QaTargetRegistry, TargetQuery};
use crate::routes::{RouteResolution, resolve_route_path};
use crate::theme::ThemeTokens;
use crate::views::{ScreenContext, WindowNavigator};

/// 가짜 서비스가 받은 호출
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Task(String),
    Files(String),
    Original(String, String),
    Modified(String, String),
    Save(String, String, String),
}

type Reply = SourceResult<String>;
/// 호출마다 바로 줄 응답. `None`이면 테스트가 `resolve`로 나중에 준다
type Responder = Box<dyn Fn(&Call) -> Option<Reply>>;

enum TaskReply {
    Ready(Box<SourceResult<Option<KanbanTask>>>),
    Hang,
}

struct FakeDiffSource {
    calls: RefCell<Vec<Call>>,
    responder: RefCell<Responder>,
    pending: RefCell<Vec<(Call, oneshot::Sender<Reply>)>>,
    task_replies: RefCell<VecDeque<TaskReply>>,
    files: RefCell<Vec<DiffFile>>,
}

impl FakeDiffSource {
    fn new(responder: impl Fn(&Call) -> Option<Reply> + 'static) -> Rc<Self> {
        Rc::new(Self {
            calls: RefCell::default(),
            responder: RefCell::new(Box::new(responder)),
            pending: RefCell::default(),
            task_replies: RefCell::default(),
            files: RefCell::default(),
        })
    }

    /// 원본은 "original", 현재 내용은 "modified", 저장은 성공
    fn immediate() -> Rc<Self> {
        Self::new(|call| {
            Some(Ok(match call {
                Call::Original(..) => "original".to_string(),
                _ => "modified".to_string(),
            }))
        })
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }

    fn resolve(&self, matches: impl Fn(&Call) -> bool, reply: Reply) {
        let index = self
            .pending
            .borrow()
            .iter()
            .position(|(call, _)| matches(call))
            .expect("pending call");
        let (_, sender) = self.pending.borrow_mut().remove(index);
        let _ = sender.send(reply);
    }

    fn reply(&self, call: Call, cx: &App) -> Task<Reply> {
        self.calls.borrow_mut().push(call.clone());
        if let Some(reply) = (self.responder.borrow())(&call) {
            return Task::ready(reply);
        }
        let (sender, receiver) = oneshot::channel();
        self.pending.borrow_mut().push((call, sender));
        cx.background_spawn(async move { receiver.await.unwrap_or_else(|_| Err("dropped".into())) })
    }
}

impl DiffSource for FakeDiffSource {
    fn load_task(&self, task_id: &str, cx: &App) -> Task<SourceResult<Option<KanbanTask>>> {
        self.calls.borrow_mut().push(Call::Task(task_id.into()));
        match self.task_replies.borrow_mut().pop_front() {
            Some(TaskReply::Ready(reply)) => Task::ready(*reply),
            // TS `new Promise(() => {})`: 영영 끝나지 않는 조회
            Some(TaskReply::Hang) | None => cx.background_spawn(std::future::pending()),
        }
    }

    fn load_files(&self, task_id: &str, _: &App) -> Task<SourceResult<Vec<DiffFile>>> {
        self.calls.borrow_mut().push(Call::Files(task_id.into()));
        Task::ready(Ok(self.files.borrow().clone()))
    }

    fn read_original(
        &self,
        task_id: &str,
        file_path: &str,
        cx: &App,
    ) -> Task<SourceResult<String>> {
        self.reply(Call::Original(task_id.into(), file_path.into()), cx)
    }

    fn read_modified(
        &self,
        task_id: &str,
        file_path: &str,
        cx: &App,
    ) -> Task<SourceResult<String>> {
        self.reply(Call::Modified(task_id.into(), file_path.into()), cx)
    }

    fn save(
        &self,
        task_id: &str,
        file_path: &str,
        content: String,
        cx: &App,
    ) -> Task<SourceResult<()>> {
        let reply = self.reply(Call::Save(task_id.into(), file_path.into(), content), cx);
        cx.background_spawn(async move { reply.await.map(|_| ()) })
    }
}

fn init(cx: &mut TestAppContext) {
    let launch = LaunchOptions {
        data_dir: None,
        initial_route: "/ko".into(),
        window_size: DEFAULT_WINDOW_SIZE,
        qa: None,
    };
    cx.update(|cx| {
        crate::init_app(&launch, cx).expect("app init");
        cx.set_global(QaTargetRegistry::default());
    });
}

/// 새 프레임을 그려 이번에 보이는 글자만 남긴다
fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        let window_id = window.window_handle().window_id();
        cx.global_mut::<QaTargetRegistry>().begin_frame(window_id);
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn find(
    cx: &mut VisualTestContext,
    query: TargetQuery,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    redraw(cx);
    cx.update(|window, cx| {
        cx.global::<QaTargetRegistry>()
            .find(window.window_handle().window_id(), &query)
    })
}

fn is_shown(cx: &mut VisualTestContext, text: &str) -> bool {
    find(cx, TargetQuery::ExactText(text.into())).is_some()
}

fn is_shown_containing(cx: &mut VisualTestContext, text: &str) -> bool {
    let query =
        serde_json::from_value(serde_json::json!({ "text": text, "exact": false })).unwrap();
    find(cx, query).is_some()
}

fn click(cx: &mut VisualTestContext, text: &str) {
    let bounds = find(cx, TargetQuery::ExactText(text.into()))
        .unwrap_or_else(|| panic!("{text} must be on screen"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

fn diff_file(path: &str) -> DiffFile {
    DiffFile {
        path: path.into(),
        original_path: None,
        status: DiffFileStatus::Modified,
        additions: 1,
        deletions: 0,
    }
}

fn files_ab() -> Vec<DiffFile> {
    vec![diff_file("A"), diff_file("B")]
}

fn open_page<'a>(
    cx: &'a mut TestAppContext,
    task_id: &str,
    files: Vec<DiffFile>,
    source: Rc<FakeDiffSource>,
) -> (Entity<DiffPage>, &'a mut VisualTestContext) {
    let source: SharedDiffSource = source;
    let task_id = task_id.to_string();
    cx.add_window_view(move |_, cx| DiffPage::new(&task_id, Locale::Ko, files, source, cx))
}

fn task_with_worktree() -> KanbanTask {
    KanbanTask {
        id: "task-1".into(),
        title: "diff".into(),
        description: None,
        status: TaskStatus::Progress,
        branch_name: Some("feature/diff".into()),
        worktree_path: Some("/repo__worktrees/feature-diff".into()),
        session_type: None,
        session_name: None,
        ssh_host: None,
        agent_type: None,
        project_id: None,
        base_branch: Some("main".into()),
        pr_url: None,
        priority: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn open_route(
    cx: &mut TestAppContext,
    source: Rc<FakeDiffSource>,
) -> (Entity<TaskDiffView>, &mut VisualTestContext) {
    let RouteResolution::Render(route) = resolve_route_path("/ko/task/task-1/diff") else {
        panic!("diff route must render");
    };
    let source: SharedDiffSource = source;
    cx.add_window_view(move |_, cx| {
        let screen = ScreenContext {
            route,
            navigator: WindowNavigator::new(WeakEntity::new_invalid()),
        };
        TaskDiffView::with_source(screen, source, cx)
    })
}

#[gpui::test]
fn slow_task_lookup_shows_the_delay_notice_instead_of_task_not_found(cx: &mut TestAppContext) {
    init(cx);
    let source = FakeDiffSource::immediate();
    source.task_replies.borrow_mut().push_back(TaskReply::Hang);
    let (_, cx) = open_route(cx, source);

    cx.executor().advance_clock(Duration::from_millis(5000));

    assert!(is_shown(cx, "조회가 지연되고 있습니다. 계속 기다리는 중…"));
    assert!(!is_shown(cx, "작업을 찾을 수 없습니다."));
}

#[gpui::test]
fn failed_task_lookup_shows_the_reason_and_retries(cx: &mut TestAppContext) {
    init(cx);
    let source = FakeDiffSource::immediate();
    source.task_replies.borrow_mut().extend([
        TaskReply::Ready(Box::new(Err("git timeout".into()))),
        TaskReply::Ready(Box::new(Ok(Some(task_with_worktree())))),
    ]);
    *source.files.borrow_mut() = vec![diff_file("a.ts")];
    let (_, cx) = open_route(cx, Rc::clone(&source));

    assert!(is_shown(cx, "불러오지 못했습니다: git timeout"));
    assert!(!is_shown(cx, "작업을 찾을 수 없습니다."));
    click(cx, "다시 시도");

    assert!(is_shown(cx, "a.ts"));
    assert_eq!(
        source
            .calls()
            .iter()
            .filter(|call| matches!(call, Call::Task(_)))
            .count(),
        2
    );
}

#[gpui::test]
fn missing_task_shows_task_not_found_without_an_alert(cx: &mut TestAppContext) {
    init(cx);
    let source = FakeDiffSource::immediate();
    source
        .task_replies
        .borrow_mut()
        .push_back(TaskReply::Ready(Box::new(Ok(None))));
    let (_, cx) = open_route(cx, source);

    assert!(is_shown(cx, "작업을 찾을 수 없습니다."));
    assert!(
        find(
            cx,
            serde_json::from_value(serde_json::json!({ "role": "alert" })).unwrap()
        )
        .is_none()
    );
}

#[gpui::test]
fn saving_the_default_file_needs_no_tree_click(cx: &mut TestAppContext) {
    init(cx);
    let source = FakeDiffSource::immediate();
    let (_, cx) = open_page(cx, "task", files_ab(), Rc::clone(&source));

    click(cx, "편집 모드");
    click(cx, "저장");

    assert!(
        source
            .calls()
            .contains(&Call::Save("task".into(), "A".into(), "modified".into()))
    );
    assert!(is_shown(cx, "저장 완료"));
}

#[gpui::test]
fn a_late_response_for_the_previous_file_does_not_replace_the_current_file(
    cx: &mut TestAppContext,
) {
    init(cx);
    let source = FakeDiffSource::new(|call| match call {
        Call::Original(_, path) | Call::Modified(_, path) if path == "A" => None,
        Call::Original(..) => Some(Ok("B-original".into())),
        _ => Some(Ok("B-modified".into())),
    });
    let (_, cx) = open_page(cx, "task", files_ab(), Rc::clone(&source));

    click(cx, "B");
    assert!(is_shown(cx, "B-modified"));
    source.resolve(
        |call| matches!(call, Call::Original(_, path) if path == "A"),
        Ok("A-original".into()),
    );
    source.resolve(
        |call| matches!(call, Call::Modified(_, path) if path == "A"),
        Ok("A-modified".into()),
    );
    cx.run_until_parked();

    assert!(is_shown(cx, "B-modified"));
    assert!(!is_shown(cx, "A-modified"));
    let reads_of_b = source
        .calls()
        .iter()
        .filter(
            |call| matches!(call, Call::Original(_, path) | Call::Modified(_, path) if path == "B"),
        )
        .count();
    assert_eq!(reads_of_b, 2);
}

#[gpui::test]
fn a_read_failure_ends_loading_and_offers_no_writable_editor(cx: &mut TestAppContext) {
    init(cx);
    let source = FakeDiffSource::new(|call| match call {
        Call::Modified(..) => Some(Err("read unavailable".into())),
        _ => Some(Ok("original".into())),
    });
    let (_, cx) = open_page(cx, "task", files_ab(), Rc::clone(&source));

    assert!(is_shown(cx, "read unavailable"));
    click(cx, "편집 모드");

    assert!(!is_shown(cx, "저장"));
    assert!(
        !source
            .calls()
            .iter()
            .any(|call| matches!(call, Call::Save(..)))
    );
}

#[gpui::test]
fn responses_for_a_previous_task_are_ignored_even_with_the_same_file_name(cx: &mut TestAppContext) {
    init(cx);
    let source = FakeDiffSource::new(|call| match call {
        Call::Modified(task, _) if task == "first" => None,
        Call::Modified(..) => Some(Ok("second-content".into())),
        _ => Some(Ok("original".into())),
    });
    let _ = open_page(cx, "first", files_ab(), Rc::clone(&source));
    let (_, cx) = open_page(cx, "second", files_ab(), Rc::clone(&source));
    assert!(is_shown(cx, "second-content"));

    source.resolve(
        |call| matches!(call, Call::Modified(task, _) if task == "first"),
        Ok("first-content".into()),
    );
    cx.run_until_parked();

    assert!(is_shown(cx, "second-content"));
    assert!(!is_shown(cx, "first-content"));
}

#[gpui::test]
fn a_new_inventory_with_the_same_path_reloads_the_content(cx: &mut TestAppContext) {
    init(cx);
    let replies = Rc::new(RefCell::new(VecDeque::from(["before", "after"])));
    let modified_replies = Rc::clone(&replies);
    let source = FakeDiffSource::new(move |call| match call {
        Call::Modified(..) => Some(Ok(modified_replies
            .borrow_mut()
            .pop_front()
            .unwrap_or_default()
            .to_string())),
        _ => Some(Ok("original".into())),
    });
    let (page, cx) = open_page(cx, "task", files_ab(), Rc::clone(&source));
    assert!(is_shown(cx, "before"));

    let refreshed: Vec<DiffFile> = files_ab()
        .into_iter()
        .map(|file| DiffFile {
            additions: 10,
            ..file
        })
        .collect();
    page.update(cx, |page, cx| page.set_files(refreshed, cx));
    cx.run_until_parked();

    assert!(is_shown(cx, "after"));
    assert_eq!(
        source
            .calls()
            .iter()
            .filter(|call| matches!(call, Call::Modified(..)))
            .count(),
        2
    );
}

#[gpui::test]
fn a_refresh_with_the_same_content_keeps_the_open_comparison(cx: &mut TestAppContext) {
    init(cx);
    let (page, cx) = open_page(cx, "task", files_ab(), FakeDiffSource::immediate());
    redraw(cx);
    let viewer_before = page.read_with(cx, |page, _| page.current_viewer_id());
    assert!(viewer_before.is_some());

    page.update(cx, |page, cx| page.set_files(files_ab(), cx));
    redraw(cx);

    assert_eq!(
        page.read_with(cx, |page, _| page.current_viewer_id()),
        viewer_before
    );
}

#[gpui::test]
fn the_comparison_view_copies_a_selection_and_scrolls_with_page_keys(cx: &mut TestAppContext) {
    init(cx);
    let modified: String = (0..80)
        .map(|number| format!("line {number}"))
        .collect::<Vec<_>>()
        .join("\n");
    let reply = modified.clone();
    let source = FakeDiffSource::new(move |call| {
        Some(Ok(match call {
            Call::Original(..) => "original".to_string(),
            _ => reply.clone(),
        }))
    });
    let (_page, cx) = open_page(cx, "task", files_ab(), source);
    redraw(cx);
    let second_line = TargetQuery::ExactText("line 1".into());
    let top_before = find(cx, second_line.clone())
        .expect("둘째 줄이 보인다")
        .origin
        .y;

    click(cx, "line 0");
    cx.simulate_keystrokes("ctrl-a ctrl-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some(modified)
    );

    cx.simulate_keystrokes("down");
    let top_after_down = find(cx, second_line.clone())
        .expect("한 줄만 내려가 둘째 줄은 아직 보인다")
        .origin
        .y;
    assert!(top_after_down < top_before);
    cx.simulate_keystrokes("pagedown");
    assert!(
        find(cx, second_line).is_none(),
        "한 화면 아래로 내려가 둘째 줄이 보이지 않는다"
    );
}

/// 편집기 첫 줄을 눌러 포커스를 주고 글자를 넣는다
fn type_into_editor(cx: &mut VisualTestContext, text: &str) {
    let first_line = find(
        cx,
        serde_json::from_value(serde_json::json!({ "role": "code-line" })).unwrap(),
    )
    .expect("code line");
    cx.simulate_click(first_line.center(), Modifiers::none());
    cx.simulate_keystrokes("home");
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn deferred_refresh_source(refresh_reply: Option<Reply>) -> Rc<FakeDiffSource> {
    let modified_count = Rc::new(RefCell::new(0));
    FakeDiffSource::new(move |call| match call {
        Call::Modified(..) => {
            *modified_count.borrow_mut() += 1;
            if *modified_count.borrow() == 1 {
                Some(Ok("before".into()))
            } else {
                refresh_reply.clone()
            }
        }
        _ => Some(Ok("original".into())),
    })
}

#[gpui::test]
fn an_editor_draft_survives_a_refresh_and_diff_mode_shows_fresh_content(cx: &mut TestAppContext) {
    init(cx);
    let source = deferred_refresh_source(None);
    let (page, cx) = open_page(cx, "task", files_ab(), Rc::clone(&source));
    click(cx, "편집 모드");
    type_into_editor(cx, "draft ");

    page.update(cx, |page, cx| page.set_files(files_ab(), cx));
    cx.run_until_parked();
    assert!(is_shown(cx, "draft before"));
    source.resolve(
        |call| matches!(call, Call::Modified(..)),
        Ok("after".into()),
    );
    cx.run_until_parked();

    assert!(is_shown(cx, "draft before"));
    click(cx, "비교 모드");
    assert!(is_shown(cx, "after"));
}

#[gpui::test]
fn a_failed_same_file_refresh_keeps_the_editor_draft_without_an_alert(cx: &mut TestAppContext) {
    init(cx);
    let source = deferred_refresh_source(Some(Err("refresh unavailable".into())));
    let (page, cx) = open_page(cx, "task", files_ab(), Rc::clone(&source));
    click(cx, "편집 모드");
    type_into_editor(cx, "draft ");

    page.update(cx, |page, cx| page.set_files(files_ab(), cx));
    cx.run_until_parked();

    assert!(is_shown(cx, "draft before"));
    assert!(!is_shown(cx, "refresh unavailable"));
}

#[gpui::test]
fn a_remaining_file_is_shown_when_the_selection_disappears(cx: &mut TestAppContext) {
    init(cx);
    let source = FakeDiffSource::new(|call| match call {
        Call::Modified(_, path) => Some(Ok(format!("{path}-content"))),
        _ => Some(Ok("original".into())),
    });
    let (page, cx) = open_page(cx, "task", files_ab(), source);
    click(cx, "B");
    assert!(is_shown(cx, "B-content"));

    page.update(cx, |page, cx| page.set_files(vec![diff_file("A")], cx));
    cx.run_until_parked();
    assert!(is_shown(cx, "A-content"));

    page.update(cx, |page, cx| page.set_files(Vec::new(), cx));
    cx.run_until_parked();
    assert!(!is_shown(cx, "A-content"));
    assert!(is_shown(cx, "변경 사항이 없습니다"));
}

#[gpui::test]
fn a_rename_reads_the_original_from_the_previous_path_and_saves_to_the_destination(
    cx: &mut TestAppContext,
) {
    init(cx);
    let source = FakeDiffSource::new(|call| match call {
        Call::Original(_, path) => Some(Ok(if path == "old.txt" {
            "base content".into()
        } else {
            String::new()
        })),
        _ => Some(Ok("modified".into())),
    });
    let renamed = DiffFile {
        path: "new.txt".into(),
        original_path: Some("old.txt".into()),
        status: DiffFileStatus::Renamed,
        additions: 0,
        deletions: 0,
    };
    let (_, cx) = open_page(cx, "task", vec![renamed], Rc::clone(&source));

    assert!(is_shown(cx, "base content"));
    assert!(
        source
            .calls()
            .contains(&Call::Modified("task".into(), "new.txt".into()))
    );
    click(cx, "편집 모드");
    click(cx, "저장");

    assert!(source.calls().contains(&Call::Save(
        "task".into(),
        "new.txt".into(),
        "modified".into()
    )));
}

type PendingSaves = Rc<RefCell<Vec<oneshot::Sender<Result<(), String>>>>>;

/// 편집기만 띄운다. 저장 요청은 `requests`에 쌓이고 `results`의 다음 값으로 끝난다(값이 없으면 끝나지 않는다)
struct EditorHarness {
    saved_contents: Rc<RefCell<Vec<String>>>,
    pending: PendingSaves,
    results: Rc<RefCell<VecDeque<Result<(), String>>>>,
}

impl EditorHarness {
    fn new() -> Self {
        Self {
            saved_contents: Rc::default(),
            pending: Rc::default(),
            results: Rc::default(),
        }
    }

    fn handler(&self) -> SaveHandler {
        let (saved, pending, results) = (
            Rc::clone(&self.saved_contents),
            Rc::clone(&self.pending),
            Rc::clone(&self.results),
        );
        Rc::new(move |content: String, cx: &mut App| {
            saved.borrow_mut().push(content);
            if let Some(result) = results.borrow_mut().pop_front() {
                return Task::ready(result);
            }
            let (sender, receiver) = oneshot::channel();
            pending.borrow_mut().push(sender);
            cx.background_spawn(async move { receiver.await.unwrap_or(Err("dropped".into())) })
        })
    }

    fn open<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<DiffFileEditor>, &'a mut VisualTestContext) {
        self.open_with(cx, "original")
    }

    fn open_with<'a>(
        &self,
        cx: &'a mut TestAppContext,
        content: &'static str,
    ) -> (Entity<DiffFileEditor>, &'a mut VisualTestContext) {
        let handler = self.handler();
        cx.add_window_view(move |_, cx| {
            DiffFileEditor::new(content, "src/a.ts", Locale::Ko, handler, cx)
        })
    }
}

#[gpui::test]
fn saving_twice_while_a_save_is_running_sends_one_save(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open(cx);

    click(cx, "저장");
    type_into_editor(cx, "");
    cx.simulate_keystrokes("ctrl-s");
    cx.simulate_keystrokes("ctrl-s");
    assert_eq!(harness.saved_contents.borrow().len(), 1);

    let _ = harness.pending.borrow_mut().remove(0).send(Ok(()));
    cx.run_until_parked();
    assert!(is_shown(cx, "저장 완료"));
}

#[gpui::test]
fn a_save_failure_reason_stays_until_a_retry_succeeds(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    harness
        .results
        .borrow_mut()
        .extend([Err("EACCES: permission denied".into()), Ok(())]);
    let (_, cx) = harness.open(cx);

    click(cx, "저장");
    assert!(is_shown(cx, "저장 실패: EACCES: permission denied"));
    cx.executor().advance_clock(Duration::from_secs(10));
    assert!(is_shown(cx, "저장 실패: EACCES: permission denied"));

    click(cx, "저장");
    assert!(!is_shown_containing(cx, "EACCES"));
    assert_eq!(harness.saved_contents.borrow().len(), 2);
}

#[gpui::test]
fn editing_marks_unsaved_changes_until_the_save_finishes(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    harness.results.borrow_mut().push_back(Ok(()));
    let (_, cx) = harness.open(cx);
    assert!(!is_shown(cx, "저장하지 않은 변경"));

    type_into_editor(cx, "edited ");
    assert!(is_shown(cx, "저장하지 않은 변경"));
    click(cx, "저장");

    assert!(!is_shown(cx, "저장하지 않은 변경"));
    assert_eq!(
        harness.saved_contents.borrow().as_slice(),
        ["edited original"]
    );
}

fn tree_files(paths: &[(&str, DiffFileStatus)]) -> Vec<DiffFile> {
    paths
        .iter()
        .map(|(path, status)| DiffFile {
            status: *status,
            ..diff_file(path)
        })
        .collect()
}

#[gpui::test]
fn the_tree_shows_folders_and_file_names(cx: &mut TestAppContext) {
    init(cx);
    let files = tree_files(&[
        ("src/index.ts", DiffFileStatus::Modified),
        ("src/utils/helper.ts", DiffFileStatus::Added),
    ]);
    let (_, cx) = open_page(cx, "task", files, FakeDiffSource::immediate());

    for name in ["src", "utils", "index.ts", "helper.ts"] {
        assert!(is_shown(cx, name), "{name}");
    }
}

#[gpui::test]
fn clicking_a_file_selects_it(cx: &mut TestAppContext) {
    init(cx);
    let files = tree_files(&[
        ("README.md", DiffFileStatus::Added),
        ("src/app.ts", DiffFileStatus::Modified),
    ]);
    let (page, cx) = open_page(cx, "task", files, FakeDiffSource::immediate());

    click(cx, "README.md");

    assert_eq!(
        page.read_with(cx, |page, _| page.effective_selected_file())
            .as_deref(),
        Some("README.md")
    );
    assert!(is_shown(cx, "README.md"));
}

fn row_state(is_selected: bool, is_viewed: bool) -> FileRowState {
    FileRowState {
        is_selected,
        is_viewed,
    }
}

#[gpui::test]
fn the_selected_file_uses_the_brand_highlight(cx: &mut TestAppContext) {
    init(cx);
    let theme = cx.update(|cx| *cx.global::<ThemeTokens>());

    assert_eq!(
        file_row_text_color(&theme, &row_state(true, false)),
        theme.text.brand
    );
    assert_eq!(
        file_row_text_color(&theme, &row_state(false, false)),
        theme.text.secondary
    );
}

#[gpui::test]
fn viewed_files_show_a_check_icon_in_muted_text(cx: &mut TestAppContext) {
    init(cx);
    let theme = cx.update(|cx| *cx.global::<ThemeTokens>());

    assert_eq!(file_row_icon(&row_state(false, true)), DiffIcon::Viewed);
    assert_eq!(file_row_icon(&row_state(false, false)), DiffIcon::File);
    assert_eq!(
        file_row_text_color(&theme, &row_state(false, true)),
        theme.text.muted
    );
}

#[test]
fn viewed_file_names_are_struck_through() {
    assert!(file_name_is_struck_through(&row_state(false, true)));
    assert!(!file_name_is_struck_through(&row_state(false, false)));
}

#[gpui::test]
fn clicking_a_folder_collapses_it(cx: &mut TestAppContext) {
    init(cx);
    let files = tree_files(&[("src/deep/file.ts", DiffFileStatus::Modified)]);
    let (_, cx) = open_page(cx, "task", files, FakeDiffSource::immediate());
    assert!(is_shown(cx, "file.ts"));

    click(cx, "src");

    assert!(!is_shown(cx, "file.ts"));
    assert!(!is_shown(cx, "deep"));
}

#[gpui::test]
fn marking_a_file_viewed_updates_the_counter_and_label(cx: &mut TestAppContext) {
    init(cx);
    let (_, cx) = open_page(cx, "task", files_ab(), FakeDiffSource::immediate());
    assert!(is_shown(cx, "0 / 2"));

    click(cx, "확인");

    assert!(is_shown(cx, "1 / 2"));
    assert!(is_shown(cx, "확인 완료"));
}

#[gpui::test]
fn keyboard_moves_through_the_tree_and_opens_files(cx: &mut TestAppContext) {
    init(cx);
    let (page, cx) = open_page(cx, "task", files_ab(), FakeDiffSource::immediate());
    click(cx, "A");

    cx.simulate_keystrokes("down enter");

    assert_eq!(
        page.read_with(cx, |page, _| page.effective_selected_file())
            .as_deref(),
        Some("B")
    );
}

#[gpui::test]
fn dragging_the_resize_handle_clamps_the_sidebar_width(cx: &mut TestAppContext) {
    init(cx);
    let (_, cx) = open_page(cx, "task", files_ab(), FakeDiffSource::immediate());
    let handle_x = super::page::DEFAULT_SIDEBAR_WIDTH + 2.0;
    let start = gpui_kit::point(gpui_kit::px(handle_x), gpui_kit::px(300.0));

    cx.simulate_mouse_down(start, gpui_kit::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        gpui_kit::point(gpui_kit::px(handle_x + 1000.0), gpui_kit::px(300.0)),
        Some(gpui_kit::MouseButton::Left),
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        gpui_kit::point(gpui_kit::px(handle_x + 1000.0), gpui_kit::px(300.0)),
        gpui_kit::MouseButton::Left,
        Modifiers::none(),
    );

    // 줄 틀은 사이드바 폭에서 테두리 1px과 좌우 여백 6px씩을 뺀 폭이다
    let row = find(cx, TargetQuery::ExactText("A".into())).expect("tree row");
    assert_eq!(
        f32::from(row.size.width),
        super::page::MAX_SIDEBAR_WIDTH - 13.0
    );
}

#[gpui::test]
fn a_failed_refresh_keeps_the_loaded_screen_and_shows_a_banner(cx: &mut TestAppContext) {
    init(cx);
    let source = FakeDiffSource::immediate();
    source.task_replies.borrow_mut().extend([
        TaskReply::Ready(Box::new(Ok(Some(task_with_worktree())))),
        TaskReply::Ready(Box::new(Err("offline".into()))),
    ]);
    *source.files.borrow_mut() = vec![diff_file("a.ts")];
    let (view, cx) = open_route(cx, source);
    assert!(is_shown(cx, "a.ts"));

    view.update(cx, |view, cx| view.refresh(cx));
    cx.run_until_parked();

    assert!(is_shown(
        cx,
        "최신 데이터를 불러오지 못해 이전 데이터를 표시합니다: offline"
    ));
    assert!(is_shown(cx, "a.ts"));
}

#[gpui::test]
fn undo_shortcut_reverts_typing_in_the_editor(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open(cx);
    type_into_editor(cx, "edited ");
    assert!(is_shown(cx, "edited original"));

    cx.simulate_keystrokes("ctrl-z");

    assert!(is_shown(cx, "original"));
    assert!(!is_shown(cx, "edited original"));
}

/// 편집기 줄을 눌러 그 줄에 커서를 둔다
fn click_code_line(cx: &mut VisualTestContext, text: &str, click_count: usize) {
    let bounds = find(cx, TargetQuery::ExactText(text.into()))
        .unwrap_or_else(|| panic!("{text} must be on screen"));
    for count in 1..=click_count {
        cx.simulate_event(gpui_kit::MouseDownEvent {
            position: bounds.center(),
            modifiers: Modifiers::none(),
            button: gpui_kit::MouseButton::Left,
            click_count: count,
            first_mouse: false,
        });
        cx.simulate_event(gpui_kit::MouseUpEvent {
            position: bounds.center(),
            modifiers: Modifiers::none(),
            button: gpui_kit::MouseButton::Left,
            click_count: count,
        });
    }
    cx.run_until_parked();
}

fn find_status(cx: &mut VisualTestContext) -> Option<String> {
    let query = serde_json::from_value(serde_json::json!({ "role": "status" })).unwrap();
    find(cx, query)?;
    let mut labels = vec!["No results".to_string()];
    for total in 1..=5 {
        labels.extend((1..=total).map(|current| format!("{current} of {total}")));
    }
    labels.into_iter().find(|label| is_shown(cx, label))
}

const FIND_SAMPLE: &str = "card one\ncards two\nCARD three";

#[gpui::test]
fn find_counts_matches_moves_with_enter_and_closes_with_escape(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, FIND_SAMPLE);
    click_code_line(cx, "card one", 1);

    cx.simulate_keystrokes("ctrl-home ctrl-f");
    cx.simulate_input("card");
    assert_eq!(find_status(cx).as_deref(), Some("1 of 3"));
    cx.simulate_keystrokes("enter");
    assert_eq!(find_status(cx).as_deref(), Some("2 of 3"));
    cx.simulate_keystrokes("shift-enter shift-f3");
    assert_eq!(find_status(cx).as_deref(), Some("3 of 3"));

    cx.simulate_keystrokes("escape");
    assert!(find_status(cx).is_none());
    cx.simulate_input("X");
    assert!(
        is_shown_containing(cx, "X"),
        "focus returns to the editor after Escape"
    );
}

#[gpui::test]
fn find_toggles_match_case_whole_word_and_regex(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, FIND_SAMPLE);
    click_code_line(cx, "card one", 1);
    cx.simulate_keystrokes("ctrl-home ctrl-f");
    cx.simulate_input("card");

    assert_eq!(find_status(cx).as_deref(), Some("1 of 3"));
    cx.simulate_keystrokes("alt-c");
    assert_eq!(find_status(cx).as_deref(), Some("1 of 2"));
    cx.simulate_keystrokes("alt-w");
    assert_eq!(find_status(cx).as_deref(), Some("1 of 1"));
    cx.simulate_keystrokes("alt-w alt-r");
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("c[a-z]+s");
    assert_eq!(find_status(cx).as_deref(), Some("1 of 1"));
    cx.simulate_input("(");
    assert_eq!(find_status(cx).as_deref(), Some("No results"));
}

#[gpui::test]
fn replace_all_rewrites_every_match_in_one_undo_step(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "a = card;\nb = card;");
    click_code_line(cx, "a = card;", 1);

    cx.simulate_keystrokes("ctrl-home ctrl-h");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("card");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("deck");
    cx.simulate_keystrokes("ctrl-alt-enter");
    assert!(is_shown(cx, "a = deck;") && is_shown(cx, "b = deck;"));

    cx.simulate_keystrokes("escape ctrl-z");
    assert!(is_shown(cx, "a = card;") && is_shown(cx, "b = card;"));
}

#[gpui::test]
fn the_diff_viewer_finds_in_the_clicked_side_without_replace(cx: &mut TestAppContext) {
    init(cx);
    let source = FakeDiffSource::new(|call| match call {
        Call::Original(..) => Some(Ok("old line".into())),
        _ => Some(Ok("new line\nnew tail".into())),
    });
    let (_, cx) = open_page(cx, "task", files_ab(), source);
    click_code_line(cx, "new line", 1);

    cx.simulate_keystrokes("ctrl-f");
    cx.simulate_input("new");

    assert_eq!(find_status(cx).as_deref(), Some("1 of 2"));
    assert!(
        find(
            cx,
            serde_json::from_value(serde_json::json!({ "role": "textbox", "name": "Replace" }))
                .unwrap()
        )
        .is_none()
    );
}

const FOLD_SAMPLE: &str = "function a() {\n  first();\n  second();\n}\nend";

#[gpui::test]
fn folding_shortcuts_hide_and_show_a_block(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, FOLD_SAMPLE);
    click_code_line(cx, "  first();", 1);

    cx.simulate_keystrokes("ctrl-shift-[");
    assert!(!is_shown(cx, "  first();") && is_shown(cx, "}"));
    cx.simulate_keystrokes("ctrl-shift-]");
    assert!(is_shown(cx, "  first();") && is_shown(cx, "  second();"));
}

#[gpui::test]
fn the_folding_control_next_to_the_line_number_toggles_the_block(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, FOLD_SAMPLE);
    let header = find(cx, TargetQuery::ExactText("function a() {".into())).expect("header line");
    let control = gpui_kit::point(header.origin.x - gpui_kit::px(8.0), header.center().y);

    cx.simulate_click(control, Modifiers::none());
    assert!(!is_shown(cx, "  second();"));
    cx.simulate_click(control, Modifiers::none());
    assert!(is_shown(cx, "  second();"));
}

#[gpui::test]
fn brackets_auto_close_type_over_and_delete_as_a_pair(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "");
    click_code_line(cx, "", 1);

    cx.simulate_input("f(");
    assert!(is_shown(cx, "f()"));
    cx.simulate_input("x)");
    assert!(is_shown(cx, "f(x)"));
    cx.simulate_input(" [");
    cx.simulate_keystrokes("backspace");
    assert!(is_shown(cx, "f(x) "));
}

#[gpui::test]
fn typing_a_bracket_over_a_selection_surrounds_it(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "value");
    click_code_line(cx, "value", 2);

    cx.simulate_input("\"");

    assert!(is_shown(cx, "\"value\""));
}

#[gpui::test]
fn word_shortcuts_move_select_and_delete_by_word(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "return cards.length;");
    click_code_line(cx, "return cards.length;", 1);

    cx.simulate_keystrokes("end ctrl-left ctrl-left ctrl-backspace");
    assert!(is_shown(cx, "return cardslength;"));
    cx.simulate_keystrokes("ctrl-shift-right");
    cx.simulate_input("size");
    assert!(is_shown(cx, "return cardssize;"));
    cx.simulate_keystrokes("ctrl-delete");
    assert!(is_shown(cx, "return cardssize"));
}

#[gpui::test]
fn double_click_selects_a_word_and_triple_click_a_line(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "alpha beta\nsecond");
    let first = find(cx, TargetQuery::ExactText("alpha beta".into())).expect("first line");
    let on_alpha = gpui_kit::point(first.origin.x + gpui_kit::px(10.0), first.center().y);
    cx.simulate_event(gpui_kit::MouseDownEvent {
        position: on_alpha,
        modifiers: Modifiers::none(),
        button: gpui_kit::MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui_kit::MouseUpEvent {
        position: on_alpha,
        modifiers: Modifiers::none(),
        button: gpui_kit::MouseButton::Left,
        click_count: 2,
    });

    cx.simulate_input("A");
    assert!(is_shown(cx, "A beta"));

    click_code_line(cx, "second", 3);
    cx.simulate_input("Z");
    assert!(is_shown(cx, "Z"));
    assert!(!is_shown(cx, "second"));
}

#[gpui::test]
fn regex_replace_all_substitutes_capture_groups(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "countOpen();\ncountDone();");
    click_code_line(cx, "countOpen();", 1);

    cx.simulate_keystrokes("ctrl-home ctrl-h tab alt-r");
    cx.simulate_input(r"count(\w+)");
    assert_eq!(find_status(cx).as_deref(), Some("1 of 2"));
    cx.simulate_keystrokes("tab");
    cx.simulate_input("$1Count$$");
    cx.simulate_keystrokes("ctrl-alt-enter");

    assert!(is_shown(cx, "OpenCount$();") && is_shown(cx, "DoneCount$();"));
}

#[gpui::test]
fn preserve_case_keeps_the_shape_of_each_replaced_word(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "Card card CARD");
    click_code_line(cx, "Card card CARD", 1);

    cx.simulate_keystrokes("ctrl-home ctrl-h");
    cx.simulate_input("deck");
    cx.simulate_keystrokes("alt-p ctrl-alt-enter");

    assert!(is_shown(cx, "Deck deck DECK"));
}

#[gpui::test]
fn regex_with_a_newline_escape_finds_across_lines(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "a;\nb;\na;");
    click_code_line(cx, "b;", 1);

    cx.simulate_keystrokes("ctrl-f alt-r");
    cx.simulate_input(r";\nb");

    assert_eq!(find_status(cx).as_deref(), Some("1 of 1"));
}

#[gpui::test]
fn typing_a_doc_comment_opener_closes_it_and_enter_continues_it(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "");
    click_code_line(cx, "", 1);

    cx.simulate_input("/**");
    assert!(is_shown(cx, "/** */"));
    cx.simulate_keystrokes("enter");
    cx.simulate_input("Sum");

    assert!(is_shown(cx, "/**") && is_shown(cx, " * Sum") && is_shown(cx, " */"));
}

#[gpui::test]
fn region_markers_fold_the_whole_region(cx: &mut TestAppContext) {
    init(cx);
    let harness = EditorHarness::new();
    let (_, cx) = harness.open_with(cx, "// #region helpers\nconst x = 1;\n// #endregion\nend");
    let header = find(cx, TargetQuery::ExactText("// #region helpers".into())).expect("region");
    let control = gpui_kit::point(header.origin.x - gpui_kit::px(8.0), header.center().y);

    cx.simulate_click(control, Modifiers::none());

    assert!(!is_shown(cx, "const x = 1;") && !is_shown(cx, "// #endregion"));
    assert!(is_shown(cx, "end"));
}
