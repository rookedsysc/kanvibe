//! pane 채팅 칸의 글자 우클릭 메뉴(TS `requestTextContextMenu`·`TerminalPaneChatLayer`의 우클릭과 Electron main 메뉴).
//! 우클릭은 실제 마우스 사건으로 보내고, 메뉴 항목은 QA 대상(`menuitem`)으로 찾아 누른다. 결과는 클립보드와 입력창 글로 본다.

mod task_detail_pane_chat_support;

use gpui_kit::base::TextSelectionLayer;
use gpui_kit::{
    AppContext, Bounds, ClipboardItem, Context, Entity, IntoElement, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render, Styled,
    TestAppContext, VisualTestContext, Window, div, gpui, point, px,
};
use kanvibe_app::qa::targets::{DescribedTarget, QaTargetRegistry, TargetQuery};
use kanvibe_app::views::task_detail::pane_chat::preferences::PaneChatPreferences;
use kanvibe_app::views::task_detail::pane_chat::{PaneChatView, PaneChatViewProps};
use task_detail_pane_chat_support::*;

const SNAPSHOT: &str = "paneChat.getPaneChatSnapshot";
/// 첫 메시지 `Exact pane response`에서 끌어 고른 뒤쪽
const SELECTED_MESSAGE: &str = "pane response";

/// 앱 창 뿌리(`AppWindow`)처럼 창 글자 선택 층을 함께 그린다
struct SelectableHost {
    chat: Option<Entity<PaneChatView>>,
}

impl Render for SelectableHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(TextSelectionLayer)
            .children(self.chat.clone())
    }
}

fn open_chat(cx: &mut TestAppContext) -> &mut VisualTestContext {
    init(cx);
    cx.update(|cx| cx.set_global(QaTargetRegistry::default()));
    let backend = FakeBackend::new();
    backend.respond(SNAPSHOT, snapshot("token-1"));
    let client = backend.client();
    std::mem::forget(backend);
    let (host, cx) = cx.add_window_view(|_, _| SelectableHost { chat: None });
    cx.update(|window, _| window.activate_window());
    host.update_in(cx, |host, window, cx| {
        let chat = cx.new(|cx| {
            let preferences = cx.new(|cx| PaneChatPreferences::new(client.clone(), cx));
            let props = PaneChatViewProps {
                client,
                task_id: "task-1".into(),
                pane_id: Some("%3".into()),
                task_title: Some("Task".into()),
                translator: translator(),
                preferences,
                initial_snapshot: None,
            };
            PaneChatView::new(props, window, cx)
        });
        host.chat = Some(chat);
    });
    redraw(cx);
    cx
}

/// QA 대상 표를 새로 채우며 그린다
fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        let window_id = window.window_handle().window_id();
        cx.global_mut::<QaTargetRegistry>().begin_frame(window_id);
        window.refresh();
    });
    cx.run_until_parked();
}

fn find(cx: &mut VisualTestContext, role: &str, name: Option<&str>) -> Option<Bounds<Pixels>> {
    let query = TargetQuery::Described(DescribedTarget {
        role: Some(role.into()),
        name: name.map(str::to_string),
        exact: Some(true),
        ..DescribedTarget::default()
    });
    cx.update(|window, cx| {
        cx.global::<QaTargetRegistry>()
            .find(window.window_handle().window_id(), &query)
    })
}

fn composer(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    find(cx, "textbox", Some(&t("placeholder"))).expect("composer textbox")
}

fn has_composer_text(cx: &mut VisualTestContext, text: &str) -> bool {
    let query = TargetQuery::Described(DescribedTarget {
        role: Some("textbox".into()),
        text: Some(text.into()),
        exact: Some(true),
        ..DescribedTarget::default()
    });
    cx.update(|window, cx| {
        cx.global::<QaTargetRegistry>()
            .find(window.window_handle().window_id(), &query)
            .is_some()
    })
}

/// 대화 영역의 한 점(입력창 위)
fn transcript_point(cx: &mut VisualTestContext) -> Point<Pixels> {
    let composer = composer(cx);
    point(composer.center().x, composer.origin.y - px(80.0))
}

fn right_click(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(MouseDownEvent {
        button: MouseButton::Right,
        position,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        button: MouseButton::Right,
        position,
        modifiers: Modifiers::default(),
        click_count: 1,
    });
    redraw(cx);
}

