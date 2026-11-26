//! 편집기의 찾기·바꾸기(Monaco `Mod+F`, Linux `Ctrl+H`·macOS `Cmd+Alt+F`). 현재 결과는 선택으로 보이고 화면 안으로 스크롤한다.
//! 바꿀 글자는 정규식 모드에서 Monaco처럼 `$1`·`$&`·`$$` 등을 치환하고, 대소문자 보존 토글을 따른다(`replace_pattern`).

use gpui_kit::{App, AppContext, Context, Entity, Window};

use super::DiffFileEditor;
use crate::views::diff::find_session::{FindSession, new_session};
use crate::views::diff::find_widget::{FindEvent, FindWidget, seed_query};
use crate::views::diff::replace_pattern::ReplacePattern;

impl DiffFileEditor {
    /// 위젯을 연다. 이미 열려 있으면 검색어를 새로 채우고 입력란으로 간다
    pub(super) fn open_find(
        &mut self,
        should_show_replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let seed = seed_query(
            self.buffer.lines(),
            self.buffer.selection(),
            self.buffer.cursor(),
        );
        if let Some(session) = &self.find {
            let widget = session.widget.clone();
            widget.update(cx, |widget, cx| {
                widget.reopen(seed.as_deref(), should_show_replace, window, cx)
            });
            return;
        }
        let platform = self.platform;
        let widget =
            cx.new(|cx| FindWidget::new(seed.as_deref().unwrap_or_default(), true, platform, cx));
        let subscription =
            cx.subscribe_in(&widget, window, |this, _, event: &FindEvent, window, cx| {
                this.handle_find_event(*event, window, cx)
            });
        self.find = Some(new_session(widget.clone(), subscription));
        widget.update(cx, |widget, cx| {
            widget.reopen(None, should_show_replace, window, cx)
        });
        self.recompute_find(cx);
        self.select_current_match(cx);
    }

    fn handle_find_event(&mut self, event: FindEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            FindEvent::Changed => {
                self.recompute_find(cx);
                self.select_current_match(cx);
            }
            FindEvent::Next | FindEvent::Previous => {
                self.step_find(event == FindEvent::Next, cx);
            }
            FindEvent::ReplaceOne => self.replace_current(cx),
            FindEvent::ReplaceAll => self.replace_all(cx),
            FindEvent::Close => self.close_find(window, cx),
        }
    }

    /// 결과를 다시 센다. 기준 위치는 선택의 앞(없으면 커서)이다
    pub(super) fn recompute_find(&mut self, cx: &mut Context<Self>) {
        let anchor = self
            .buffer
            .selection()
            .map_or(self.buffer.cursor(), |(start, _)| start);
        let scope = self
            .buffer
            .selection()
            .filter(|(start, end)| start.line != end.line);
        let Some(session) = &mut self.find else {
            return;
        };
        session.recompute(self.buffer.lines(), scope, anchor, cx);
    }

    pub(super) fn step_find(&mut self, is_forward: bool, cx: &mut Context<Self>) -> bool {
        let Some(session) = &mut self.find else {
            return false;
        };
        if session.step(is_forward, cx).is_some() {
            self.select_current_match(cx);
        }
        true
    }

    fn select_current_match(&mut self, cx: &mut Context<Self>) {
        let Some(found) = self
            .find
            .as_ref()
            .and_then(FindSession::current_match)
            .cloned()
        else {
            cx.notify();
            return;
        };
        self.buffer.select_range(found.start, found.end);
        self.after_cursor_move(cx);
    }

    /// 지금 고른 결과를 바꾸고 다음 결과로 간다
    fn replace_current(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.find else {
            return;
        };
        let Some(found) = session.current_match().cloned() else {
            return;
        };
        let (pattern, is_preserving_case) = replace_pattern_of(&session.widget, cx);
        let replacement = pattern.build(&found.captures, is_preserving_case);
        self.buffer
            .replace_ranges(&[(found.start, found.end, replacement)]);
        self.after_edit(cx);
        self.select_current_match(cx);
    }

    fn replace_all(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.find else {
            return;
        };
        let (pattern, is_preserving_case) = replace_pattern_of(&session.widget, cx);
        let replacements: Vec<_> = session
            .matches
            .iter()
            .map(|found| {
                let replacement = pattern.build(&found.captures, is_preserving_case);
                (found.start, found.end, replacement)
            })
            .collect();
        self.buffer.replace_ranges(&replacements);
        self.after_edit(cx);
    }

    /// 닫고 편집기로 포커스를 돌려준다
    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }
}

/// 위젯의 바꿀 글자·정규식 토글로 만든 치환 규칙과 대소문자 보존 여부
fn replace_pattern_of(widget: &Entity<FindWidget>, cx: &App) -> (ReplacePattern, bool) {
    let widget = widget.read(cx);
    let pattern = ReplacePattern::new(&widget.replacement(), widget.options().is_regex);
    (pattern, widget.is_preserving_case())
}
