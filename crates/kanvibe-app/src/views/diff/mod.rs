//! `/:locale/task/:id/diff` 변경 비교 화면(TS `DiffRoute` + `DiffPageClient`).
//! 작업을 읽은 뒤 브랜치와 worktree가 있으면 변경 파일 목록을 읽어 헤더와 `page::DiffPage`를 그린다.
//! 조회가 5초를 넘으면 지연 안내를, 실패하면 사유와 재시도를 보이고, 성공했는데 작업이 없을 때만 "작업 없음" 화면을 보인다.

mod file_editor;
mod file_tree;
mod icons;
mod line_diff;
mod monaco;
mod page;
mod source;
mod syntax;
mod text_baseline;
mod text_buffer;
mod viewer;

#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Task, Window, div, px,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::task::KanbanTask;
use kanvibe_domain::task_diff_stats::DiffFile;

use crate::components::{box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::routes::AppRoute;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::{ScreenContext, TailwindText};
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};
use icons::{DiffIcon, diff_icon};
use page::DiffPage;
use source::{ServiceDiffSource, SharedDiffSource};
use text_baseline::{TextFamily, aligned_text, chromium_text};

/// TS `INITIAL_DESKTOP_LOAD_TIMEOUT_MS`
const LOAD_DELAY_NOTICE_AFTER: Duration = Duration::from_millis(5000);

/// 작업 조회 상태(`useRouteLoadStatus`의 로딩·실패·성공)
enum RouteLoadState {
    Loading {
        is_delayed: bool,
    },
    Failed(String),
    Loaded {
        task: Option<Box<KanbanTask>>,
        page: Option<Entity<DiffPage>>,
        file_count: usize,
    },
}

/// 작업과 변경 파일 목록 조회 결과
type RouteLoadResult = Result<(Option<KanbanTask>, Vec<DiffFile>), String>;

pub struct TaskDiffView {
    screen: ScreenContext,
    task_id: String,
    source: SharedDiffSource,
    load_state: RouteLoadState,
    /// 보이던 화면을 다시 읽다가 실패한 사유(TS `RouteRefreshFailureBanner`). 데이터는 지우지 않는다
    refresh_failure: Option<String>,
    has_set_window_title: bool,
    _load: Option<Task<()>>,
    _delay_notice: Option<Task<()>>,
}

impl TaskDiffView {
    pub fn new(screen: ScreenContext, cx: &mut Context<Self>) -> Self {
        let source: SharedDiffSource = Rc::new(ServiceDiffSource::from_app(cx));
        Self::with_source(screen, source, cx)
    }

    fn with_source(
        screen: ScreenContext,
        source: SharedDiffSource,
        cx: &mut Context<Self>,
    ) -> Self {
        let task_id = match &screen.route.route {
            AppRoute::TaskDiff { task_id } => task_id.clone(),
            _ => String::new(),
        };
        let mut view = Self {
            screen,
            task_id,
            source,
            load_state: RouteLoadState::Loading { is_delayed: false },
            refresh_failure: None,
            has_set_window_title: false,
            _load: None,
            _delay_notice: None,
        };
        view.begin_load(cx);
        view
    }

    fn locale(&self) -> Locale {
        self.screen.route.locale
    }

