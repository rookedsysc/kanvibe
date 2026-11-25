//! 백그라운드 sync 검토(TS `BackgroundSyncReviewDialog` + `useBackgroundSyncReview`). 검토 알림을 열면 merge된 PR task를
//! 골라 백그라운드에서 지우고, 새로 등록된 worktree·pull 결과·실패를 함께 보인다.
//! 고른 task 삭제는 대화상자를 닫아도 계속되고, 알림 센터의 "정리 중" 버튼으로 다시 연다.
//! TS는 렌더러 모듈 하나가 상태를 들었다. 여기서는 앱 전역 엔티티가 그 자리이고, 열림 여부만 창마다 따로 둔다.

use std::collections::HashSet;
use std::sync::Arc;

use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, Global, Hsla, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Rgba, ScrollHandle, StatefulInteractiveElement, Styled, Window,
    WindowId, anchored, canvas, deferred, div, point, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::notifications::{
    AppNotification, AppNotificationAction, BackgroundSyncFailurePayload, BackgroundSyncOperation,
    BackgroundSyncPulledTaskPayload, BackgroundSyncRegisteredWorktreePayload,
    BackgroundSyncReviewPayload, PulledTaskStatus, TaskPrMergedDetectedPayload,
};

use super::LocaleShell;
use crate::components::scroll_area::scroll_area;
use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{qa_button_layer, qa_text_layer};
use crate::theme::{ThemeMode, ThemeTokens, radius, spacing, text_scale};
use crate::views::board::icons::{BoardIcon, board_icon_sized};
use crate::views::board::services::{BoardServices, board_services};

/// `z-[520]`: 커맨드 팔레트(500)보다 위, 릴리스 업데이트(540)보다 아래
const SYNC_REVIEW_LAYER_PRIORITY: usize = 520;
/// `max-w-2xl`
const DIALOG_MAX_WIDTH: f32 = 672.0;
/// Tailwind 기본 팔레트 `green-50`·`red-50`. 앱 토큰이 아니어서 두 테마 모두 같다
const GREEN_50: u32 = 0xf0fdf4;
const RED_50: u32 = 0xfef2f2;

/// merge된 PR task 하나의 삭제 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteResult {
    Pending,
    Succeeded,
    Failed(String),
}

/// TS `ReviewSnapshot`. 열림 여부는 창마다 따로 둔다
pub struct BackgroundSyncReviewStore {
    notification: Option<AppNotification>,
    open_windows: HashSet<WindowId>,
    is_cleaning: bool,
    selected_task_ids: Vec<String>,
    /// 삭제를 시작한 순서대로의 결과(TS `Object.values(results)` 순서)
    results: Vec<(String, DeleteResult)>,
    services: Option<Arc<BoardServices>>,
}

#[derive(Clone)]
struct SyncReviewGlobal(Entity<BackgroundSyncReviewStore>);

impl Global for SyncReviewGlobal {}

/// 전역 검토 상태. 처음 부를 때 만든다
pub fn sync_review_store(cx: &mut App) -> Entity<BackgroundSyncReviewStore> {
    if let Some(global) = cx.try_global::<SyncReviewGlobal>() {
        return global.0.clone();
    }
    let store = cx.new(|cx| BackgroundSyncReviewStore {
        notification: None,
        open_windows: HashSet::new(),
        is_cleaning: false,
        selected_task_ids: Vec::new(),
        results: Vec::new(),
        services: board_services(cx),
    });
    cx.set_global(SyncReviewGlobal(store.clone()));
    store
}

fn review_payload(notification: &AppNotification) -> Option<&BackgroundSyncReviewPayload> {
    match notification.action.as_ref()?.as_ref()? {
        AppNotificationAction::BackgroundSyncReview { payload } => Some(payload),
    }
}

impl BackgroundSyncReviewStore {
    pub fn is_open_in(&self, window_id: WindowId) -> bool {
        self.open_windows.contains(&window_id)
    }

