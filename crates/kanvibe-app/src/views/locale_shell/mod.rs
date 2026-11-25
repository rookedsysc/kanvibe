//! 경로마다 늘 떠 있는 전역 덮개(TS `App.tsx`의 `LocaleShell` 자식들). 창 하나에 하나이고, 화면이 바뀌어도 남는다.
//! 빠른 검색(`TaskQuickSearchDialog`), 커맨드 팔레트(`CommandPaletteDialog`), 릴리스 업데이트(`ReleaseUpdateDialog`),
//! 보드 이벤트 경고(`BoardEventAlert`), 작업 결과 토스트(`ActionStatusToast`), 백그라운드 sync 검토(`BackgroundSyncReviewDialog`)를 그린다.
//!
//! 보드가 떠 있으면 보드가 가진 것(포커스된 카드, 카드 이동, 새 작업 모달, 워크스페이스 선택)을 쓴다.
//! TS `BoardCommandProvider`가 보드 처리기를 등록받던 자리다.

pub mod background_sync;
mod command_palette;
mod event_alert;
mod quick_search;
mod release_notes;
mod release_update;
pub mod sync_review;

use std::time::Duration;

use gpui_kit::{
    AnyElement, AnyWindowHandle, AppContext, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, ParentElement, Pixels, Render, Size, Styled, Subscription, Task, WeakEntity,
    Window, div,
};
use kanvibe_domain::i18n::{Locale, Translator};

use crate::app_events::{AppEvents, BoardEventReceived, NotificationActivated};
use crate::components::toast::{Toast, ToastKind, toast_stack};
use crate::shortcuts::OpenCreateTask;
use crate::theme::ThemeTokens;
use crate::views::WindowNavigator;
use crate::views::board::BoardView;
use crate::views::board::text_field::{BoardTextField, TextFieldEvent};
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};
use background_sync::{BackgroundSyncStore, background_sync_store};
pub use command_palette::{PaletteCommand, PaletteItem};
pub use quick_search::{QuickSearchResult, build_search_results, highlighted_byte_ranges};
use sync_review::{BackgroundSyncReviewStore, sync_review_store};

/// TS `SUCCESS_STATUS_DURATION_MS`
const SUCCESS_TOAST_DURATION: Duration = Duration::from_millis(5_000);

/// 작업 결과 알림 한 건(TS `ActionStatusEntry`). 성공은 5초 뒤 사라지고 실패는 닫을 때까지 남는다
#[derive(Clone)]
struct ActionStatusEntry {
    id: usize,
    kind: ToastKind,
    message: String,
    /// 결과에서 바로 열 작업(“열기” 버튼)
    open_task_id: Option<String>,
}

