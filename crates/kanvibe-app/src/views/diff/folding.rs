//! 편집 모드의 접기(Monaco `folding: true`, `foldingStrategy: auto`). Monaco TypeScript 언어는 접기 범위 제공자를 두지 않아
//! 들여쓰기 기준 범위(`IndentRangeProvider.computeRanges`)를 쓰고, TS 언어 설정의 `// #region`·`// #endregion` 표식이 그 위에
//! 범위를 더한다. 비교 모드의 두 편집기는 Monaco diff 편집기가 `folding: false`로 끈다.

use std::collections::BTreeSet;

use super::syntax::Language;
use super::typescript_config::region_marker;

/// 접을 수 있는 범위. `start` 줄은 남고 `start + 1..=end` 줄이 숨는다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldRange {
    pub start: usize,
    pub end: usize,
}

/// Monaco `computeIndentLevel`: 탭은 다음 탭 멈춤까지 센다. 공백뿐인 줄은 `None`
fn indentation(line: &str, tab_size: usize) -> Option<usize> {
    let mut indent = 0;
    for character in line.chars() {
        match character {
            ' ' => indent += 1,
            '\t' => indent = indent - indent % tab_size + tab_size,
            _ => return Some(indent),
        }
    }
    None
}

/// 아래에서 위로 올라가며 열린 들여쓰기 구역을 쌓는 칸(`previousRegions`의 원소)
struct OpenRegion {
    /// `None`은 `// #endregion` 표식이 연 구역이다(Monaco의 `indent: -2`)
    indent: Option<usize>,
    /// 이 구역과 같은 들여쓰기로 시작하는 가장 위 줄
    end_above: usize,
    line: usize,
}

/// Monaco `computeRanges`: 들여쓰기가 더 깊은 줄이 이어지는 줄마다, 다음 같거나 얕은 줄 바로 위(빈 줄 포함)까지 접는다.
/// `// #region` 줄은 짝이 되는 `// #endregion` 줄까지(그 줄 포함) 접는다
pub fn compute_fold_ranges(
    lines: &[String],
    tab_size: usize,
    language: Language,
) -> Vec<FoldRange> {
    let mut ranges = Vec::new();
    let sentinel = OpenRegion {
        indent: None,
        end_above: lines.len(),
        line: lines.len(),
    };
    let mut regions = vec![sentinel];
    for (line_index, line) in lines.iter().enumerate().rev() {
        let Some(indent) = indentation(line, tab_size) else {
            continue;
        };
        match region_marker(language, line) {
            Some(false) => {
                regions.push(OpenRegion {
                    indent: None,
                    end_above: line_index,
                    line: line_index,
                });
                continue;
            }
            Some(true) if close_marker_region(&mut regions, &mut ranges, line_index, indent) => {
                continue;
            }
            _ => {}
        }
        add_indent_region(&mut regions, &mut ranges, line_index, indent);
    }
    ranges.reverse();
    ranges
}

/// `// #region` 줄: 가장 가까운 `// #endregion` 구역까지 버리고 그 구역을 이 줄에서 시작하는 범위로 닫는다.
/// 짝이 없으면 거짓을 돌려 보통 줄로 다룬다
fn close_marker_region(
    regions: &mut Vec<OpenRegion>,
    ranges: &mut Vec<FoldRange>,
    line: usize,
    indent: usize,
) -> bool {
    let Some(marker_index) = (1..regions.len())
        .rev()
        .find(|index| regions[*index].indent.is_none())
    else {
        return false;
    };
    regions.truncate(marker_index + 1);
    let region = &mut regions[marker_index];
    ranges.push(FoldRange {
        start: line,
        end: region.line,
    });
    *region = OpenRegion {
        indent: Some(indent),
        end_above: line,
        line,
    };
    true
}

/// 보통 줄: 더 깊은 구역을 닫아 범위로 남기고, 같은 깊이면 구역 위끝을 올리고, 더 깊으면 새 구역을 연다
fn add_indent_region(
    regions: &mut Vec<OpenRegion>,
    ranges: &mut Vec<FoldRange>,
    line: usize,
    indent: usize,
) {
    // `// #endregion` 구역(-2)과 맨 아래 칸(-1)은 어느 들여쓰기보다 얕다
    let is_deeper = |region: &OpenRegion| region.indent.is_some_and(|open| open > indent);
    if regions.last().is_some_and(is_deeper) {
        while regions.last().is_some_and(is_deeper) {
            regions.pop();
        }
        let end = regions.last().map_or(line, |region| region.end_above) - 1;
        if end > line {
            ranges.push(FoldRange { start: line, end });
        }
    }
    match regions.last_mut() {
        Some(region) if region.indent == Some(indent) => region.end_above = line,
        _ => regions.push(OpenRegion {
            indent: Some(indent),
            end_above: line,
            line,
        }),
    }
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

        let ranges = compute_fold_ranges(&text, 4, Language::TypeScript);

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
        let ranges = compute_fold_ranges(&text, 4, Language::TypeScript);
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

    #[test]
    fn region_markers_fold_through_their_endregion_line_in_typescript() {
        let text = lines(
            "// #region helpers\nfunction f() {\n  x();\n}\n// #endregion\nend\n\nblock {\n  y\n\n}",
        );

        assert_eq!(
            compute_fold_ranges(&text, 4, Language::TypeScript),
            vec![
                FoldRange { start: 0, end: 4 },
                FoldRange { start: 1, end: 2 },
                // 닫는 줄 앞 빈 줄까지 접는다(Monaco `computeRanges`)
                FoldRange { start: 7, end: 9 },
            ]
        );
        assert_eq!(
            compute_fold_ranges(&text, 4, Language::Python),
            vec![
                FoldRange { start: 1, end: 2 },
                FoldRange { start: 7, end: 9 },
            ]
        );
    }

    #[test]
    fn tabs_indent_to_the_next_tab_stop() {
        let text = lines("a\n  \tb\n    c\nd");

        assert_eq!(
            compute_fold_ranges(&text, 4, Language::PlainText),
            vec![FoldRange { start: 0, end: 2 }]
        );
    }
}
