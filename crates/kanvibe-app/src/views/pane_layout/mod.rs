//! `/:locale/pane-layout` pane 레이아웃 화면(TS `PaneLayoutRoute`). 전역 기본 레이아웃 편집기와 프로젝트별 덮어쓰기 목록을 보인다.
//!
//! 읽기 안전 규칙: 전역 설정을 읽지 못하면 빈 편집기를 띄우지 않고(저장하면 실제 설정을 덮으므로) 오류와 재시도만 보인다.
//! 프로젝트 설정 하나를 읽지 못한 것은 "덮어쓰기 없음"과 다르므로 그 프로젝트 안에 오류와 재시도를 보인다.
//! 편집기는 한 번 만들면 다시 읽어도 바꾸지 않아, 고치던 값이 새로 고침에 덮이지 않는다.

pub mod editor;
pub mod store;

use std::collections::HashMap;

use gpui_kit::{
    AnyElement, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, ScrollHandle, Styled, Subscription, Task, Window, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::pane_layout::PaneLayoutConfig;
use kanvibe_domain::project::Project;

use crate::app_events::{RefreshScope, subscribe_to_refresh};
use crate::components::{TailwindText, fade};
use crate::services::ServiceResult;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::ScreenContext;
use crate::views::settings::controls::set_window_title_once;
use crate::views::settings::controls::{
    keyed_id, make_pressable, overflow_scroll_area, pressable, scrollbar_color, text_link,
    visible_text,
};
use crate::views::settings::route_load::INITIAL_LOAD_TIMEOUT;
use editor::PaneLayoutEditor;

/// `max-w-2xl`
const CONTENT_MAX_WIDTH: f32 = 672.0;

/// 한 번 읽은 결과. 프로젝트 설정은 프로젝트마다 성공·실패가 따로다
pub struct PaneLayoutSnapshot {
    pub global_config: Option<PaneLayoutConfig>,
    pub projects: Vec<Project>,
    pub project_configs: HashMap<String, Result<Option<PaneLayoutConfig>, String>>,
}

/// 화면에 붙은 프로젝트 한 줄
struct ProjectLayoutRow {
    project: Project,
    /// 마지막으로 읽은 설정. `None`이면 아직 한 번도 읽지 못했다
    config: Option<Option<PaneLayoutConfig>>,
    editor: Option<Entity<PaneLayoutEditor>>,
    is_open: bool,
}

struct LoadedPaneLayouts {
    global_editor: Entity<PaneLayoutEditor>,
    projects: Vec<ProjectLayoutRow>,
}

pub struct PaneLayoutView {
    screen: ScreenContext,
    /// 창 제목을 걸었는지(TS `document.title`)
    is_title_set: bool,
    loaded: Option<LoadedPaneLayouts>,
    /// 창이 있어야 편집기를 만들 수 있어 다음 render까지 들고 있는 조회 결과
    pending_snapshot: Option<PaneLayoutSnapshot>,
    pub is_load_failed: bool,
    attempt: u64,
    scroll_handle: ScrollHandle,
    _load: Option<Task<()>>,
    _timeout: Option<Task<()>>,
    _refresh_subscription: Option<Subscription>,
}

impl PaneLayoutView {
    pub fn new(screen: ScreenContext, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            screen,
            is_title_set: false,
            loaded: None,
            pending_snapshot: None,
            is_load_failed: false,
            attempt: 0,
            scroll_handle: ScrollHandle::new(),
            _load: None,
            _timeout: None,
            _refresh_subscription: subscribe_to_refresh(
                &[RefreshScope::PaneLayout],
                cx,
                Self::reload,
            ),
        };
        view.reload(cx);
        view
    }

    /// 전역 기본값 편집기. 첫 조회 전에는 없다
    pub fn global_editor(&self) -> Option<Entity<PaneLayoutEditor>> {
        self.loaded
            .as_ref()
            .map(|loaded| loaded.global_editor.clone())
    }

    /// 프로젝트 덮어쓰기 편집기. 그 프로젝트 설정을 한 번도 읽지 못했으면 없다
    pub fn project_editor(&self, project_id: &str) -> Option<Entity<PaneLayoutEditor>> {
        self.project_row(project_id)?.editor.clone()
    }

    /// 프로젝트 배지 상태: `None`은 읽지 못함, `Some(true)`는 덮어쓰기 있음, `Some(false)`는 전역 사용
    pub fn project_override(&self, project_id: &str) -> Option<bool> {
        self.project_row(project_id)?
            .config
            .as_ref()
            .map(|config| config.is_some())
    }

    fn project_row(&self, project_id: &str) -> Option<&ProjectLayoutRow> {
        self.loaded
            .as_ref()?
            .projects
            .iter()
            .find(|row| row.project.id == project_id)
    }

    fn translator(&self, namespace: &'static str) -> Translator {
        Translator::new(self.screen.route.locale, namespace)
    }

    /// 다시 읽는다. 5초 안에 첫 결과가 없으면 오류와 재시도를 보이되, 늦게라도 결과가 오면 그 값으로 채운다
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.attempt += 1;
        let attempt = self.attempt;
        self.is_load_failed = false;
        let timer = cx.background_executor().timer(INITIAL_LOAD_TIMEOUT);
        self._timeout = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.attempt == attempt {
                    this.is_load_failed = true;
                    cx.notify();
                }
            });
        }));
        let request = load_snapshot(cx);
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| this.finish_load(attempt, result, cx));
        }));
        cx.notify();
    }

    fn finish_load(
        &mut self,
        attempt: u64,
        result: ServiceResult<PaneLayoutSnapshot>,
        cx: &mut Context<Self>,
    ) {
        if self.attempt != attempt {
            return;
        }
        self._timeout = None;
        match result {
            Ok(snapshot) => {
                self.is_load_failed = false;
                self.pending_snapshot = Some(snapshot);
            }
            Err(_) => self.is_load_failed = true,
        }
        cx.notify();
    }

    /// 조회 결과를 화면에 붙인다. 이미 있는 편집기는 그대로 두고, 이번에 처음 읽힌 프로젝트만 편집기를 만든다
    fn apply_snapshot(
        &mut self,
        snapshot: PaneLayoutSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let locale = self.screen.route.locale;
        let (previous_global, mut previous_rows) = match self.loaded.take() {
            Some(loaded) => {
                let rows: HashMap<String, ProjectLayoutRow> = loaded
                    .projects
                    .into_iter()
                    .map(|row| (row.project.id.clone(), row))
                    .collect();
                (Some(loaded.global_editor), rows)
            }
            None => (None, HashMap::new()),
        };
        let global_editor = previous_global.unwrap_or_else(|| {
            let config = snapshot.global_config.clone();
            cx.new(|cx| PaneLayoutEditor::new(locale, None, config.as_ref(), window, cx))
        });
        let mut projects = Vec::new();
        for project in snapshot.projects {
            let previous = previous_rows.remove(&project.id);
            let fresh = snapshot
                .project_configs
                .get(&project.id)
                .and_then(|result| result.clone().ok());
            projects.push(merge_project_row(
                locale, project, previous, fresh, window, cx,
            ));
        }
        self.loaded = Some(LoadedPaneLayouts {
            global_editor,
            projects,
        });
    }

    fn toggle_project(&mut self, project_id: &str, cx: &mut Context<Self>) {
        if let Some(row) = self.loaded.as_mut().and_then(|loaded| {
            loaded
                .projects
                .iter_mut()
                .find(|row| row.project.id == project_id)
        }) {
            row.is_open = !row.is_open;
        }
        cx.notify();
    }
}