pub struct LocaleShell {
    locale: Locale,
    window_handle: AnyWindowHandle,
    navigator: WindowNavigator,
    /// 지금 화면이 보드면 그 뷰. TS `registerBoardHandlers`로 등록되던 처리기의 주인이다
    board: Option<WeakEntity<BoardView>>,
    /// 덮개를 열 때 포커스가 있던 곳. 닫으면 돌려준다(TS `requestActiveTerminalFocusAfterUiSettles`)
    return_focus: Option<FocusHandle>,
    palette_field: Entity<BoardTextField>,
    quick_search_field: Entity<BoardTextField>,
    command_palette: Option<command_palette::CommandPaletteState>,
    quick_search: Option<quick_search::QuickSearchState>,
    event_alert: event_alert::BoardEventAlertState,
    release_dialog: release_update::ReleaseDialogState,
    release_focus: FocusHandle,
    review_focus: FocusHandle,
    review_scroll: gpui_kit::ScrollHandle,
    toasts: Vec<ActionStatusEntry>,
    next_toast_id: usize,
    background_sync: Entity<BackgroundSyncStore>,
    sync_review: Entity<BackgroundSyncReviewStore>,
    _release_check_loop: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl LocaleShell {
    pub fn new(
        locale: Locale,
        navigator: WindowNavigator,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let palette_field = create_dialog_field(locale, "commandPalette", cx);
        let quick_search_field = create_dialog_field(locale, "taskSearch", cx);
        let background_sync = background_sync_store(cx);
        let sync_review = sync_review_store(cx);
        let mut shell = Self {
            locale,
            window_handle: window.window_handle(),
            navigator,
            board: None,
            return_focus: None,
            palette_field,
            quick_search_field,
            command_palette: None,
            quick_search: None,
            event_alert: Default::default(),
            release_dialog: Default::default(),
            release_focus: cx.focus_handle(),
            review_focus: cx.focus_handle(),
            review_scroll: gpui_kit::ScrollHandle::new(),
            toasts: Vec::new(),
            next_toast_id: 0,
            background_sync,
            sync_review,
            _release_check_loop: Task::ready(()),
            _subscriptions: Vec::new(),
        };
        shell.subscribe(window, cx);
        shell.start_release_update_checks(window, cx);
        shell
    }

    fn subscribe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let palette_field = self.palette_field.clone();
        let quick_search_field = self.quick_search_field.clone();
        let mut subscriptions = vec![
            cx.subscribe_in(
                &palette_field,
                window,
                |this: &mut Self, _, event: &TextFieldEvent, window, cx| {
                    this.handle_palette_field_event(event, window, cx)
                },
            ),
            cx.subscribe_in(
                &quick_search_field,
                window,
                |this: &mut Self, _, event: &TextFieldEvent, window, cx| {
                    this.handle_quick_search_field_event(event, window, cx)
                },
            ),
            cx.observe(&self.background_sync, |_, _, cx| cx.notify()),
            cx.observe(&self.sync_review, |_, _, cx| cx.notify()),
        ];
        if let Some(bus) = cx.try_global::<AppEvents>().map(|events| events.0.clone()) {
            subscriptions.push(
                cx.subscribe(&bus, |this, _, event: &BoardEventReceived, cx| {
                    this.show_board_event_alert(&event.0, cx)
                }),
            );
            subscriptions.push(
                cx.subscribe(&bus, |this, _, event: &NotificationActivated, cx| {
                    this.receive_notification_activation(event, cx)
                }),
            );
        }
        self._subscriptions = subscriptions;
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    /// 창 루트가 화면을 바꿀 때 부른다. 보드면 보드 처리기를 쓸 수 있다
    pub fn set_board(&mut self, board: Option<WeakEntity<BoardView>>) {
        self.board = board;
    }

    fn board(&self) -> Option<Entity<BoardView>> {
        self.board.as_ref().and_then(WeakEntity::upgrade)
    }

    /// 전역 덮개(팔레트·빠른 검색·릴리스·sync 검토)가 열려 있는지. 열려 있으면 보드 단축키를 쓰지 않는다
    pub fn has_open_overlay(&self, cx: &gpui_kit::App) -> bool {
        self.command_palette.is_some()
            || self.quick_search.is_some()
            || self.release_dialog.release.is_some()
            || self
                .sync_review
                .read(cx)
                .is_open_in(self.window_handle.window_id())
    }

    /// 보드의 대화상자·메뉴가 열려 있는지(TS `registerShortcutBlocker`)
    fn has_board_blocker(&self, cx: &gpui_kit::App) -> bool {
        self.board()
            .is_some_and(|board| board.read(cx).has_board_blocker())
    }

    /// 덮개 층 크기. 칸반은 페이지 스크롤바 홈을 덮지 않는다(Chromium `fixed inset-0`이 스크롤바를 빼는 것과 같다)
    fn overlay_viewport(&self, window: &Window, cx: &gpui_kit::App) -> Size<Pixels> {
        match self.board() {
            Some(board) => board.read(cx).overlay_viewport(window),
            None => window.viewport_size(),
        }
    }

    /// 덮개를 열 때 지금 포커스를 기억한다. 이미 다른 덮개가 기억했으면 그대로 둔다
    fn remember_focus(&mut self, window: &Window, cx: &gpui_kit::App) {
        if self.return_focus.is_none() {
            self.return_focus = window.focused(cx);
        }
    }

    /// 덮개를 닫은 뒤 열기 전 포커스로 돌아간다
    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.return_focus.take() {
            handle.focus(window, cx);
        }
    }

    fn receive_notification_activation(
        &mut self,
        event: &NotificationActivated,
        cx: &mut Context<Self>,
    ) {
        let window_id = self.window_handle.window_id();
        if event.window_id.is_some_and(|target| target != window_id) {
            return;
        }
        let notification = event.notification.clone();
        self.sync_review
            .update(cx, |review, cx| review.open(notification, window_id, cx));
    }