    /// 앱이 새로 고침을 알릴 때(TS `useRefreshSignal(["all", "diff"])`) 작업과 변경 목록을 다시 읽는다.
    /// 보이던 화면은 그대로 두고, 같은 파일을 보고 있으면 그 내용도 다시 읽는다
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_failure = None;
        self.begin_load(cx);
        cx.notify();
    }

    /// 작업과 변경 파일 목록을 읽는다. 브랜치나 worktree가 없는 작업은 목록을 읽지 않는다
    fn begin_load(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.load_state, RouteLoadState::Loaded { .. }) {
            self.load_state = RouteLoadState::Loading { is_delayed: false };
            self._delay_notice = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(LOAD_DELAY_NOTICE_AFTER)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let RouteLoadState::Loading { is_delayed } = &mut this.load_state {
                        *is_delayed = true;
                        cx.notify();
                    }
                });
            }));
        }
        let (source, task_id) = (Rc::clone(&self.source), self.task_id.clone());
        self._load = Some(cx.spawn(async move |this, cx| {
            let task = cx.update(|cx| source.load_task(&task_id, cx)).await;
            let result = match task {
                Ok(Some(task)) if has_worktree(&task) => {
                    let files = cx.update(|cx| source.load_files(&task_id, cx)).await;
                    files.map(|files| (Some(task), files))
                }
                Ok(task) => Ok((task, Vec::new())),
                Err(reason) => Err(reason),
            };
            let _ = this.update(cx, |this, cx| this.finish_load(result, cx));
        }));
    }

    fn finish_load(&mut self, result: RouteLoadResult, cx: &mut Context<Self>) {
        self._delay_notice = None;
        let existing_page = match &self.load_state {
            RouteLoadState::Loaded { page, .. } => Some(page.clone()),
            _ => None,
        };
        match (result, existing_page) {
            (Err(reason), Some(_)) => self.refresh_failure = Some(reason),
            (Err(reason), None) => self.load_state = RouteLoadState::Failed(reason),
            (Ok((task, files)), existing_page) => {
                let file_count = files.len();
                let page = self.page_for(task.as_ref(), files, existing_page.flatten(), cx);
                self.load_state = RouteLoadState::Loaded {
                    task: task.map(Box::new),
                    page,
                    file_count,
                };
            }
        }
        cx.notify();
    }

    /// 새로 고침이면 열린 페이지에 새 목록을 넘겨 선택·확인 표시·편집 중인 내용을 지킨다
    fn page_for(
        &self,
        task: Option<&KanbanTask>,
        files: Vec<DiffFile>,
        existing_page: Option<Entity<DiffPage>>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<DiffPage>> {
        if !task.is_some_and(has_worktree) {
            return None;
        }
        if let Some(page) = existing_page {
            page.update(cx, |page, cx| page.set_files(files, cx));
            return Some(page);
        }
        let (task_id, locale, source) =
            (self.task_id.clone(), self.locale(), Rc::clone(&self.source));
        Some(cx.new(|cx| DiffPage::new(&task_id, locale, files, source, cx)))
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.refresh_failure = None;
        self.begin_load(cx);
        cx.notify();
    }

    /// "작업으로 돌아가기". 수식키를 누른 클릭은 새 창으로 연다(TS `shouldHandleTaskNavigationClick`)
    fn open_task_detail(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.modifiers();
        let request = TaskNavigationRequest {
            origin_window: Some(window.window_handle().window_id()),
            should_open_new_window: modifiers.platform || modifiers.control || modifiers.shift,
        };
        navigate_to_task_detail(&self.task_id, self.locale(), request, cx);
    }
}

fn has_worktree(task: &KanbanTask) -> bool {
    task.branch_name
        .as_deref()
        .is_some_and(|branch| !branch.is_empty())
        && task
            .worktree_path
            .as_deref()
            .is_some_and(|path| !path.is_empty())
}

fn qa_target(role: &'static str, text: impl Into<SharedString>) -> impl IntoElement {
    let text = text.into();
    qa_target_layer(QaTargetDescription {
        role,
        name: text.clone(),
        text,
    })
}

/// 화면 전체를 채우고 가운데에 놓는 틀(`min-h-screen flex items-center justify-center bg-bg-page`)
fn centered_page(theme: &ThemeTokens) -> gpui_kit::Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .bg(theme.background.page)
        .px(spacing(4.0))
}

impl TaskDiffView {
    fn render_loading(&self, theme: &ThemeTokens, is_delayed: bool) -> impl IntoElement {
        let common = Translator::new(self.locale(), "common");
        let delay_notice = is_delayed.then(|| {
            let notice = common.translate("loadDelayed");
            div()
                .relative()
                .text_scale(text_scale::XS)
                .child(chromium_text(notice.clone()))
                .child(qa_target("status", notice))
        });
        centered_page(theme)
            .gap(spacing(3.0))
            .text_scale(text_scale::BODY)
            .text_color(theme.text.muted)
            .child(chromium_text("Loading..."))
            .children(delay_notice)
    }