    pub fn is_cleaning(&self) -> bool {
        self.is_cleaning
    }

    /// 지우는 중인 알림(정리 중 버튼이 다시 열 대상)
    pub fn notification(&self) -> Option<&AppNotification> {
        self.notification.as_ref()
    }

    fn result_for(&self, task_id: &str) -> Option<&DeleteResult> {
        self.results
            .iter()
            .find(|(id, _)| id == task_id)
            .map(|(_, result)| result)
    }

    fn set_result(&mut self, task_id: &str, result: DeleteResult) {
        match self.results.iter_mut().find(|(id, _)| id == task_id) {
            Some(entry) => entry.1 = result,
            None => self.results.push((task_id.to_string(), result)),
        }
    }

    /// TS `openBackgroundSyncReview`. 검토 알림이 아니면 무시한다. 다른 알림이면 고른 task를 새로 채운다
    pub fn open(
        &mut self,
        notification: AppNotification,
        window_id: WindowId,
        cx: &mut Context<Self>,
    ) {
        let Some(payload) = review_payload(&notification) else {
            return;
        };
        let is_other_notification = self
            .notification
            .as_ref()
            .is_none_or(|current| current.id != notification.id);
        if is_other_notification && !self.is_cleaning {
            let mut selected = Vec::new();
            for merged in &payload.merged_pull_requests {
                if !selected.contains(&merged.task_id) {
                    selected.push(merged.task_id.clone());
                }
            }
            self.selected_task_ids = selected;
            self.results.clear();
            self.notification = Some(notification);
        }
        self.open_windows.insert(window_id);
        cx.notify();
    }

    pub fn close(&mut self, window_id: WindowId, cx: &mut Context<Self>) {
        if self.open_windows.remove(&window_id) {
            cx.notify();
        }
    }

    /// 삭제를 시작한 뒤에는 고른 task를 바꾸지 않는다
    fn toggle(&mut self, task_id: &str, cx: &mut Context<Self>) {
        if self.is_cleaning || !self.results.is_empty() {
            return;
        }
        match self.selected_task_ids.iter().position(|id| id == task_id) {
            Some(index) => {
                self.selected_task_ids.remove(index);
            }
            None => self.selected_task_ids.push(task_id.to_string()),
        }
        cx.notify();
    }

    /// TS `getBackgroundSyncReviewCount`: 검토할 항목 수에서 이미 지운 task를 뺀다
    pub fn review_count(&self, notification: &AppNotification) -> usize {
        let Some(review) = review_payload(notification) else {
            return 0;
        };
        let is_current = self
            .notification
            .as_ref()
            .is_some_and(|current| current.id == notification.id);
        let deleted = if is_current {
            review
                .merged_pull_requests
                .iter()
                .filter(|task| self.result_for(&task.task_id) == Some(&DeleteResult::Succeeded))
                .count()
        } else {
            0
        };
        review.merged_pull_requests.len()
            + review.registered_worktrees.len()
            + review.pulled_tasks.as_ref().map_or(0, Vec::len)
            + review.failures.as_ref().map_or(0, Vec::len)
            - deleted
    }

    /// TS `deleteBackgroundSyncReviewTasks`. 고른 task(또는 실패한 task)를 차례로 지운다. 대화상자는 바로 닫힌다
    fn delete_tasks(&mut self, should_retry_failed: bool, cx: &mut Context<Self>) {
        if self.is_cleaning || self.notification.is_none() {
            return;
        }
        let task_ids: Vec<String> = if should_retry_failed {
            self.results
                .iter()
                .filter(|(_, result)| matches!(result, DeleteResult::Failed(_)))
                .map(|(task_id, _)| task_id.clone())
                .collect()
        } else {
            self.selected_task_ids
                .iter()
                .filter(|task_id| self.result_for(task_id) != Some(&DeleteResult::Succeeded))
                .cloned()
                .collect()
        };
        self.open_windows.clear();
        let Some(services) = self.services.clone().filter(|_| !task_ids.is_empty()) else {
            cx.notify();
            return;
        };
        for task_id in &task_ids {
            self.set_result(task_id, DeleteResult::Pending);
        }
        self.is_cleaning = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            for task_id in task_ids {
                let Ok(delete) = this.update(cx, |_, cx| services.delete_task(task_id.clone(), cx))
                else {
                    return;
                };
                let result = match delete.await {
                    Ok(_) => DeleteResult::Succeeded,
                    Err(failure) => DeleteResult::Failed(failure.reason),
                };
                let _ = this.update(cx, |this, cx| {
                    this.set_result(&task_id, result);
                    cx.notify();
                });
            }
            let _ = this.update(cx, |this, cx| {
                this.is_cleaning = false;
                cx.notify();
            });
        })
        .detach();
    }
}

