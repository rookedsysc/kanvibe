//! JS `String.prototype.localeCompare`(ICU 기본 정렬)에 가까운 문자열 순서.
//! 대소문자를 먼저 무시하고 비교해 `commit`이 `Review`보다 앞에 오고, 같을 때만 소문자를 앞에 둔다.
//! ICU처럼 공백 < 문장 부호·기호 < 숫자 < 글자 순서로 문자 종류를 먼저 본다.

use std::cmp::Ordering;

fn character_class(character: char) -> u8 {
    if character.is_whitespace() {
        0
    } else if character.is_numeric() {
        2
    } else if character.is_alphabetic() {
        3
    } else {
        1
    }
}

fn primary_key(text: &str) -> impl Iterator<Item = (u8, char)> + '_ {
    text.chars().flat_map(|character| {
        let class = character_class(character);
        character
            .to_lowercase()
            .map(move |lowered| (class, lowered))
    })
}

/// 같은 글자에서 소문자를 대문자보다 앞에 둔다
fn case_key(text: &str) -> impl Iterator<Item = bool> + '_ {
    text.chars().map(char::is_uppercase)
}

pub fn compare_like_locale(left: &str, right: &str) -> Ordering {
    primary_key(left)
        .cmp(primary_key(right))
        .then_with(|| case_key(left).cmp(case_key(right)))
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_case_insensitively_with_lowercase_first_on_ties() {
        let mut labels = vec!["release", "Review", "commit", "plan", "review", "B", "a"];
        labels.sort_by(|left, right| compare_like_locale(left, right));

        assert_eq!(
            labels,
            ["a", "B", "commit", "plan", "release", "review", "Review"]
        );
    }

    #[test]
    fn punctuation_sorts_before_digits_and_letters() {
        let mut labels = vec!["skillA", "skill0", "skill-1", "skill-0"];
        labels.sort_by(|left, right| compare_like_locale(left, right));

        assert_eq!(labels, ["skill-0", "skill-1", "skill0", "skillA"]);
    }
}