/// 프로젝트 한 줄을 새 조회와 합친다. 이번에 못 읽었으면 지난 값을 지키고, 편집기는 처음 읽힌 때 한 번만 만든다
fn merge_project_row(
    locale: kanvibe_domain::i18n::Locale,
    project: Project,
    previous: Option<ProjectLayoutRow>,
    fresh: Option<Option<PaneLayoutConfig>>,
    window: &mut Window,
    cx: &mut Context<PaneLayoutView>,
) -> ProjectLayoutRow {
    let (previous_config, editor, is_open) = previous
        .map(|row| (row.config, row.editor, row.is_open))
        .unwrap_or((None, None, false));
    let config = fresh.or(previous_config);
    let editor =
        editor.or_else(|| {
            let config = config.clone()?;
            let project_id = project.id.clone();
            Some(cx.new(|cx| {
                PaneLayoutEditor::new(locale, Some(project_id), config.as_ref(), window, cx)
            }))
        });
    ProjectLayoutRow {
        project,
        config,
        editor,
        is_open,
    }
}

/// 전역 설정과 프로젝트 목록을 함께 읽고, 프로젝트마다 덮어쓰기를 읽는다
fn load_snapshot(cx: &mut Context<PaneLayoutView>) -> Task<ServiceResult<PaneLayoutSnapshot>> {
    let global = store::load_global_pane_layout(cx);
    let projects = store::load_projects(cx);
    cx.spawn(async move |_, cx| {
        let global_config = global.await?;
        let projects = projects.await?;
        let mut requests = Vec::new();
        for project in &projects {
            let project_id = project.id.clone();
            let request = cx.update(|cx| store::load_project_pane_layout(project_id.clone(), cx));
            requests.push((project_id, request));
        }
        let mut project_configs = HashMap::new();
        for (project_id, request) in requests {
            project_configs.insert(project_id, request.await.map_err(|failure| failure.reason));
        }
        Ok(PaneLayoutSnapshot {
            global_config,
            projects,
            project_configs,
        })
    })
}

