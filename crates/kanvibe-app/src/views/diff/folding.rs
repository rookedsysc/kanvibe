//! 편집 모드의 접기(Monaco `folding: true`, `foldingStrategy: auto`). Monaco TypeScript 언어는 접기 범위 제공자를 두지 않아
//! 들여쓰기 기준 범위(`IndentRangeProvider`)를 쓴다. 비교 모드의 두 편집기는 Monaco diff 편집기가 `folding: false`로 끈다.

use std::collections::BTreeSet;

/// 접을 수 있는 범위. `start` 줄은 남고 `start + 1..=end` 줄이 숨는다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldRange {
    pub start: usize,
    pub end: usize,
}

fn indentation(line: &str, tab_size: usize) -> Option<usize> {
    if line.trim().is_empty() {
        return None;
    }
    Some(
        line.chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .map(|c| if c == '\t' { tab_size } else { 1 })
            .sum(),
    )
}

/// 다음 내용 줄이 더 깊이 들어간 줄마다, 들여쓰기가 다시 그 줄 이하로 돌아오기 전 마지막 내용 줄까지 접는다
pub fn compute_fold_ranges(lines: &[String], tab_size: usize) -> Vec<FoldRange> {
    let indents: Vec<Option<usize>> = lines
        .iter()
        .map(|line| indentation(line, tab_size))
        .collect();
    let mut ranges = Vec::new();
    for (start, indent) in indents.iter().enumerate() {
        let Some(indent) = *indent else {
            continue;
        };
        let mut end = start;
        for (index, candidate) in indents.iter().enumerate().skip(start + 1) {
            match candidate {
                Some(candidate) if *candidate > indent => end = index,
                Some(_) => break,
                None => {}
            }
        }
        if end > start {
            ranges.push(FoldRange { start, end });
        }
    }
    ranges
}

/// 접힌 범위를 시작 줄로 기억한다. 편집으로 범위가 바뀌면 같은 시작 줄의 새 범위를 따른다
#[derive(Debug, Clone, Default)]
pub struct FoldState {
    collapsed_starts: BTreeSet<usize>,
}

impl FoldState {
    pub fn is_collapsed(&self, start: usize) -> bool {
        self.collapsed_starts.contains(&start)
    }

    pub fn toggle(&mut self, start: usize) {
        if !self.collapsed_starts.remove(&start) {
            self.collapsed_starts.insert(start);
        }
    }

    pub fn collapse(&mut self, start: usize) {
        self.collapsed_starts.insert(start);
    }

    pub fn expand(&mut self, start: usize) {
        self.collapsed_starts.remove(&start);
    }

    /// 더는 범위가 시작하지 않는 줄의 접힘 표시를 지운다
    pub fn retain_valid(&mut self, ranges: &[FoldRange]) {
        self.collapsed_starts
            .retain(|start| ranges.iter().any(|range| range.start == *start));
    }

    /// 접힌 범위에 숨은 줄인지
    pub fn hidden_by(&self, ranges: &[FoldRange], line: usize) -> Option<FoldRange> {
        ranges
            .iter()
            .filter(|range| {
                self.is_collapsed(range.start) && range.start < line && line <= range.end
            })
            .min_by_key(|range| range.start)
            .copied()
    }

    /// 화면에 남는 줄 번호들
    pub fn visible_lines(&self, ranges: &[FoldRange], line_count: usize) -> Vec<usize> {
        let mut visible = Vec::with_capacity(line_count);
        let mut line = 0;
        while line < line_count {
            visible.push(line);
            let folded_end = ranges
                .iter()
                .filter(|range| range.start == line && self.is_collapsed(line))
                .map(|range| range.end)
                .max();
            line = folded_end.map_or(line + 1, |end| end + 1);
        }
        visible
    }
}

/// 커서 줄을 감싼 가장 안쪽 범위(Ctrl+Shift+[가 접는 범위). 커서 줄이 범위의 시작이면 그 범위다
pub fn innermost_range_at(ranges: &[FoldRange], line: usize) -> Option<FoldRange> {
    ranges
        .iter()
        .filter(|range| range.start <= line && line <= range.end)
        .max_by_key(|range| range.start)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_string).collect()
    }

    #[test]
    fn indented_blocks_fold_up_to_their_last_inner_line() {
        let text = lines("function a() {\n  if (x) {\n    y();\n  }\n\n  z();\n}\nend");

        let ranges = compute_fold_ranges(&text, 4);

        assert_eq!(
            ranges,
            vec![
                FoldRange { start: 0, end: 5 },
                FoldRange { start: 1, end: 2 }
            ]
        );
    }

    #[test]
    fn collapsed_ranges_hide_their_body_lines() {
        let text = lines("a {\n  b\n  c\n}\nd");
        let ranges = compute_fold_ranges(&text, 4);
        let mut state = FoldState::default();

        state.toggle(0);

        assert_eq!(state.visible_lines(&ranges, text.len()), vec![0, 3, 4]);
        assert_eq!(
            state.hidden_by(&ranges, 2),
            Some(FoldRange { start: 0, end: 2 })
        );
        assert_eq!(
            innermost_range_at(&ranges, 1),
            Some(FoldRange { start: 0, end: 2 })
        );
    }
}
