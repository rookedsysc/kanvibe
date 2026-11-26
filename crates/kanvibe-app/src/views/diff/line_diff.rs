//! 원본과 수정본의 줄 대응과 줄 안 변경 범위. TS 앱의 Monaco 기본 diff(`DefaultLinesDiffComputer`, 공백 무시)를 따른다.
//! 줄 정렬은 같은 줄 점수(빈 줄 0.1, 그 밖 1 + ln(1 + 길이))와 연속 일치 가산점을 쓰는 동적 계획법이고,
//! 짧은 공통 줄을 사이에 둔 변경은 합친다. 줄 안 변경은 바뀐 덩어리를 앞뒤 공백을 뺀 글자열로 다시 비교하고,
//! 삽입·삭제 위치를 줄바꿈 같은 경계로 옮긴다. Monaco의 나머지 후처리(단어 확장 등)는 옮기지 않았다.

use std::ops::Range;

/// Monaco도 이보다 긴 입력은 Myers로 넘긴다. 여기서는 점수 없는 최장 공통 부분열로 대신한다
const DYNAMIC_PROGRAMMING_LIMIT: usize = 1700;
const CHARACTER_DYNAMIC_PROGRAMMING_LIMIT: usize = 500;
/// 긴 파일의 가운데 변경 구간을 표로 비교할 최대 칸 수(약 70MB)
const LONG_FILE_TABLE_LIMIT: usize = 4_000_000;
const MAX_SHIFT: usize = 100;

/// 줄 안의 위치. `column`은 줄 텍스트의 바이트 위치다
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPoint {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSpan {
    pub start: TextPoint,
    pub end: TextPoint,
}

/// 줄 덩어리 안의 글자 단위 변경 한 쌍
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InnerChange {
    pub original: TextSpan,
    pub modified: TextSpan,
}

/// 바뀐 줄 덩어리 하나. 줄 번호는 0부터이고 끝은 포함하지 않는다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineChange {
    pub original: Range<usize>,
    pub modified: Range<usize>,
    pub inner_changes: Vec<InnerChange>,
}

/// 두 순서열의 어긋난 구간(동적 계획법 결과)
#[derive(Debug, Clone, PartialEq, Eq)]
struct SequenceDiff {
    first: Range<usize>,
    second: Range<usize>,
}

impl SequenceDiff {
    fn join(&self, next: &SequenceDiff) -> SequenceDiff {
        SequenceDiff {
            first: self.first.start..next.first.end,
            second: self.second.start..next.second.end,
        }
    }
}

pub fn compute_line_changes(original: &[String], modified: &[String]) -> Vec<LineChange> {
    let original_keys: Vec<&str> = original.iter().map(|line| line.trim()).collect();
    let modified_keys: Vec<&str> = modified.iter().map(|line| line.trim()).collect();
    let line_diffs = if original.len() + modified.len() < DYNAMIC_PROGRAMMING_LIMIT {
        align_sequences(&original_keys, &modified_keys, |first, second| {
            line_equality_score(&original[first], &modified[second])
        })
    } else {
        align_long_sequences(&original_keys, &modified_keys)
    };
    let line_diffs = join_diffs_around_short_lines(&original_keys, line_diffs);
    line_diffs
        .into_iter()
        .map(|diff| LineChange {
            inner_changes: compute_inner_changes(original, modified, &diff),
            original: diff.first,
            modified: diff.second,
        })
        .collect()
}