impl Render for PaneLayoutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        set_window_title_once(&mut self.is_title_set, "Pane Layout", window);
        if let Some(snapshot) = self.pending_snapshot.take() {
            self.apply_snapshot(snapshot, window, cx);
        }
        let theme = *cx.global::<ThemeTokens>();
        if self.loaded.is_none() {
            return self.render_first_load(&theme, cx).into_any_element();
        }
        let page = div()
            .w_full()
            .min_h_full()
            .bg(theme.background.page)
            .p(spacing(6.0))
            .child(
                div()
                    .mx_auto()
                    .w_full()
                    .max_w(px(CONTENT_MAX_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(spacing(8.0))
                    .child(self.render_header(&theme))
                    .child(self.render_global_section(&theme))
                    .child(self.render_projects_section(&theme, cx)),
            );
        div()
            .size_full()
            .child(overflow_scroll_area(
                "pane-layout-scroll",
                &self.scroll_handle,
                scrollbar_color(&theme),
                page,
            ))
            .into_any_element()
    }
}

impl PaneLayoutView {
    /// 첫 조회 전: `로딩 중...`, 실패면 `role=alert` 문구와 "다시 시도"(`text-sm text-brand-primary hover:underline`)
    fn render_first_load(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> impl IntoElement {
        let content: AnyElement = if self.is_load_failed {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(spacing(3.0))
                .child(visible_text(
                    self.translator("paneLayout").translate("loadError"),
                ))
                .child(self.render_retry_button(theme, "pane-layout-retry", cx))
                .into_any_element()
        } else {
            div()
                .child(self.translator("common").translate("loading"))
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background.page)
            .text_color(theme.text.muted)
            .child(content)
    }

    fn render_retry_button(
        &self,
        theme: &ThemeTokens,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = self.translator("paneLayout").translate("retry");
        let button = pressable(id, "button", label.clone(), label.clone())
            .text_scale(text_scale::SM)
            .text_color(theme.brand.primary)
            .hover(|style| style.underline())
            .child(label);
        make_pressable(
            button,
            theme,
            false,
            cx.listener(|this, _, _, cx| this.reload(cx)),
        )
    }

    /// 제목 `text-xl font-bold`, 설명 `text-sm text-text-secondary mt-1`, 오른쪽 보드로 돌아가기 링크
    fn render_header(&self, theme: &ThemeTokens) -> impl IntoElement {
        let pane_layout = self.translator("paneLayout");
        let navigator = self.screen.navigator.clone();
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .child(
                        div()
                            .text_scale(text_scale::XL)
                            .css_weight(700)
                            .text_color(theme.text.primary)
                            .child(pane_layout.translate("title")),
                    )
                    .child(
                        div()
                            .mt(spacing(1.0))
                            .text_scale(text_scale::SM)
                            .text_color(theme.text.secondary)
                            .child(pane_layout.translate("description")),
                    ),
            )
            .child(text_link(
                theme,
                "pane-layout-back",
                pane_layout.translate("backToBoard"),
                move |_, _, cx| navigator.navigate("/", cx),
            ))
    }

