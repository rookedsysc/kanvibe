//! 새로 고침(TS `useRefreshSignal(["all", "task-detail"])`)과 재조회 실패 배너(TS `RouteRefreshFailureBanner`).
//! 보드 이벤트가 모인 `all` 신호나 작업 상세 신호가 오면 보이던 화면을 둔 채 다시 읽고, 실패하면 데이터를 지우지 않고 위쪽에 알린다.

use gpui_kit::{AnyElement, ClickEvent, Context, IntoElement, ParentElement, Styled, div, px};

use super::TaskDetailView;
use super::load_states::render_inline_load_error;
use crate::app_events::{RefreshScope, subscribe_to_refresh};
use crate::components::box_shadows;
use crate::theme::{ThemeTokens, spacing};

/// 작업 상세가 받는 새로 고침 범위
const TASK_DETAIL_REFRESH_SCOPES: &[RefreshScope] = &[RefreshScope::All, RefreshScope::TaskDetail];
/// `w-[min(32rem,calc(100%-2rem))]`
const BANNER_MAX_WIDTH: f32 = 512.0;

impl TaskDetailView {
    pub(super) fn subscribe_to_refresh_signal(&mut self, cx: &mut Context<Self>) {
        self._refresh_signal =
            subscribe_to_refresh(TASK_DETAIL_REFRESH_SCOPES, cx, |view, cx| view.refresh(cx));
    }

    /// 보이던 화면을 둔 채 다시 읽는다. 첫 조회가 끝나기 전이면 그 조회를 다시 시작한다
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.begin_load(cx);
        self.reload_visible_hook_statuses(cx);
    }

    /// `fixed left-1/2 top-3 z-[500] -translate-x-1/2 shadow-md`
    pub(super) fn render_refresh_failure_banner(
        &self,
        theme: &ThemeTokens,
        viewport_width: gpui_kit::Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let reason = self.refresh_failure.as_ref()?;
        let message = self
            .common
            .translate_with("refreshFailed", &[("reason", reason)]);
        let width = px(BANNER_MAX_WIDTH).min(viewport_width - spacing(8.0));
        let banner = render_inline_load_error(theme, message, self.common.translate("retry"))
            .w(width)
            .shadow(box_shadows(&theme.shadow.md))
            .on_click_retry(cx.listener(|view, _: &ClickEvent, _, cx| view.retry_load(cx)));
        Some(
            div()
                .absolute()
                .top(spacing(3.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(banner)
                .into_any_element(),
        )
    }
}