    /// 작업 결과 알림을 띄운다(TS `showActionStatus`)
    pub fn show_action_status(
        &mut self,
        kind: ToastKind,
        message: String,
        open_task_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toasts.push(ActionStatusEntry {
            id,
            kind,
            message,
            open_task_id,
        });
        if kind == ToastKind::Success {
            let timer = cx.background_executor().timer(SUCCESS_TOAST_DURATION);
            cx.spawn(async move |this, cx| {
                timer.await;
                let _ = this.update(cx, |this, cx| this.dismiss_toast(id, cx));
            })
            .detach();
        }
        cx.notify();
    }

    /// 떠 있는 작업 결과 알림 글자(오래된 것부터)
    pub fn toast_messages(&self) -> Vec<String> {
        self.toasts
            .iter()
            .map(|toast| toast.message.clone())
            .collect()
    }

    fn dismiss_toast(&mut self, id: usize, cx: &mut Context<Self>) {
        self.toasts.retain(|toast| toast.id != id);
        cx.notify();
    }

    fn render_toasts(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.toasts.is_empty() {
            return None;
        }
        let labels = Translator::new(self.locale, "common");
        let close_label = labels.translate("close");
        let open_label = labels.translate("open");
        let locale = self.locale;
        let toasts: Vec<Toast> = self
            .toasts
            .iter()
            .map(|toast| {
                let id = toast.id;
                let view = cx.entity().downgrade();
                let mut element = Toast::new(
                    gpui_kit::SharedString::from(format!("action-status-{id}")),
                    toast.kind,
                    toast.message.clone(),
                    close_label.clone(),
                )
                .on_dismiss(move |_, _, cx| {
                    let _ = view.update(cx, |this, cx| this.dismiss_toast(id, cx));
                });
                if let Some(task_id) = toast.open_task_id.clone() {
                    element = element.action(open_label.clone(), move |_, window, cx| {
                        let request = TaskNavigationRequest {
                            origin_window: Some(window.window_handle().window_id()),
                            should_open_new_window: false,
                        };
                        navigate_to_task_detail(&task_id, locale, request, cx);
                    });
                }
                element
            })
            .collect();
        Some(toast_stack(self.overlay_viewport(window, cx), toasts).into_any_element())
    }
}

impl LocaleShell {
    /// 덮개 안에 포커스가 있을 때 받는 앱 단축키. 빠른 검색이 열린 채 새 작업 단축키를 누르면 브랜치 TODO를 만든다
    fn register_shell_actions(&self, root: gpui_kit::Div, cx: &mut Context<Self>) -> gpui_kit::Div {
        root.on_action(cx.listener(|this, _: &OpenCreateTask, window, cx| {
            if this.quick_search.is_some() {
                this.create_branch_todo_from_quick_search(window, cx);
            }
        }))
    }
}

/// 커맨드 팔레트·빠른 검색 입력칸. 안내 문구는 `namespace.placeholder`다
fn create_dialog_field(
    locale: Locale,
    namespace: &'static str,
    cx: &mut Context<LocaleShell>,
) -> Entity<BoardTextField> {
    let theme = *cx.global::<ThemeTokens>();
    let colors = [theme.text.primary, theme.text.muted, theme.text.primary];
    let placeholder = Translator::new(locale, namespace).translate("placeholder");
    cx.new(|cx| BoardTextField::new(placeholder, colors, cx))
}

impl Render for LocaleShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let toasts = self.render_toasts(window, cx);
        let command_palette = self.render_command_palette(&theme, window, cx);
        let quick_search = self.render_quick_search(&theme, window, cx);
        let release_dialog = self.render_release_dialog(&theme, window, cx);
        let event_alert = self.render_board_event_alert(&theme, window, cx);
        let sync_review = self.render_sync_review(&theme, window, cx);
        // 덮개 층은 창 원점에 붙어야 한다(창 루트에서는 화면 뒤에 놓이므로 흐름에서 빼낸다)
        let root = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .children(toasts)
            .children(command_palette)
            .children(quick_search)
            .children(release_dialog)
            .children(sync_review)
            .children(event_alert);
        self.register_shell_actions(root, cx)
    }
}
