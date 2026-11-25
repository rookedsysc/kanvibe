//! 단어 단위 이동·삭제·선택의 경계(Monaco `WordOperations`, 기본 `wordSeparators`).
//! 단어는 일반 글자가 이어진 묶음이거나 구분 기호가 이어진 묶음이고, 공백은 단어가 아니다.

use std::ops::Range;

/// Monaco `USUAL_WORD_SEPARATORS`
const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterClass {
    Regular,
    Separator,
    Whitespace,
}

fn classify(character: char) -> CharacterClass {
    if character.is_whitespace() {
        CharacterClass::Whitespace
    } else if WORD_SEPARATORS.contains(character) {
        CharacterClass::Separator
    } else {
        CharacterClass::Regular
    }
}

/// 구분 기호 판정(찾기의 "단어 단위로" 경계에도 쓴다)
pub fn is_word_boundary_character(character: char) -> bool {
    classify(character) != CharacterClass::Regular
}

/// Ctrl+Left(macOS Alt+Left): 커서 앞 공백을 건너 앞 단어의 시작으로
pub fn previous_word_start(line: &str, column: usize) -> usize {
    let characters: Vec<(usize, char)> = line[..column].char_indices().collect();
    let mut index = characters.len();
    while index > 0 && classify(characters[index - 1].1) == CharacterClass::Whitespace {
        index -= 1;
    }
    let Some(&(_, last)) = index.checked_sub(1).and_then(|last| characters.get(last)) else {
        return 0;
    };
    let word_class = classify(last);
    while index > 0 && classify(characters[index - 1].1) == word_class {
        index -= 1;
    }
    characters.get(index).map_or(0, |(offset, _)| *offset)
}

/// Ctrl+Right(macOS Alt+Right): 커서 뒤 공백을 건너 다음 단어의 끝으로
pub fn next_word_end(line: &str, column: usize) -> usize {
    let mut characters = line[column..].char_indices().peekable();
    while characters
        .next_if(|(_, character)| classify(*character) == CharacterClass::Whitespace)
        .is_some()
    {}
    let Some(&(_, first)) = characters.peek() else {
        return line.len();
    };
    let word_class = classify(first);
    while characters
        .next_if(|(_, character)| classify(*character) == word_class)
        .is_some()
    {}
    characters
        .peek()
        .map_or(line.len(), |(offset, _)| column + offset)
}

/// 두 번 누른 자리의 단어(같은 종류 글자 묶음). 묶음 끝에 있으면 앞 묶음을 고른다
pub fn word_range_at(line: &str, column: usize) -> Range<usize> {
    let after = line[column..].chars().next();
    let before = line[..column].chars().next_back();
    let target = match (before, after) {
        (Some(before), Some(after))
            if classify(after) == CharacterClass::Whitespace
                && classify(before) != CharacterClass::Whitespace =>
        {
            before
        }
        (_, Some(after)) => after,
        (Some(before), None) => before,
        (None, None) => return column..column,
    };
    let class = classify(target);
    let start = line[..column]
        .char_indices()
        .rev()
        .take_while(|(_, character)| classify(*character) == class)
        .last()
        .map_or(column, |(offset, _)| offset);
    let end = line[column..]
        .char_indices()
        .find(|(_, character)| classify(*character) != class)
        .map_or(line.len(), |(offset, _)| column + offset);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_moves_stop_at_identifier_and_punctuation_runs() {
        let line = "  return cards.length;";

        assert_eq!(previous_word_start(line, line.len()), 21);
        assert_eq!(previous_word_start(line, 21), 15);
        assert_eq!(previous_word_start(line, 15), 14);
        assert_eq!(previous_word_start(line, 9), 2);
        assert_eq!(next_word_end(line, 0), 8);
        assert_eq!(next_word_end(line, 8), 14);
        assert_eq!(next_word_end(line, 14), 15);
    }

    #[test]
    fn double_click_picks_the_word_under_or_just_before_the_pointer() {
        let line = "countOpen(cards)";

        assert_eq!(word_range_at(line, 3), 0..9);
        assert_eq!(word_range_at(line, 9), 9..10);
        assert_eq!(word_range_at("ab cd", 2), 0..2);
    }
}