/// Monaco의 줄 일치 점수. 공백만 다른 줄은 조금 덜 쳐준다
fn line_equality_score(original_line: &str, modified_line: &str) -> f64 {
    if original_line != modified_line {
        return 0.99;
    }
    if modified_line.is_empty() {
        0.1
    } else {
        1.0 + (1.0 + modified_line.chars().count() as f64).ln()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Diagonal,
    SkipFirst,
    SkipSecond,
}

/// Monaco `DynamicProgrammingDiffing`. 연속으로 맞는 칸에 가산점을 줘 변경이 한 덩어리로 모이게 한다
fn align_sequences<T: PartialEq>(
    first: &[T],
    second: &[T],
    equality_score: impl Fn(usize, usize) -> f64,
) -> Vec<SequenceDiff> {
    let mut table = AlignmentTable::new(first.len(), second.len());
    for (row, first_item) in first.iter().enumerate() {
        for (column, second_item) in second.iter().enumerate() {
            let equality = (first_item == second_item).then(|| equality_score(row, column));
            table.fill_cell(row, column, equality);
        }
    }
    collect_sequence_diffs(&table.steps, first.len(), second.len())
}

/// 동적 계획법 표. 칸마다 지금까지의 점수, 이어진 대각선 길이, 고른 방향을 둔다
struct AlignmentTable {
    width: usize,
    scores: Vec<f64>,
    run_lengths: Vec<usize>,
    steps: Vec<Step>,
}

impl AlignmentTable {
    fn new(rows: usize, width: usize) -> Self {
        Self {
            width,
            scores: vec![0.0; rows * width],
            run_lengths: vec![0; rows * width],
            steps: vec![Step::SkipFirst; rows * width],
        }
    }

    /// `equality`: 두 원소가 같으면 그 일치 점수. 같은 점수면 대각선, 위, 왼쪽 순으로 고른다
    fn fill_cell(&mut self, row: usize, column: usize, equality: Option<f64>) {
        let index = row * self.width + column;
        let horizontal = if row == 0 {
            0.0
        } else {
            self.scores[index - self.width]
        };
        let vertical = if column == 0 {
            0.0
        } else {
            self.scores[index - 1]
        };
        let previous = (row > 0 && column > 0).then(|| index - self.width - 1);
        let previous_run = previous.map_or(0, |previous| self.run_lengths[previous]);
        let diagonal = equality.map_or(-1.0, |score| {
            let base = previous.map_or(0.0, |previous| self.scores[previous]);
            let is_continuing =
                previous.is_some_and(|previous| self.steps[previous] == Step::Diagonal);
            base + if is_continuing {
                previous_run as f64
            } else {
                0.0
            } + score
        });
        let best = horizontal.max(vertical).max(diagonal);
        (self.steps[index], self.run_lengths[index]) = if best == diagonal {
            (Step::Diagonal, previous_run + 1)
        } else if best == horizontal {
            (Step::SkipFirst, 0)
        } else {
            (Step::SkipSecond, 0)
        };
        self.scores[index] = best;
    }
}

/// 끝에서부터 대각선을 따라가며 맞지 않은 구간을 모은다
fn collect_sequence_diffs(
    steps: &[Step],
    first_len: usize,
    second_len: usize,
) -> Vec<SequenceDiff> {
    let mut diffs = Vec::new();
    let (mut last_first, mut last_second) = (first_len, second_len);
    let mut report = |row: isize, column: isize, diffs: &mut Vec<SequenceDiff>| {
        let (next_first, next_second) = ((row + 1) as usize, (column + 1) as usize);
        if next_first != last_first || next_second != last_second {
            diffs.push(SequenceDiff {
                first: next_first..last_first,
                second: next_second..last_second,
            });
        }
        (last_first, last_second) = (row.max(0) as usize, column.max(0) as usize);
    };
    let (mut row, mut column) = (first_len as isize - 1, second_len as isize - 1);
    while row >= 0 && column >= 0 {
        match steps[row as usize * second_len + column as usize] {
            Step::Diagonal => {
                report(row, column, &mut diffs);
                row -= 1;
                column -= 1;
            }
            Step::SkipFirst => row -= 1,
            Step::SkipSecond => column -= 1,
        }
    }
    report(-1, -1, &mut diffs);
    diffs.reverse();
    diffs
}

/// Monaco `removeVeryShortMatchingLinesBetweenDiffs`: 공백을 뺀 4글자 이하 공통 줄만 사이에 둔 큰 변경은 합친다
fn join_diffs_around_short_lines(
    original_keys: &[&str],
    diffs: Vec<SequenceDiff>,
) -> Vec<SequenceDiff> {
    let mut diffs = diffs;
    for _ in 0..10 {
        let mut joined: Vec<SequenceDiff> = Vec::with_capacity(diffs.len());
        let mut has_joined = false;
        for diff in diffs {
            let Some(previous) = joined.last_mut() else {
                joined.push(diff);
                continue;
            };
            let unchanged_text: String =
                original_keys[previous.first.end..diff.first.start].concat();
            let is_short = unchanged_text
                .chars()
                .filter(|c| !c.is_whitespace())
                .count()
                <= 4;
            let is_large = |diff: &SequenceDiff| diff.first.len() + diff.second.len() > 5;
            if is_short && (is_large(previous) || is_large(&diff)) {
                *previous = previous.join(&diff);
                has_joined = true;
            } else {
                joined.push(diff);
            }
        }
        diffs = joined;
        if !has_joined {
            break;
        }
    }
    diffs
}

/// 줄 덩어리 안의 글자 하나와 그 위치. 앞뒤 공백을 뺀 줄 내용 뒤에 줄바꿈 하나를 둔다
struct SliceCharacters {
    characters: Vec<char>,
    points: Vec<TextPoint>,
    end_point: TextPoint,
}

impl SliceCharacters {
    fn new(lines: &[String], range: &Range<usize>) -> Self {
        let mut characters = Vec::new();
        let mut points = Vec::new();
        for line_index in range.clone() {
            let line = &lines[line_index];
            let trimmed_start = line.len() - line.trim_start().len();
            let trimmed = line.trim();
            for (offset, character) in trimmed.char_indices() {
                characters.push(character);
                points.push(TextPoint {
                    line: line_index,
                    column: trimmed_start + offset,
                });
            }
            characters.push('\n');
            points.push(TextPoint {
                line: line_index,
                column: trimmed_start + trimmed.len(),
            });
        }
        let end_point = lines
            .get(range.end)
            .map(|line| TextPoint {
                line: range.end,
                column: line.len() - line.trim_start().len(),
            })
            .unwrap_or_else(|| {
                points.last().copied().unwrap_or(TextPoint {
                    line: range.end,
                    column: 0,
                })
            });
        Self {
            characters,
            points,
            end_point,
        }
    }

    fn point_at(&self, offset: usize) -> TextPoint {
        self.points.get(offset).copied().unwrap_or(self.end_point)
    }

    /// Monaco `LinesSliceCharSequence.getBoundaryScore`. 줄바꿈 바로 뒤를 가장 좋은 경계로 본다
    fn boundary_score(&self, offset: usize) -> i32 {
        let previous = offset
            .checked_sub(1)
            .and_then(|index| self.characters.get(index));
        let next = self.characters.get(offset);
        let (previous_category, next_category) =
            (boundary_category(previous), boundary_category(next));
        if previous_category == BoundaryCategory::LineBreak {
            return 150;
        }
        let mut score = 0;
        if previous_category != next_category {
            score += 10;
            if previous_category == BoundaryCategory::WordLower
                && next_category == BoundaryCategory::WordUpper
            {
                score += 1;
            }
        }
        score + previous_category.score() + next_category.score()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryCategory {
    WordLower,
    WordUpper,
    WordNumber,
    End,
    Other,
    Separator,
    Space,
    LineBreak,
}

impl BoundaryCategory {
    fn score(self) -> i32 {
        match self {
            Self::WordLower | Self::WordUpper | Self::WordNumber => 0,
            Self::End | Self::LineBreak => 10,
            Self::Other => 2,
            Self::Separator => 30,
            Self::Space => 3,
        }
    }
}

fn boundary_category(character: Option<&char>) -> BoundaryCategory {
    match character {
        None => BoundaryCategory::End,
        Some('\n' | '\r') => BoundaryCategory::LineBreak,
        Some(' ' | '\t') => BoundaryCategory::Space,
        Some(',' | ';') => BoundaryCategory::Separator,
        Some(c) if c.is_ascii_lowercase() => BoundaryCategory::WordLower,
        Some(c) if c.is_ascii_uppercase() => BoundaryCategory::WordUpper,
        Some(c) if c.is_ascii_digit() => BoundaryCategory::WordNumber,
        Some(_) => BoundaryCategory::Other,
    }
}

fn compute_inner_changes(
    original: &[String],
    modified: &[String],
    diff: &SequenceDiff,
) -> Vec<InnerChange> {
    if diff.first.is_empty() || diff.second.is_empty() {
        return Vec::new();
    }
    let original_slice = SliceCharacters::new(original, &diff.first);
    let modified_slice = SliceCharacters::new(modified, &diff.second);
    let character_diffs = if original_slice.characters.len() + modified_slice.characters.len()
        < CHARACTER_DYNAMIC_PROGRAMMING_LIMIT
    {
        align_sequences(
            &original_slice.characters,
            &modified_slice.characters,
            |_, _| 1.0,
        )
    } else {
        align_sequences_by_common_affix(&original_slice.characters, &modified_slice.characters)
    };
    character_diffs
        .into_iter()
        .map(|diff| shift_to_better_boundary(diff, &original_slice, &modified_slice))
        .map(|diff| InnerChange {
            original: TextSpan {
                start: original_slice.point_at(diff.first.start),
                end: original_slice.point_at(diff.first.end),
            },
            modified: TextSpan {
                start: modified_slice.point_at(diff.second.start),
                end: modified_slice.point_at(diff.second.end),
            },
        })
        .collect()
}

/// 긴 파일. 앞뒤 공통 줄을 떼고, 남은 가운데가 표 크기 한도 안이면 점수 없는 동적 계획법으로, 넘으면 한 덩어리로 본다
fn align_long_sequences(first: &[&str], second: &[&str]) -> Vec<SequenceDiff> {
    let [affix] = align_sequences_by_common_affix(first, second)
        .try_into()
        .expect("one diff");
    if affix.first.len() * affix.second.len() > LONG_FILE_TABLE_LIMIT {
        return vec![affix]
            .into_iter()
            .filter(|diff| !diff.first.is_empty() || !diff.second.is_empty())
            .collect();
    }
    align_sequences(
        &first[affix.first.clone()],
        &second[affix.second.clone()],
        |_, _| 1.0,
    )
    .into_iter()
    .map(|diff| SequenceDiff {
        first: diff.first.start + affix.first.start..diff.first.end + affix.first.start,
        second: diff.second.start + affix.second.start..diff.second.end + affix.second.start,
    })
    .collect()
}

/// 앞뒤 공통 부분만 떼어 가운데를 한 변경으로 본다(Monaco는 긴 입력에 Myers를 쓴다)
fn align_sequences_by_common_affix<T: PartialEq>(first: &[T], second: &[T]) -> Vec<SequenceDiff> {
    let prefix = first.iter().zip(second).take_while(|(a, b)| a == b).count();
    let suffix = first[prefix..]
        .iter()
        .rev()
        .zip(second[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    vec![SequenceDiff {
        first: prefix..first.len() - suffix,
        second: prefix..second.len() - suffix,
    }]
}

/// Monaco `shiftDiffToBetterPosition`: 한쪽이 빈 변경을 같은 글자가 이어지는 동안 앞뒤로 밀어 경계 점수가 가장 높은 곳에 둔다
fn shift_to_better_boundary(
    diff: SequenceDiff,
    original: &SliceCharacters,
    modified: &SliceCharacters,
) -> SequenceDiff {
    if diff.first.is_empty() {
        let shift = best_shift(
            diff.second.clone(),
            diff.first.start,
            &modified.characters,
            original.characters.len(),
            |delta_start, delta_end, other| {
                original.boundary_score(other)
                    + modified.boundary_score(delta_start)
                    + modified.boundary_score(delta_end)
            },
        );
        return offset_diff(diff, shift);
    }
    if diff.second.is_empty() {
        let shift = best_shift(
            diff.first.clone(),
            diff.second.start,
            &original.characters,
            modified.characters.len(),
            |delta_start, delta_end, other| {
                modified.boundary_score(other)
                    + original.boundary_score(delta_start)
                    + original.boundary_score(delta_end)
            },
        );
        return offset_diff(diff, shift);
    }
    diff
}

fn best_shift(
    changed: Range<usize>,
    other_offset: usize,
    characters: &[char],
    other_len: usize,
    score: impl Fn(usize, usize, usize) -> i32,
) -> isize {
    let can_shift_left = |delta: usize| {
        changed.start >= delta
            && other_offset >= delta
            && characters[changed.start - delta] == characters[changed.end - delta]
    };
    let can_shift_right = |delta: usize| {
        changed.end + delta < characters.len()
            && other_offset + delta < other_len
            && characters[changed.start + delta] == characters[changed.end + delta]
    };
    let before = (1..MAX_SHIFT)
        .take_while(|delta| can_shift_left(*delta))
        .count() as isize;
    let after = (0..MAX_SHIFT)
        .take_while(|delta| can_shift_right(*delta))
        .count() as isize;
    let mut best = (0, i32::MIN);
    for delta in -before..=after {
        let shifted = |offset: usize| (offset as isize + delta) as usize;
        let candidate = score(
            shifted(changed.start),
            shifted(changed.end),
            shifted(other_offset),
        );
        if candidate > best.1 {
            best = (delta, candidate);
        }
    }
    best.0
}

fn offset_diff(diff: SequenceDiff, delta: isize) -> SequenceDiff {
    let shift = |range: Range<usize>| {
        (range.start as isize + delta) as usize..(range.end as isize + delta) as usize
    };
    SequenceDiff {
        first: shift(diff.first),
        second: shift(diff.second),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_string).collect()
    }

    /// 기준 이미지 fixture(커밋 6854c963까지 `parity/fixtures/make_git_repos.py`의 `src/board.ts`): 원본은 git 출력이라 끝 줄바꿈이 잘려 있다
    fn board_fixture() -> (Vec<String>, Vec<String>) {
        let original = "export interface Card {\n  id: string;\n  title: string;\n  status: \"todo\" | \"progress\" | \"done\";\n}\n\nexport function sortCards(cards: Card[]): Card[] {\n  return [...cards].sort((left, right) => left.title.localeCompare(right.title));\n}\n\nexport function countDone(cards: Card[]): number {\n  return cards.filter((card) => card.status === \"done\").length;\n}";
        let modified = "export interface Card {\n  id: string;\n  title: string;\n  status: \"todo\" | \"progress\" | \"done\";\n}\n\nexport function sortCards(cards: Card[]): Card[] {\n  return [...cards].sort((left, right) => left.title.localeCompare(right.title, \"ko\"));\n}\n\nexport function countOpen(cards: Card[]): number {\n  return cards.length - countDone(cards);\n}\n\nexport function countDone(cards: Card[]): number {\n  return cards.filter((card) => card.status === \"done\").length;\n}\n";
        (lines(original), lines(modified))
    }

    #[test]
    fn inserted_function_joins_the_changed_line_into_one_block_like_monaco() {
        let (original, modified) = board_fixture();

        let changes = compute_line_changes(&original, &modified);

        let ranges: Vec<_> = changes
            .iter()
            .map(|change| (change.original.clone(), change.modified.clone()))
            .collect();
        assert_eq!(ranges, vec![(7..8, 7..12), (13..13, 17..18)]);
    }

    #[test]
    fn inserted_lines_inside_a_block_start_at_a_line_start() {
        let (original, modified) = board_fixture();

        let changes = compute_line_changes(&original, &modified);

        let inserted_lines = changes[0]
            .inner_changes
            .iter()
            .find(|change| change.modified.end.line > change.modified.start.line)
            .expect("multi-line insertion");
        assert_eq!(
            inserted_lines.modified.start,
            TextPoint { line: 8, column: 0 }
        );
        assert_eq!(
            inserted_lines.modified.end,
            TextPoint {
                line: 12,
                column: 0
            }
        );
        assert!(
            changes[0]
                .inner_changes
                .iter()
                .all(|change| change.original.start == change.original.end)
        );
    }

    #[test]
    fn identical_texts_have_no_changes() {
        let text = lines("a\nb\nc");

        assert!(compute_line_changes(&text, &text).is_empty());
    }

    #[test]
    fn whitespace_only_changes_are_ignored_like_the_diff_editor_default() {
        assert!(compute_line_changes(&lines("  a\nb"), &lines("a\nb  ")).is_empty());
    }
}