impl LocaleShell {
    fn close_sync_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let window_id = self.window_handle.window_id();
        self.sync_review
            .update(cx, |review, cx| review.close(window_id, cx));
        self.restore_focus(window, cx);
    }

    fn confirm_sync_review(
        &mut self,
        should_retry_failed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_review.update(cx, |review, cx| {
            review.delete_tasks(should_retry_failed, cx)
        });
        self.restore_focus(window, cx);
    }

    pub(super) fn render_sync_review(
        &mut self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let review = self.sync_review.read(cx);
        if !review.is_open_in(self.window_handle.window_id()) {
            return None;
        }
        let notification = review.notification.clone()?;
        let payload = review_payload(&notification)?.clone();
        if !self.review_focus.contains_focused(window, cx) && !self.review_focus.is_focused(window)
        {
            self.remember_focus(window, cx);
            self.review_focus.focus(window, cx);
        }
        let viewport = self.overlay_viewport(window, cx);
        let labels = ReviewLabels::new(self.locale);
        let sections = self.render_review_sections(theme, &labels, &payload, cx);
        let list_max_height = f32::from(viewport.height) * 0.5;
        let dialog = div()
            .id("sync-review-dialog")
            .track_focus(&self.review_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close_sync_review(window, cx);
                }
                cx.stop_propagation();
            }))
            .occlude()
            .relative()
            .w_full()
            .max_w(px(DIALOG_MAX_WIDTH))
            .max_h(viewport.height - px(32.0))
            .overflow_hidden()
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.lg))
            .p(spacing(6.0))
            .child(qa_text_layer("dialog", &labels.review.translate("title")))
            .child(
                div()
                    .mb(spacing(2.0))
                    .text_scale(text_scale::LG)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(labels.review.translate("title")),
            )
            .child(
                div()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.secondary)
                    .child(labels.review.translate("message"))
                    .child(
                        div()
                            .relative()
                            .mt(spacing(2.0))
                            .child(labels.review.translate("backgroundProcessing"))
                            .child(qa_text_layer(
                                "paragraph",
                                &labels.review.translate("backgroundProcessing"),
                            )),
                    ),
            )
            .child(div().mt(spacing(4.0)).child(capped_scroll_area(
                &self.review_scroll,
                list_max_height,
                theme.border.strong.into(),
                sections,
            )))
            .children(self.render_review_progress(theme, &labels, cx))
            .child(self.render_review_actions(theme, &labels, cx));
        let close = cx.listener(|this, _, window, cx| this.close_sync_review(window, cx));
        let layer = div()
            .occlude()
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background.overlay)
            .p(spacing(4.0))
            .child(
                div()
                    .id("sync-review-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .on_click(close),
            )
            .child(dialog);
        Some(
            deferred(anchored().position(point(px(0.0), px(0.0))).child(layer))
                .with_priority(SYNC_REVIEW_LAYER_PRIORITY)
                .into_any_element(),
        )
    }

    fn render_review_sections(
        &self,
        theme: &ThemeTokens,
        labels: &ReviewLabels,
        payload: &BackgroundSyncReviewPayload,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let merged_rows: Vec<AnyElement> = payload
            .merged_pull_requests
            .iter()
            .map(|merged| self.render_merged_row(theme, labels, merged, cx))
            .collect();
        let merged = review_section(
            theme,
            labels.review.translate("mergedSection"),
            merged_rows,
            labels.review.translate("emptyMerged"),
        );
        let registered = review_section(
            theme,
            labels.review.translate("registeredSection"),
            payload
                .registered_worktrees
                .iter()
                .map(|worktree| registered_row(theme, labels, worktree))
                .collect(),
            labels.review.translate("emptyRegistered"),
        );
        let pulled = payload
            .pulled_tasks
            .as_ref()
            .filter(|pulled| !pulled.is_empty())
            .map(|pulled| {
                review_section(
                    theme,
                    labels.review.translate("pulledSection"),
                    pulled
                        .iter()
                        .map(|task| pulled_row(theme, labels, task))
                        .collect(),
                    String::new(),
                )
                .mt(spacing(5.0))
            });
        let failures = payload
            .failures
            .as_ref()
            .filter(|failures| !failures.is_empty())
            .map(|failures| {
                review_section(
                    theme,
                    labels.review.translate("failureSection"),
                    failures
                        .iter()
                        .map(|failure| failure_row(theme, labels, failure))
                        .collect(),
                    String::new(),
                )
                .mt(spacing(5.0))
            });
        div()
            .pr(spacing(1.0))
            .child(merged)
            .child(registered.mt(spacing(5.0)))
            .children(pulled)
            .children(failures)
            .into_any_element()
    }

    fn render_merged_row(
        &self,
        theme: &ThemeTokens,
        labels: &ReviewLabels,
        merged: &TaskPrMergedDetectedPayload,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let review = self.sync_review.read(cx);
        let is_checked = review.selected_task_ids.contains(&merged.task_id);
        let result = review.result_for(&merged.task_id).cloned();
        let task_id = merged.task_id.clone();
        let pr_link = div().mt(spacing(3.0)).flex().child(
            div()
                .flex()
                .min_w_0()
                .max_w_full()
                .items_center()
                .gap(spacing(1.0))
                .rounded(radius::BASE)
                .bg(theme.tag.pull_request.background)
                .px(spacing(2.0))
                .py(spacing(1.0))
                .text_scale(text_scale::XS)
                .text_color(theme.tag.pull_request.text)
                .child(
                    div()
                        .flex_none()
                        .child(labels.pr_merge.translate("prLinkLabel")),
                )
                .child(div().min_w_0().truncate().child(merged.pr_url.clone())),
        );
        let failure_reason = match &result {
            Some(DeleteResult::Failed(reason)) => Some(
                div()
                    .relative()
                    .mt(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.status.error)
                    .child(reason.clone())
                    .child(qa_text_layer("paragraph", reason)),
            ),
            _ => None,
        };
        let status_pill = result.map(|result| delete_status_pill(theme, labels, &result));
        div()
            .id(gpui_kit::SharedString::from(format!(
                "sync-review-merged-{}",
                merged.task_id
            )))
            .relative()
            .flex()
            .items_start()
            .gap(spacing(3.0))
            .cursor_pointer()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .px(spacing(4.0))
            .py(spacing(3.0))
            .on_click(cx.listener(move |this, _, _, cx| {
                let task_id = task_id.clone();
                this.sync_review
                    .update(cx, |review, cx| review.toggle(&task_id, cx));
            }))
            .child(native_checkbox(theme, is_checked).mt(px(2.0)))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .text_scale(text_scale::SM)
                            .css_weight(600)
                            .text_color(theme.text.primary)
                            .child(merged.task_title.clone()),
                    )
                    .child(
                        div()
                            .mt(spacing(1.0))
                            .text_scale(text_scale::XS)
                            .text_color(theme.text.muted)
                            .child(merged.branch_name.clone()),
                    )
                    .child(pr_link)
                    .children(failure_reason),
            )
            .children(status_pill)
            .child(qa_text_layer("checkbox", &merged.task_title))
            .into_any_element()
    }

    fn render_review_progress(
        &self,
        theme: &ThemeTokens,
        labels: &ReviewLabels,
        cx: &App,
    ) -> Option<impl IntoElement + use<>> {
        let review = self.sync_review.read(cx);
        if review.results.is_empty() {
            return None;
        }
        let total = review.results.len();
        let processed = review
            .results
            .iter()
            .filter(|(_, result)| *result != DeleteResult::Pending)
            .count();
        let failed = review
            .results
            .iter()
            .filter(|(_, result)| matches!(result, DeleteResult::Failed(_)))
            .count();
        let has_failures = !review.is_cleaning && failed > 0;
        let text = if review.is_cleaning {
            labels.review.translate_with(
                "deleteProgress",
                &[
                    ("processed", &processed.to_string()),
                    ("total", &total.to_string()),
                ],
            )
        } else {
            labels.review.translate_with(
                "deleteSummary",
                &[
                    ("total", &total.to_string()),
                    ("succeeded", &(total - failed).to_string()),
                    ("failed", &failed.to_string()),
                ],
            )
        };
        let color = if has_failures {
            theme.status.error
        } else {
            theme.text.secondary
        };
        Some(
            div()
                .relative()
                .mt(spacing(4.0))
                .text_scale(text_scale::SM)
                .text_color(color)
                .child(text.clone())
                .child(qa_text_layer(
                    if has_failures { "alert" } else { "status" },
                    &text,
                )),
        )
    }

    fn render_review_actions(
        &self,
        theme: &ThemeTokens,
        labels: &ReviewLabels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let review = self.sync_review.read(cx);
        let has_started = !review.results.is_empty();
        let has_failures = !review.is_cleaning
            && review
                .results
                .iter()
                .any(|(_, result)| matches!(result, DeleteResult::Failed(_)));
        let is_confirm_disabled = review.is_cleaning || has_started;
        let has_selection = !review.selected_task_ids.is_empty();
        let cancel_label = labels
            .common
            .translate(if has_started { "close" } else { "cancel" });
        let hover_border = theme.brand.primary;
        let cancel = div()
            .id("sync-review-cancel")
            .relative()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .px(spacing(4.0))
            .py(px(6.0))
            .text_scale(text_scale::SM)
            .text_color(theme.text.secondary)
            .cursor_pointer()
            .hover(move |style| style.border_color(hover_border))
            .on_click(cx.listener(|this, _, window, cx| this.close_sync_review(window, cx)))
            .child(cancel_label.clone())
            .child(qa_button_layer(&cancel_label, &cancel_label));
        let (confirm_label, should_retry_failed) = if has_failures {
            (labels.review.translate("retryFailedDeletes"), true)
        } else if has_selection {
            (labels.review.translate("deleteInBackground"), false)
        } else {
            (labels.common.translate("confirm"), false)
        };
        let hover_brand = theme.brand.hover;
        let confirm = div()
            .id("sync-review-confirm")
            .relative()
            .rounded(radius::MD)
            .bg(theme.brand.primary)
            .px(spacing(4.0))
            .py(px(6.0))
            .text_scale(text_scale::SM)
            .text_color(theme.text.inverse)
            .child(confirm_label.clone())
            .child(qa_button_layer(&confirm_label, &confirm_label));
        let confirm = if is_confirm_disabled && !has_failures {
            confirm.opacity(0.5)
        } else {
            confirm
                .cursor_pointer()
                .hover(move |style| style.bg(hover_brand))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.confirm_sync_review(should_retry_failed, window, cx)
                }))
        };
        div()
            .mt(spacing(5.0))
            .flex()
            .flex_wrap()
            .justify_end()
            .gap(spacing(2.0))
            .child(cancel)
            .child(confirm)
    }
}

