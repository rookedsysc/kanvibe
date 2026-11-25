//! c4Db가 문장 인자를 요소 칸에 담는 방식.
//!
//! c4Db의 `add*` 함수는 인자를 자리 순서대로 받는다. 자리 인자는 그 자리 이름의 칸을 채우지만 `$키="값"` 인자는
//! 어느 자리에 있든 `키` 칸을 채운다. 글자 자리(`label`·`techn`·`descr`·`type`)에 온 값은 `{ text }`로 감싸 담고,
//! `sprite`·`tags`·`link` 같은 자리에 온 값은 맨 글로 담는다. 비어 있는 글자 자리는 빈 글로, 비어 있는 맨 글
//! 자리는 값 없음으로 덮어쓰므로 앞 자리에서 채운 칸이 뒤 자리 때문에 지워질 수 있다. 그리는 쪽은 글자 칸을
//! `{ text }`로 여기므로 맨 글이 담긴 글자 칸은 그릴 때 예외가 나고, TS 앱은 문법 오류로 보여 준다.

use std::collections::BTreeMap;

use crate::DiagramError;

/// 인자 하나: 자리 인자이거나 `$키="값"`
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Argument {
    Positional(String),
    Named(String, String),
}

/// 칸 하나에 담긴 값
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FieldValue {
    /// `{ text }` 객체
    Text(String),
    /// 맨 글
    Raw(String),
}

/// `{ text }`로 감싸 담는 자리
const TEXT_SLOTS: [&str; 4] = ["label", "techn", "descr", "type"];

/// 요소·경계·관계 하나의 칸들
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Fields {
    values: BTreeMap<String, FieldValue>,
}

impl Fields {
    pub(crate) fn insert(&mut self, name: &str, value: FieldValue) {
        self.values.insert(name.to_string(), value);
    }

    /// 칸을 자리 순서대로 채운다(c4Db `addPersonOrSystem`·`addContainer`·`addRel` 등)
    pub(crate) fn fill_slots(&mut self, arguments: &[Argument], slots: &[&str]) {
        for (index, slot) in slots.iter().enumerate() {
            let is_text_slot = TEXT_SLOTS.contains(slot);
            match arguments.get(index) {
                Some(Argument::Positional(value)) if is_text_slot => {
                    self.insert(slot, FieldValue::Text(value.clone()));
                }
                Some(Argument::Positional(value)) => {
                    self.insert(slot, FieldValue::Raw(value.clone()))
                }
                Some(Argument::Named(key, value)) if is_text_slot => {
                    self.insert(key, FieldValue::Text(value.clone()));
                }
                Some(Argument::Named(key, value)) => {
                    self.insert(key, FieldValue::Raw(value.clone()))
                }
                None if is_text_slot => self.insert(slot, FieldValue::Text(String::new())),
                None => {
                    self.values.remove(*slot);
                }
            }
        }
    }

    /// `UpdateElementStyle`·`UpdateRelStyle`: 값이 있는 자리만 맨 글로 덮어쓴다
    pub(crate) fn update_slots(&mut self, arguments: &[Argument], slots: &[&str]) {
        for (argument, slot) in arguments.iter().zip(slots) {
            match argument {
                Argument::Positional(value) => self.insert(slot, FieldValue::Raw(value.clone())),
                Argument::Named(key, value) => self.insert(key, FieldValue::Raw(value.clone())),
            }
        }
    }

    /// 글자 칸. 없거나 맨 글이면 빈 글이다
    pub(crate) fn text(&self, name: &str) -> String {
        match self.values.get(name) {
            Some(FieldValue::Text(text)) => text.clone(),
            _ => String::new(),
        }
    }

    /// 맨 글 칸(색 등). 비어 있으면 JS에서 거짓이라 없는 것과 같다
    pub(crate) fn raw(&self, name: &str) -> Option<String> {
        match self.values.get(name) {
            Some(FieldValue::Raw(value)) if !value.is_empty() => Some(value.clone()),
            _ => None,
        }
    }

    /// JS에서 참인 값이 담겼는지(`{ text }` 객체는 빈 글이어도 참이다)
    pub(crate) fn is_truthy(&self, name: &str) -> bool {
        match self.values.get(name) {
            Some(FieldValue::Text(_)) => true,
            Some(FieldValue::Raw(value)) => !value.is_empty(),
            None => false,
        }
    }

    /// 그리는 쪽이 `{ text }`로 읽는 칸에 맨 글이 담겼으면 TS처럼 문법 오류로 끝낸다
    pub(crate) fn validate_text_slots(&self, names: &[&str]) -> Result<(), DiagramError> {
        for name in names {
            let is_broken = match self.values.get(*name) {
                Some(FieldValue::Raw(value)) => *name == "label" || !value.is_empty(),
                _ => false,
            };
            if is_broken {
                return Err(DiagramError::Syntax(format!("C4 글자 칸 `{name}`에 맨 글")));
            }
        }
        Ok(())
    }
}

/// JS `parseInt`: 앞 공백을 건너뛰고 부호와 이어진 숫자만 읽는다. 숫자가 없으면 `None`(NaN)
pub(crate) fn parse_int(text: &str) -> Option<i64> {
    let trimmed = text.trim_start();
    let (sign, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (-1, &trimmed[1..]),
        Some(b'+') => (1, &trimmed[1..]),
        _ => (1, trimmed),
    };
    let end = digits
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse::<i64>().ok().map(|value| sign * value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 이름_인자는_자리와_상관없이_제_칸을_채운다() {
        let mut fields = Fields::default();
        let arguments = [
            Argument::Positional("api".to_string()),
            Argument::Positional("API".to_string()),
            Argument::Named("descr".to_string(), "Serves".to_string()),
            Argument::Named("link".to_string(), "https://x".to_string()),
        ];
        fields.fill_slots(&arguments, &["alias", "label", "techn", "descr", "sprite"]);
        assert_eq!(fields.text("descr"), "Serves");
        assert_eq!(fields.text("techn"), "");
        assert!(fields.is_truthy("link"));
        assert!(!fields.is_truthy("sprite"));
    }

    #[test]
    fn 비어_있는_뒤_자리가_앞에서_채운_칸을_지운다() {
        let mut fields = Fields::default();
        let arguments = [
            Argument::Positional("cli".to_string()),
            Argument::Positional("CLI".to_string()),
            Argument::Named("descr".to_string(), "Dropped".to_string()),
        ];
        fields.fill_slots(&arguments, &["alias", "label", "techn", "descr"]);
        assert_eq!(fields.text("descr"), "");
    }

    #[test]
    fn 글자_칸의_맨_글은_오류다() {
        let mut fields = Fields::default();
        fields.update_slots(
            &[Argument::Named("techn".to_string(), "Rust".to_string())],
            &["bgColor"],
        );
        assert!(fields.validate_text_slots(&["techn"]).is_err());
        assert_eq!(parse_int(" -20px"), Some(-20));
        assert_eq!(parse_int("x"), None);
    }
}
