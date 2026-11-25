//! 편집기 마우스: 누르면 커서, 끌면 선택, 두 번은 단어·세 번은 줄 선택, 줄 번호 옆 접기 단추, 휠 스크롤.

use gpui_kit::{Context, MouseButton, MouseDownEvent, MouseMoveEvent, ScrollEvent, Window, px};

use super::DiffFileEditor;
use crate::views::diff::monaco::{FOLDING_CONTROL_WIDTH, LINE_HEIGHT, hit_test};
use crate::views::diff::viewer::SCROLLBAR_FADE_AFTER;

impl DiffFileEditor {
    pub(super) fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let Some(bounds) = self.surface_bounds.get() else {
            return;
        };
        let pane = self.pane(window);
        let target = hit_test(&pane, bounds, event.position, window);
        let relative_x = f32::from(event.position.x - bounds.origin.x);
        let content_left = pane.content_left();
        let is_on_fold_control =
            relative_x >= content_left - FOLDING_CONTROL_WIDTH && relative_x < content_left;
        if is_on_fold_control
            && self
                .fold_ranges
                .iter()
                .any(|range| range.start == target.line)
        {
            self.fold_state.toggle(target.line);
            self.refresh_visible_lines();
            cx.notify();
            return;
        }
        if self.fold_state.is_collapsed(target.line)
            && target.column == self.buffer.lines()[target.line].len()
        {
            // 접힌 줄 끝의 `⋯`을 누르면 편다
            self.fold_state.expand(target.line);
            self.refresh_visible_lines();
        }
        match event.click_count {
            2 => self.buffer.select_word_at(target),
            count if count >= 3 => self.buffer.select_line(target.line),
            _ => self.buffer.set_cursor(target, event.modifiers.shift),
        }
        self.is_selecting = event.click_count == 1;
        self.after_cursor_move(cx);
    }

    pub(super) fn handle_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.surface_bounds.get() else {
            return;
        };
        let pane = self.pane(window);
        let is_over_gutter = bounds.contains(&event.position)
            && f32::from(event.position.x - bounds.origin.x) < pane.content_left();
        if is_over_gutter != self.is_gutter_hovered {
            self.is_gutter_hovered = is_over_gutter;
            cx.notify();
        }
        if !self.is_selecting || event.pressed_button != Some(MouseButton::Left) {
            self.is_selecting = false;
            return;
        }
        let target = hit_test(&pane, bounds, event.position, window);
        self.buffer.set_cursor(target, true);
        cx.notify();
    }

    pub(super) fn handle_scroll(
        &mut self,
        event: &ScrollEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(bounds) = self.surface_bounds.get() else {
            return;
        };
        let delta = event.delta.pixel_delta(px(LINE_HEIGHT));
        let (max_x, max_y) = self.pane(window).scroll_limits(bounds);
        self.scroll_y = (self.scroll_y - f32::from(delta.y)).clamp(0.0, max_y);
        self.scroll_x = (self.scroll_x - f32::from(delta.x)).clamp(0.0, max_x);
        self.is_scrollbar_visible = true;
        self._scrollbar_fade = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SCROLLBAR_FADE_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                this.is_scrollbar_visible = false;
                cx.notify();
            });
        }));
        cx.notify();
    }
}
