//! 열린 찾기 위젯 하나와 그 결과. 편집 모드 편집기와 비교 모드 보기가 함께 쓴다(Monaco `FindModel`).
//! 결과를 다시 셀 때는 기준 위치(커서) 뒤의 첫 결과를 현재 결과로 고르고, 이전·다음은 끝에서 처음으로 돈다.

use gpui_kit::{App, Entity, Subscription};

use super::find_widget::{FindWidget, MatchSummary};
use super::line_diff::TextPoint;
use super::search::{FindMatch, find_matches};

pub struct FindSession {
    pub widget: Entity<FindWidget>,
    pub matches: Vec<FindMatch>,
    pub current: Option<usize>,
    pub _subscription: Subscription,
}

impl FindSession {
    pub fn current_match(&self) -> Option<&FindMatch> {
        self.current.and_then(|index| self.matches.get(index))
    }

    /// 검색어·토글·범위가 바뀌었거나 문서가 바뀌었을 때 다시 센다. `scope`가 있으면 그 안의 결과만 남긴다
    pub fn recompute(
        &mut self,
        lines: &[String],
        scope: Option<(TextPoint, TextPoint)>,
        anchor: TextPoint,
        cx: &mut App,
    ) {
        let widget = self.widget.read(cx);
        let (query, options) = (widget.query(), widget.options());
        let scope = scope.filter(|_| widget.is_in_selection());
        let result = find_matches(lines, &query, options);
        let summary = match result {
            Err(_) => {
                self.matches.clear();
                MatchSummary::InvalidPattern
            }
            Ok(found) => {
                self.matches = found
                    .into_iter()
                    .filter(|found| scope.is_none_or(|scope| is_inside(found, scope)))
                    .collect();
                self.current = self
                    .matches
                    .iter()
                    .position(|found| found.start >= anchor)
                    .or((!self.matches.is_empty()).then_some(0));
                self.summary_for(&query)
            }
        };
        self.widget
            .update(cx, |widget, cx| widget.set_summary(summary, cx));
    }

    fn summary_for(&self, query: &str) -> MatchSummary {
        if query.is_empty() {
            MatchSummary::NoQuery
        } else if self.matches.is_empty() {
            MatchSummary::NoResults
        } else {
            MatchSummary::Position {
                current: self.current,
                total: self.matches.len(),
            }
        }
    }

    /// 다음(이전) 결과로 옮기고 그 결과를 돌려준다
    pub fn step(&mut self, is_forward: bool, cx: &mut App) -> Option<FindMatch> {
        let total = self.matches.len();
        if total == 0 {
            return None;
        }
        let next = match (self.current, is_forward) {
            (Some(current), true) => (current + 1) % total,
            (Some(current), false) => (current + total - 1) % total,
            (None, true) => 0,
            (None, false) => total - 1,
        };
        self.current = Some(next);
        let summary = MatchSummary::Position {
            current: self.current,
            total,
        };
        self.widget
            .update(cx, |widget, cx| widget.set_summary(summary, cx));
        self.matches.get(next).cloned()
    }
}

fn is_inside(found: &FindMatch, (start, end): (TextPoint, TextPoint)) -> bool {
    found.start >= start && found.end <= end
}

/// 열린 위젯의 사건을 구독할 때 쓰는 생성 도우미
pub fn new_session(widget: Entity<FindWidget>, subscription: Subscription) -> FindSession {
    FindSession {
        widget,
        matches: Vec::new(),
        current: None,
        _subscription: subscription,
    }
}