struct ReviewLabels {
    common: Translator,
    review: Translator,
    pr_merge: Translator,
}

impl ReviewLabels {
    fn new(locale: kanvibe_domain::i18n::Locale) -> Self {
        Self {
            common: Translator::new(locale, "common"),
            review: Translator::new(locale, "common.backgroundSyncReview"),
            pr_merge: Translator::new(locale, "common.prMergeAlert"),
        }
    }
}

/// `max-h-[50dvh] overflow-y-auto`. 내용이 넘칠 때만 Chromium 스크롤바 홈(15px)을 둔다
fn capped_scroll_area(
    handle: &ScrollHandle,
    max_height: f32,
    color: Hsla,
    content: AnyElement,
) -> AnyElement {
    if f32::from(handle.max_offset().y) > 0.0 {
        return div()
            .h(px(max_height))
            .child(scroll_area("sync-review-sections", handle, color, content))
            .into_any_element();
    }
    let measured = handle.clone();
    div()
        .max_h(px(max_height))
        .flex()
        .flex_col()
        .child(
            div()
                .id("sync-review-sections")
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(handle)
                .child(content),
        )
        .child(
            canvas(
                |_, _, _| {},
                move |_, _, window: &mut Window, _: &mut App| {
                    if f32::from(measured.max_offset().y) > 0.0 {
                        window.refresh();
                    }
                },
            )
            .size_0(),
        )
        .into_any_element()
}

