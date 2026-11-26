//! 터미널 오른쪽 스크롤 막대(xterm.js 뷰포트의 기본 스크롤바). 스크롤백 줄을 문서 높이로 보고 페이지 스크롤바와 같은 모양으로 그린다.
//! 스크롤백이 없으면(대체 화면 포함) 막대를 그리지 않는다.

use kanvibe_terminal::screen::TerminalScreen;

use crate::components::scroll_area::{scroll_top_for_thumb_top, thumb_span};

/// 지금 보이는 스크롤백 위치. `display_offset`은 맨 아래에서 위로 올라간 줄 수다
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbackPosition {
    pub history_size: usize,
    pub display_offset: usize,
    pub rows: usize,
    pub cell_height: f32,
}

impl ScrollbackPosition {
    /// 화면의 지금 위치를 읽는다. 대체 화면은 스크롤백이 없어 `history_size`가 0이다
    pub fn read(screen: &TerminalScreen, cell_height: f32) -> Self {
        Self {
            history_size: screen.history_size(),
            display_offset: screen.display_offset(),
            rows: screen.rows(),
            cell_height,
        }
    }

    /// 막대를 그릴 문서 높이와 스크롤 위치. 스크롤백이 없으면 `None`이다
    pub fn page_extent(&self) -> Option<(f32, f32)> {
        if self.history_size == 0 {
            return None;
        }
        let content_height = (self.rows + self.history_size) as f32 * self.cell_height;
        let scroll_top = (self.history_size - self.display_offset) as f32 * self.cell_height;
        Some((content_height, scroll_top))
    }

    /// 막대 홈 안 thumb의 윗변과 길이
    pub fn thumb(&self, track_height: f32) -> Option<(f32, f32)> {
        let (content_height, scroll_top) = self.page_extent()?;
        Some(thumb_span(track_height, content_height, scroll_top))
    }

    /// thumb 윗변을 `thumb_top`으로 끌었을 때의 `display_offset`
    pub fn display_offset_for_thumb_top(&self, track_height: f32, thumb_top: f32) -> usize {
        let Some((content_height, _)) = self.page_extent() else {
            return 0;
        };
        let scroll_top = scroll_top_for_thumb_top(track_height, content_height, thumb_top);
        let lines_from_top = (scroll_top / self.cell_height).round() as usize;
        self.history_size - lines_from_top.min(self.history_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_HEIGHT: f32 = 17.0;
    const TRACK_HEIGHT: f32 = 24.0 * CELL_HEIGHT;

    fn position(history_size: usize, display_offset: usize) -> ScrollbackPosition {
        ScrollbackPosition {
            history_size,
            display_offset,
            rows: 24,
            cell_height: CELL_HEIGHT,
        }
    }

    #[test]
    fn a_screen_without_scrollback_has_no_thumb() {
        assert_eq!(position(0, 0).thumb(TRACK_HEIGHT), None);
    }

    #[test]
    fn the_alternate_screen_hides_the_thumb_of_the_normal_screen_scrollback() {
        let mut screen = TerminalScreen::new(20, 4);
        screen.write("1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n".as_bytes());
        let normal = ScrollbackPosition::read(&screen, CELL_HEIGHT);

        screen.write(b"\x1b[?1049h");
        let alternate = ScrollbackPosition::read(&screen, CELL_HEIGHT);

        assert!(normal.thumb(TRACK_HEIGHT).is_some());
        assert_eq!(alternate.thumb(TRACK_HEIGHT), None);
    }

    #[test]
    fn the_thumb_rises_as_the_view_scrolls_back() {
        let (bottom_top, bottom_length) = position(76, 0).thumb(TRACK_HEIGHT).unwrap();
        let (middle_top, middle_length) = position(76, 38).thumb(TRACK_HEIGHT).unwrap();
        let (oldest_top, oldest_length) = position(76, 76).thumb(TRACK_HEIGHT).unwrap();

        assert_eq!(
            oldest_top, 15.0,
            "가장 오래된 줄을 보면 위 화살표 바로 아래다"
        );
        assert!(oldest_top < middle_top && middle_top < bottom_top);
        assert_eq!(bottom_top + bottom_length, TRACK_HEIGHT - 15.0);
        assert_eq!(bottom_length, middle_length);
        assert_eq!(bottom_length, oldest_length);
    }

    #[test]
    fn a_longer_scrollback_makes_a_shorter_thumb() {
        let (_, short_history_length) = position(24, 0).thumb(TRACK_HEIGHT).unwrap();
        let (_, long_history_length) = position(976, 0).thumb(TRACK_HEIGHT).unwrap();
        assert!(long_history_length < short_history_length);
    }

    #[test]
    fn dragging_the_thumb_to_where_it_is_drawn_keeps_the_scrollback_position() {
        for display_offset in [0, 1, 37, 75, 76] {
            let scrollback = position(76, display_offset);
            let (thumb_top, _) = scrollback.thumb(TRACK_HEIGHT).unwrap();

            let dragged = scrollback.display_offset_for_thumb_top(TRACK_HEIGHT, thumb_top);

            assert_eq!(dragged, display_offset);
        }
    }

    #[test]
    fn dragging_past_the_track_ends_stops_at_the_oldest_and_newest_lines() {
        let scrollback = position(76, 10);
        assert_eq!(
            scrollback.display_offset_for_thumb_top(TRACK_HEIGHT, -50.0),
            76
        );
        assert_eq!(
            scrollback.display_offset_for_thumb_top(TRACK_HEIGHT, TRACK_HEIGHT + 50.0),
            0
        );
    }
}