    /// TS `InlineLoadError`: 실패 사유와 재시도 버튼
    fn render_failure(
        &self,
        theme: &ThemeTokens,
        reason: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let common = Translator::new(self.locale(), "common");
        let message = common.translate_with("loadFailed", &[("reason", reason)]);
        let alert = self
            .render_load_error(theme, message, cx)
            .w_full()
            .max_w(px(448.0));
        centered_page(theme).gap(spacing(3.0)).child(alert)
    }

    /// 보이던 화면을 다시 읽다 실패했을 때 위쪽 가운데에 뜨는 배너(`fixed top-3 w-[min(32rem,calc(100%-2rem))] shadow-md`)
    fn render_refresh_failure(
        &self,
        theme: &ThemeTokens,
        reason: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let common = Translator::new(self.locale(), "common");
        let message = common.translate_with("refreshFailed", &[("reason", reason)]);
        let banner = self
            .render_load_error(theme, message, cx)
            .w_full()
            .max_w(px(512.0))
            .shadow(box_shadows(&theme.shadow.md));
        div()
            .absolute()
            .top(spacing(3.0))
            .left_0()
            .right_0()
            .px(spacing(4.0))
            .flex()
            .justify_center()
            .child(banner)
            .into_any_element()
    }

    /// TS `InlineLoadError`: 실패 사유와 다시 시도 버튼
    fn render_load_error(
        &self,
        theme: &ThemeTokens,
        message: String,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let retry_label = Translator::new(self.locale(), "common").translate("retry");
        let retry = div()
            .id("diff-route-retry")
            .relative()
            .flex_none()
            .rounded(radius::BASE)
            .px(spacing(1.5))
            .py(spacing(0.5))
            .css_weight(500)
            .hover(|style| style.underline())
            .on_click(cx.listener(|this, _, _, cx| this.retry(cx)))
            .child(chromium_text(retry_label.clone()))
            .child(qa_target("button", retry_label));
        div()
            .relative()
            .flex()
            .items_start()
            .justify_between()
            .gap(spacing(3.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(fade(theme.status.error, 0.2))
            .bg(theme.background.page)
            .px(spacing(3.0))
            .py(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.status.error)
            .child(div().min_w_0().child(aligned_text(
                message.clone(),
                text_scale::XS,
                TextFamily::Sans,
            )))
            .child(retry)
            .child(qa_target("alert", message))
    }

    fn render_task_not_found(&self, theme: &ThemeTokens) -> impl IntoElement {
        let labels = Translator::new(self.locale(), "diffView");
        let go_back = labels.translate("goBack");
        let navigator = self.screen.navigator.clone();
        let hover_border = theme.brand.primary;
        let hover_text = theme.text.primary;
        let button = div()
            .id("diff-route-go-back")
            .relative()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .px(spacing(4.0))
            .py(spacing(2.0))
            .text_scale(text_scale::SM)
            .text_color(theme.text.secondary)
            .hover(move |style| style.border_color(hover_border).text_color(hover_text))
            .on_click(move |_, _, cx| navigator.go_back(cx))
            .child(chromium_text(go_back.clone()))
            .child(qa_target("button", go_back));
        let message = labels.translate("taskNotFound");
        centered_page(theme).child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(spacing(4.0))
                .text_center()
                .child(
                    div()
                        .relative()
                        .text_scale(text_scale::BODY)
                        .text_color(theme.text.muted)
                        .child(chromium_text(message.clone()))
                        .child(qa_target("text", message)),
                )
                .child(button),
        )
    }

    /// 브랜치나 worktree가 없는 작업: 안내와 작업 상세로 돌아가는 링크
    fn render_missing_worktree(
        &self,
        theme: &ThemeTokens,
        message_key: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let labels = Translator::new(self.locale(), "diffView");
        let back_label = labels.translate("backToTask");
        let link = div()
            .id("diff-route-back-link")
            .relative()
            .text_scale(text_scale::SM)
            .text_color(theme.text.brand)
            .hover(|style| style.underline())
            .on_click(cx.listener(Self::open_task_detail))
            .child(chromium_text(back_label.clone()))
            .child(qa_target("link", back_label));
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(spacing(4.0))
            .bg(theme.background.page)
            .child(
                div()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.muted)
                    .child(chromium_text(labels.translate(message_key))),
            )
            .child(link)
    }