/// `section`: 제목과 카드 목록(`mt-3 space-y-3`), 비었으면 안내 글(`mt-2 text-sm text-text-muted`)
fn review_section(
    theme: &ThemeTokens,
    title: String,
    rows: Vec<AnyElement>,
    empty_message: String,
) -> gpui_kit::Div {
    let heading = div()
        .text_scale(text_scale::SM)
        .css_weight(600)
        .text_color(theme.text.primary)
        .child(title);
    if rows.is_empty() {
        return div().child(heading).child(
            div()
                .mt(spacing(2.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(empty_message),
        );
    }
    div().child(heading).child(
        div()
            .mt(spacing(3.0))
            .flex()
            .flex_col()
            .gap(spacing(3.0))
            .children(rows),
    )
}

/// `rounded-full bg-bg-surface px-2 py-0.5 text-[11px] text-text-muted` 같은 작은 알약
fn pill(background: Rgba, text_color: Rgba, label: String) -> gpui_kit::Div {
    div()
        .flex_none()
        .rounded(radius::FULL)
        .bg(background)
        .px(spacing(2.0))
        .py(spacing(0.5))
        .text_scale(text_scale::PX_11)
        .text_color(text_color)
        .child(label)
}

fn location_pill(
    theme: &ThemeTokens,
    labels: &ReviewLabels,
    ssh_host: Option<&str>,
) -> gpui_kit::Div {
    let key = if ssh_host.is_some_and(|host| !host.is_empty()) {
        "remote"
    } else {
        "local"
    };
    pill(
        theme.background.surface,
        theme.text.muted,
        labels.common.translate(key),
    )
}

fn delete_status_pill(
    theme: &ThemeTokens,
    labels: &ReviewLabels,
    result: &DeleteResult,
) -> gpui_kit::Div {
    let label = match result {
        DeleteResult::Pending => labels.common.translate("delete"),
        DeleteResult::Succeeded => labels.review.translate("deleteSucceeded"),
        DeleteResult::Failed(_) => labels.review.translate("deleteFailed"),
    };
    delete_status_pill_shape(theme, labels, result)
        .relative()
        .child(qa_text_layer("status", &label))
}

fn delete_status_pill_shape(
    theme: &ThemeTokens,
    labels: &ReviewLabels,
    result: &DeleteResult,
) -> gpui_kit::Div {
    match result {
        DeleteResult::Pending => pill(
            theme.background.surface,
            theme.text.muted,
            labels.common.translate("delete"),
        ),
        DeleteResult::Succeeded => pill(
            gpui_kit::rgb(GREEN_50),
            theme.status.done,
            labels.review.translate("deleteSucceeded"),
        ),
        DeleteResult::Failed(_) => pill(
            gpui_kit::rgb(RED_50),
            theme.status.error,
            labels.review.translate("deleteFailed"),
        ),
    }
}

/// `rounded-lg border border-border-default bg-bg-page px-4 py-3` 카드
fn review_card(theme: &ThemeTokens) -> gpui_kit::Div {
    div()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.page)
        .px(spacing(4.0))
        .py(spacing(3.0))
}

fn registered_row(
    theme: &ThemeTokens,
    labels: &ReviewLabels,
    worktree: &BackgroundSyncRegisteredWorktreePayload,
) -> AnyElement {
    review_card(theme)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(spacing(3.0))
                .child(
                    div()
                        .text_scale(text_scale::SM)
                        .css_weight(600)
                        .text_color(theme.text.primary)
                        .child(worktree.project_name.clone()),
                )
                .child(location_pill(theme, labels, worktree.ssh_host.as_deref())),
        )
        .child(
            div()
                .mt(spacing(1.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(worktree.branch_name.clone()),
        )
        .child(
            div()
                .mt(spacing(3.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(worktree.worktree_path.clone()),
        )
        .into_any_element()
}

fn pulled_row(
    theme: &ThemeTokens,
    labels: &ReviewLabels,
    task: &BackgroundSyncPulledTaskPayload,
) -> AnyElement {
    let status = match task.status {
        PulledTaskStatus::Updated => pill(
            gpui_kit::rgb(GREEN_50),
            theme.status.done,
            labels.review.translate("pullUpdated"),
        ),
        PulledTaskStatus::Failed => pill(
            gpui_kit::rgb(RED_50),
            theme.status.error,
            labels.review.translate("pullFailed"),
        ),
    };
    review_card(theme)
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(spacing(3.0))
                .child(
                    div()
                        .min_w_0()
                        .child(
                            div()
                                .text_scale(text_scale::SM)
                                .css_weight(600)
                                .text_color(theme.text.primary)
                                .child(task.task_title.clone()),
                        )
                        .child(
                            div()
                                .mt(spacing(1.0))
                                .text_scale(text_scale::XS)
                                .text_color(theme.text.muted)
                                .child(task.branch_name.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(spacing(2.0))
                        .child(location_pill(theme, labels, task.ssh_host.as_deref()))
                        .child(status),
                ),
        )
        .child(
            div()
                .mt(spacing(3.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(task.worktree_path.clone()),
        )
        .child(
            div()
                .mt(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .child(task.summary.clone()),
        )
        .into_any_element()
}

/// TS `getBackgroundSyncFailureLabelKey`
fn failure_label_key(operation: BackgroundSyncOperation) -> &'static str {
    match operation {
        BackgroundSyncOperation::PullRequestSync => "pullRequestFailure",
        BackgroundSyncOperation::TaskPullSync => "taskPullFailure",
        BackgroundSyncOperation::WorktreeSync => "worktreeFailure",
    }
}

fn failure_row(
    theme: &ThemeTokens,
    labels: &ReviewLabels,
    failure: &BackgroundSyncFailurePayload,
) -> AnyElement {
    let is_remote = failure
        .ssh_host
        .as_ref()
        .and_then(Option::as_deref)
        .is_some_and(|host| !host.is_empty());
    let branch = failure
        .branch_name
        .clone()
        .filter(|branch| !branch.is_empty())
        .map(|branch| {
            div()
                .mt(spacing(1.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(branch)
        });
    review_card(theme)
        .border_color(fade(theme.status.error, 0.2))
        .child(
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(spacing(3.0))
                .child(
                    div()
                        .min_w_0()
                        .child(
                            div()
                                .text_scale(text_scale::SM)
                                .css_weight(600)
                                .text_color(theme.text.primary)
                                .child(failure.target.clone()),
                        )
                        .children(branch),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(spacing(2.0))
                        .children(is_remote.then(|| {
                            pill(
                                theme.background.surface,
                                theme.text.muted,
                                labels.common.translate("remote"),
                            )
                        }))
                        .child(pill(
                            gpui_kit::rgb(RED_50),
                            theme.status.error,
                            labels
                                .review
                                .translate(failure_label_key(failure.operation)),
                        )),
                ),
        )
        .child(
            div()
                .mt(spacing(3.0))
                .text_scale(text_scale::XS)
                .text_color(theme.status.error)
                .child(failure.reason.clone()),
        )
        .into_any_element()
}

/// Chromium 기본 체크 상자(`<input type="checkbox">`, 16px). 앱 토큰이 아니라 브라우저 색이다.
/// 고르면 Chromium 기본 강조색(다크 `#99c8ff`, 라이트 `#0075ff`) 위에 체크를 그린다
fn native_checkbox(theme: &ThemeTokens, is_checked: bool) -> gpui_kit::Div {
    let (border, fill, checked_fill, check) = match theme.mode {
        ThemeMode::Dark => (0x858585, 0x3b3b3b, 0x99c8ff, 0x3b3b3b),
        ThemeMode::Light => (0x767676, 0xffffff, 0x0075ff, 0xffffff),
    };
    let checkbox = div()
        .size(spacing(4.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::SM);
    if is_checked {
        checkbox
            .bg(gpui_kit::rgb(checked_fill))
            .child(board_icon_sized(
                BoardIcon::Check,
                gpui_kit::rgb(check),
                10.0,
                8.0,
            ))
    } else {
        checkbox
            .border_1()
            .border_color(gpui_kit::rgb(border))
            .bg(gpui_kit::rgb(fill))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::VisualContext;
    use kanvibe_domain::notifications::BackgroundSyncReviewPayload;

    fn review_notification(id: &str, merged_task_ids: &[&str]) -> AppNotification {
        AppNotification {
            id: id.into(),
            title: "백그라운드 sync 검토".into(),
            body: String::new(),
            task_id: None,
            relative_path: "/ko".into(),
            locale: "ko".into(),
            is_read: false,
            created_at: "2026-01-15T02:55:00.000Z".into(),
            dedupe_key: id.into(),
            action: Some(Some(AppNotificationAction::BackgroundSyncReview {
                payload: BackgroundSyncReviewPayload {
                    merged_pull_requests: merged_task_ids
                        .iter()
                        .map(|task_id| TaskPrMergedDetectedPayload {
                            task_id: (*task_id).into(),
                            task_title: (*task_id).into(),
                            branch_name: "fix/x".into(),
                            pr_url: "https://example.com/pull/1".into(),
                            merged_at: "2026-01-15T02:30:00.000Z".into(),
                        })
                        .collect(),
                    registered_worktrees: Vec::new(),
                    pulled_tasks: None,
                    failures: None,
                },
            })),
        }
    }

    #[gpui_kit::gpui::test]
    fn opening_a_review_selects_every_merged_task_once(cx: &mut gpui_kit::TestAppContext) {
        let window_id = cx.add_empty_window().window_handle().window_id();
        let store = cx.update(sync_review_store);
        store.update(cx, |review, cx| {
            review.open(review_notification("n1", &["a", "b", "a"]), window_id, cx)
        });
        store.read_with(cx, |review, _| {
            assert!(review.is_open_in(window_id));
            assert_eq!(review.selected_task_ids, ["a", "b"]);
            assert_eq!(review.review_count(review.notification().unwrap()), 3);
        });
    }

    #[gpui_kit::gpui::test]
    fn non_review_notifications_do_not_open_the_dialog(cx: &mut gpui_kit::TestAppContext) {
        let window_id = cx.add_empty_window().window_handle().window_id();
        let store = cx.update(sync_review_store);
        let mut plain = review_notification("n2", &[]);
        plain.action = None;
        store.update(cx, |review, cx| review.open(plain, window_id, cx));
        store.read_with(cx, |review, _| assert!(!review.is_open_in(window_id)));
    }
}