fn click_item(cx: &mut VisualTestContext, label: &str) {
    let bounds = find(cx, "menuitem", Some(label)).unwrap_or_else(|| panic!("{label} 항목"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    redraw(cx);
}

fn menu_items(cx: &mut VisualTestContext) -> Vec<&'static str> {
    ["Cut", "Copy", "Paste", "Select All"]
        .into_iter()
        .filter(|label| find(cx, "menuitem", Some(label)).is_some())
        .collect()
}

/// 대화 첫 메시지(머리줄 아래 첫 줄)의 앞에서 입력창 바로 위까지 끌어 대화 글자를 고른다
fn drag_select_transcript(cx: &mut VisualTestContext) {
    let composer = composer(cx);
    let start = point(composer.origin.x + px(5.0), px(96.0));
    let end = point(
        composer.origin.x + composer.size.width,
        composer.origin.y - px(4.0),
    );
    cx.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: start,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseMoveEvent {
        position: end,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::default(),
    });
    cx.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position: end,
        modifiers: Modifiers::default(),
        click_count: 1,
    });
    redraw(cx);
}

fn clipboard_text(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

fn write_clipboard(cx: &mut VisualTestContext, text: &str) {
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.to_string())));
}

/// 입력창을 누르고 글을 넣는다(커서는 글 끝)
fn type_into_composer(cx: &mut VisualTestContext, text: &str) {
    let composer = composer(cx);
    cx.simulate_click(composer.center(), Modifiers::default());
    cx.simulate_input(text);
    redraw(cx);
}

#[gpui::test]
fn right_clicking_selected_chat_text_offers_copy_in_this_window(cx: &mut TestAppContext) {
    let cx = open_chat(cx);
    drag_select_transcript(cx);
    let position = transcript_point(cx);

    right_click(cx, position);

    // 메뉴는 우클릭한 이 창의 그 자리에 뜬다
    let copy = find(cx, "menuitem", Some("Copy")).expect("copy item");
    assert!((copy.origin.y - position.y).abs() < px(40.0));
    click_item(cx, "Copy");
    let copied = clipboard_text(cx).unwrap_or_default();
    assert!(copied.contains(SELECTED_MESSAGE), "{copied}");
    assert!(menu_items(cx).is_empty(), "고르면 메뉴를 닫는다");
}

#[gpui::test]
fn without_selected_text_the_menu_has_nothing_to_copy(cx: &mut TestAppContext) {
    let cx = open_chat(cx);
    write_clipboard(cx, "before");
    let position = transcript_point(cx);

    right_click(cx, position);
    assert_eq!(menu_items(cx), ["Copy"]);
    click_item(cx, "Copy");

    assert_eq!(clipboard_text(cx).as_deref(), Some("before"));
}

#[gpui::test]
fn a_selected_range_in_the_composer_can_be_cut_and_pasted(cx: &mut TestAppContext) {
    let cx = open_chat(cx);
    type_into_composer(cx, "입력한 글자");
    cx.simulate_keystrokes("shift-left shift-left");
    redraw(cx);
    let position = composer(cx).center();

    right_click(cx, position);
    assert_eq!(menu_items(cx), ["Cut", "Copy", "Paste", "Select All"]);
    click_item(cx, "Cut");

    assert_eq!(clipboard_text(cx).as_deref(), Some("글자"));
    assert!(has_composer_text(cx, "입력한 "));
}

#[gpui::test]
fn a_caret_in_the_composer_can_paste_but_has_nothing_to_copy(cx: &mut TestAppContext) {
    let cx = open_chat(cx);
    type_into_composer(cx, "초안");
    write_clipboard(cx, " 붙일 글자");
    let position = composer(cx).center();

    right_click(cx, position);
    click_item(cx, "Copy");
    assert_eq!(clipboard_text(cx).as_deref(), Some(" 붙일 글자"));
    right_click(cx, position);
    click_item(cx, "Paste");

    assert!(has_composer_text(cx, "초안 붙일 글자"));
}

#[gpui::test]
fn a_non_text_control_is_not_treated_as_a_text_field(cx: &mut TestAppContext) {
    let cx = open_chat(cx);
    let button = find(cx, "button", None).expect("a chat button");

    right_click(cx, button.center());

    assert_eq!(menu_items(cx), ["Copy"]);
}

#[gpui::test]
fn escape_closes_the_text_menu(cx: &mut TestAppContext) {
    let cx = open_chat(cx);
    let position = transcript_point(cx);
    right_click(cx, position);
    assert!(!menu_items(cx).is_empty());

    cx.simulate_keystrokes("escape");
    redraw(cx);

    assert!(menu_items(cx).is_empty());
}