    fn render_header(
        &self,
        theme: &ThemeTokens,
        task: &KanbanTask,
        file_count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let labels = Translator::new(self.locale(), "diffView");
        let back_link = self.render_back_link(theme, &labels, cx);
        let divider = || {
            div()
                .h(spacing(4.0))
                .w(px(1.0))
                .flex_none()
                .bg(theme.border.default)
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing(3.0))
            .px(spacing(4.0))
            .py(spacing(2.5))
            .bg(theme.background.surface)
            .border_b_1()
            .border_color(theme.border.default)
            .child(back_link)
            .child(divider())
            .child(
                div()
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(chromium_text(labels.translate("title"))),
            )
            .child(divider())
            .child(render_branch_badges(theme, task))
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .items_center()
                    .gap(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(chromium_text(format!("{file_count} files"))),
            )
    }
}

/// `main > feat/…` 배지 두 개(`text-xs font-mono px-2 py-0.5 rounded-md`)
fn render_branch_badges(theme: &ThemeTokens, task: &KanbanTask) -> impl IntoElement {
    let badge = |label: String, background, text_color| {
        div()
            .text_scale(text_scale::XS)
            .mono(400)
            .bg(background)
            .text_color(text_color)
            .px(spacing(2.0))
            .py(spacing(0.5))
            .rounded(radius::MD)
            .child(chromium_text(label))
    };
    let base_branch = task
        .base_branch
        .clone()
        .unwrap_or_else(|| "main".to_string());
    div()
        .flex()
        .items_center()
        .gap(spacing(1.5))
        .child(badge(
            base_branch,
            theme.tag.base_branch.background,
            theme.text.muted,
        ))
        .child(diff_icon(DiffIcon::BranchArrow, theme.text.muted))
        .child(badge(
            task.branch_name.clone().unwrap_or_default(),
            theme.tag.branch.background,
            theme.tag.branch.text,
        ))
}

impl Render for TaskDiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let body: AnyElement = match &self.load_state {
            RouteLoadState::Loading { is_delayed } => {
                self.render_loading(&theme, *is_delayed).into_any_element()
            }
            RouteLoadState::Failed(reason) => self
                .render_failure(&theme, &reason.clone(), cx)
                .into_any_element(),
            RouteLoadState::Loaded { task: None, .. } => {
                self.render_task_not_found(&theme).into_any_element()
            }
            RouteLoadState::Loaded {
                task: Some(task),
                page,
                file_count,
            } => {
                let (task, page, file_count) = (task.clone(), page.clone(), *file_count);
                if !self.has_set_window_title {
                    window.set_window_title(&format!(
                        "Diff - {}",
                        task.branch_name.clone().unwrap_or_default()
                    ));
                    self.has_set_window_title = true;
                }
                self.render_loaded(&theme, &task, page, file_count, cx)
            }
        };
        body
    }
}

impl TaskDiffView {
    /// 헤더 "작업으로 돌아가기"(`text-sm text-text-secondary hover:text-text-primary`)
    fn render_back_link(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let back_label = labels.translate("backToTask");
        let hover_text = theme.text.primary;
        div()
            .id("diff-header-back")
            .relative()
            .flex()
            .items_center()
            .gap(spacing(1.5))
            .text_scale(text_scale::SM)
            .text_color(theme.text.secondary)
            .hover(move |style| style.text_color(hover_text))
            .on_click(cx.listener(Self::open_task_detail))
            .child(diff_icon(DiffIcon::Back, theme.text.secondary))
            .child(chromium_text(back_label.clone()))
            .child(qa_target("link", back_label))
    }

    fn render_loaded(
        &self,
        theme: &ThemeTokens,
        task: &KanbanTask,
        page: Option<Entity<DiffPage>>,
        file_count: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(page) = page else {
            let message_key = if task.branch_name.as_deref().is_none_or(str::is_empty) {
                "noBranch"
            } else {
                "noWorktree"
            };
            return self
                .render_missing_worktree(theme, message_key, cx)
                .into_any_element();
        };
        let header = self
            .render_header(theme, task, file_count, cx)
            .into_any_element();
        let refresh_failure = self
            .refresh_failure
            .clone()
            .map(|reason| self.render_refresh_failure(theme, &reason, cx));
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background.page)
            .child(header)
            .child(page)
            .children(refresh_failure)
            .into_any_element()
    }
}