    /// `bg-bg-surface rounded-xl border p-5`, 제목 `text-sm font-semibold mb-1`, 설명 `text-xs text-text-muted mb-4`
    fn render_global_section(&self, theme: &ThemeTokens) -> impl IntoElement {
        let pane_layout = self.translator("paneLayout");
        let editor = self
            .loaded
            .as_ref()
            .map(|loaded| loaded.global_editor.clone());
        div()
            .bg(theme.background.surface)
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .p(spacing(5.0))
            .child(
                visible_text(pane_layout.translate("globalDefault"))
                    .mb(spacing(1.0))
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary),
            )
            .child(
                div()
                    .mb(spacing(4.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(pane_layout.translate("globalDescription")),
            )
            .children(editor)
    }

    fn render_projects_section(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pane_layout = self.translator("paneLayout");
        let rows = self
            .loaded
            .as_ref()
            .map(|loaded| loaded.projects.as_slice())
            .unwrap_or_default();
        let body: AnyElement = if rows.is_empty() {
            div()
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(pane_layout.translate("noProjects"))
                .into_any_element()
        } else {
            let mut items = Vec::new();
            for row in rows {
                items.push(self.render_project_row(theme, row, cx));
            }
            div()
                .flex()
                .flex_col()
                .gap(spacing(4.0))
                .children(items)
                .into_any_element()
        };
        div()
            .child(
                div()
                    .mb(spacing(1.0))
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(pane_layout.translate("projectOverride")),
            )
            .child(
                div()
                    .mb(spacing(4.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(pane_layout.translate("projectOverrideDescription")),
            )
            .child(body)
    }

    /// `<details class="bg-bg-surface rounded-xl border">`, 요약 줄 `flex items-center justify-between p-4`
    fn render_project_row(
        &self,
        theme: &ThemeTokens,
        row: &ProjectLayoutRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let summary = self.render_project_summary(theme, row, cx);
        let body = row.is_open.then(|| {
            div()
                .p(spacing(4.0))
                .pt_0()
                .border_t_1()
                .border_color(theme.border.default)
                .child(self.render_project_body(theme, row, cx))
        });
        div()
            .bg(theme.background.surface)
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .child(summary)
            .children(body)
            .into_any_element()
    }

    /// `<summary class="flex items-center justify-between p-4 cursor-pointer">`: 이름·배지와 "설정"
    fn render_project_summary(
        &self,
        theme: &ThemeTokens,
        row: &ProjectLayoutRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let project_id = row.project.id.clone();
        let name = div()
            .text_scale(text_scale::SM)
            .css_weight(500)
            .text_color(theme.text.primary)
            .child(row.project.name.clone());
        let summary = pressable(
            keyed_id("pane-layout-project", &row.project.id),
            "button",
            row.project.name.clone(),
            "",
        )
        .flex()
        .items_center()
        .justify_between()
        .p(spacing(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(spacing(2.0))
                .child(name)
                .child(self.render_project_badge(theme, row)),
        )
        .child(
            visible_text(self.translator("paneLayout").translate("configure"))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted),
        );
        make_pressable(
            summary,
            theme,
            false,
            cx.listener(move |this, _, _, cx| this.toggle_project(&project_id, cx)),
        )
        .into_any_element()
    }

    /// 펼친 프로젝트: 편집기, 또는 설정을 읽지 못했으면 오류와 다시 시도
    fn render_project_body(
        &self,
        theme: &ThemeTokens,
        row: &ProjectLayoutRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(editor) = &row.editor {
            return editor.clone().into_any_element();
        }
        div()
            .py(spacing(3.0))
            .flex()
            .flex_col()
            .gap(spacing(2.0))
            .child(
                div()
                    .text_scale(text_scale::SM)
                    .text_color(theme.status.error)
                    .child(self.translator("paneLayout").translate("loadError")),
            )
            .child(self.render_retry_button(theme, "pane-layout-project-retry", cx))
            .into_any_element()
    }

    /// `text-xs px-2 py-0.5 rounded-full`, 덮어쓰기 있음 `bg-brand-primary/10 text-brand-primary`, 없음 `bg-bg-page text-text-muted`
    fn render_project_badge(
        &self,
        theme: &ThemeTokens,
        row: &ProjectLayoutRow,
    ) -> impl IntoElement {
        let pane_layout = self.translator("paneLayout");
        let (label, background, color) = match &row.config {
            Some(Some(_)) => (
                "hasOverride",
                fade(theme.brand.primary, 0.1),
                theme.brand.primary,
            ),
            Some(None) => ("usingGlobal", theme.background.page, theme.text.muted),
            None => ("loadError", theme.background.page, theme.text.muted),
        };
        div()
            .text_scale(text_scale::XS)
            .px(spacing(2.0))
            .py(spacing(0.5))
            .rounded(radius::FULL)
            .bg(background)
            .text_color(color)
            .child(pane_layout.translate(label))
    }
}
