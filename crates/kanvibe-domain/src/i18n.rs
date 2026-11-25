//! 화면 문구 번역. `messages/{ko,en,zh}.json`을 빌드할 때 바이너리에 넣고, TS 앱의 next-intl과 같은 규칙으로 찾는다.
//! 키는 `.`으로 이어진 경로이고, 다른 locale로 대신 찾지 않는다. 찾지 못하거나 값을 채우지 못하면
//! next-intl 기본 동작처럼 `네임스페이스.키` 경로 문자열을 그대로 보여 준다.

use std::sync::LazyLock;

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    Ko,
    En,
    Zh,
}

impl Locale {
    pub const DEFAULT: Locale = Locale::Ko;
    pub const SUPPORTED: [Locale; 3] = [Locale::Ko, Locale::En, Locale::Zh];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ko => "ko",
            Self::En => "en",
            Self::Zh => "zh",
        }
    }

    /// 지원하는 locale 코드일 때만 돌려준다
    pub fn parse(code: &str) -> Option<Self> {
        Self::SUPPORTED
            .into_iter()
            .find(|locale| locale.as_str() == code)
    }

    /// 경로의 locale 조각을 읽고, 없거나 모르는 값이면 기본 locale(한국어)을 쓴다
    pub fn parse_or_default(code: Option<&str>) -> Self {
        code.and_then(Self::parse).unwrap_or(Self::DEFAULT)
    }
}

fn parse_bundled_messages(source: &str) -> Value {
    serde_json::from_str(source).expect("messages/*.json은 빌드에 포함된 올바른 JSON이다")
}

static KO_MESSAGES: LazyLock<Value> =
    LazyLock::new(|| parse_bundled_messages(include_str!("../../../messages/ko.json")));
static EN_MESSAGES: LazyLock<Value> =
    LazyLock::new(|| parse_bundled_messages(include_str!("../../../messages/en.json")));
static ZH_MESSAGES: LazyLock<Value> =
    LazyLock::new(|| parse_bundled_messages(include_str!("../../../messages/zh.json")));

fn bundled_messages(locale: Locale) -> &'static Value {
    match locale {
        Locale::Ko => &KO_MESSAGES,
        Locale::En => &EN_MESSAGES,
        Locale::Zh => &ZH_MESSAGES,
    }
}

fn find_message(locale: Locale, key_path: &str) -> Option<&'static str> {
    key_path
        .split('.')
        .try_fold(bundled_messages(locale), |node, segment| node.get(segment))?
        .as_str()
}

/// 문구 안 `{이름}` 자리를 채운다. 값이 없는 자리가 있거나 중괄호가 닫히지 않으면 채우지 못한 것으로 본다
fn fill_placeholders(template: &str, values: &[(&str, &str)]) -> Option<String> {
    let mut filled = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open_index) = rest.find('{') {
        filled.push_str(&rest[..open_index]);
        let after_open = &rest[open_index + 1..];
        let close_index = after_open.find('}')?;
        let placeholder_name = after_open[..close_index].trim();
        let (_, value) = values.iter().find(|(name, _)| *name == placeholder_name)?;
        filled.push_str(value);
        rest = &after_open[close_index + 1..];
    }
    if rest.contains('}') {
        return None;
    }
    filled.push_str(rest);
    Some(filled)
}

pub fn has_message(locale: Locale, key_path: &str) -> bool {
    find_message(locale, key_path).is_some()
}

/// 전체 키 경로로 문구를 찾아 `{이름}` 자리를 채운다
pub fn translate(locale: Locale, key_path: &str, values: &[(&str, &str)]) -> String {
    find_message(locale, key_path)
        .and_then(|template| fill_placeholders(template, values))
        .unwrap_or_else(|| key_path.to_string())
}

/// 한 네임스페이스 아래 키를 찾는 번역기. next-intl `useTranslations(namespace)`에 해당한다
#[derive(Debug, Clone, Copy)]
pub struct Translator {
    locale: Locale,
    namespace: &'static str,
}

impl Translator {
    pub fn new(locale: Locale, namespace: &'static str) -> Self {
        Self { locale, namespace }
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    fn key_path(&self, key: &str) -> String {
        if self.namespace.is_empty() {
            key.to_string()
        } else {
            format!("{}.{key}", self.namespace)
        }
    }

    pub fn translate(&self, key: &str) -> String {
        translate(self.locale, &self.key_path(key), &[])
    }

    pub fn translate_with(&self, key: &str, values: &[(&str, &str)]) -> String {
        translate(self.locale, &self.key_path(key), values)
    }

    pub fn has(&self, key: &str) -> bool {
        has_message(self.locale, &self.key_path(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf_paths(node: &Value, prefix: &str, paths: &mut Vec<String>) {
        match node.as_object() {
            Some(children) => {
                for (key, child) in children {
                    let child_prefix = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    leaf_paths(child, &child_prefix, paths);
                }
            }
            None => paths.push(prefix.to_string()),
        }
    }

    #[test]
    fn locale_codes_fall_back_to_korean() {
        assert_eq!(Locale::parse("en"), Some(Locale::En));
        assert_eq!(Locale::parse("fr"), None);
        assert_eq!(Locale::parse_or_default(Some("fr")), Locale::Ko);
        assert_eq!(Locale::parse_or_default(None), Locale::Ko);
    }

    #[test]
    fn every_bundled_message_is_found_by_its_path() {
        for locale in Locale::SUPPORTED {
            let mut paths = Vec::new();
            leaf_paths(bundled_messages(locale), "", &mut paths);
            assert!(paths.len() > 700, "{locale:?}: {}", paths.len());
            assert!(paths.iter().all(|path| has_message(locale, path)));
        }
    }

    #[test]
    fn placeholders_are_filled_and_missing_values_fall_back_to_the_path() {
        let common = Translator::new(Locale::En, "common");

        assert_eq!(
            common.translate_with("unreadCount", &[("count", "3")]),
            "3 unread"
        );
        assert_eq!(common.translate("unreadCount"), "common.unreadCount");
        assert_eq!(
            Translator::new(Locale::Ko, "common").translate_with("unreadCount", &[("count", "3")]),
            "읽지 않음 3개"
        );
    }

    #[test]
    fn missing_keys_and_namespaces_return_the_full_path_without_borrowing_another_locale() {
        assert!(has_message(Locale::En, "common.prMergeAlert.batchMessage"));
        assert!(!has_message(Locale::Zh, "common.prMergeAlert.batchMessage"));
        assert_eq!(
            translate(Locale::Zh, "common.prMergeAlert.batchMessage", &[]),
            "common.prMergeAlert.batchMessage"
        );
        assert_eq!(translate(Locale::En, "common", &[]), "common");
        assert_eq!(
            Translator::new(Locale::En, "nope").translate("missing"),
            "nope.missing"
        );
    }

    #[test]
    fn unbalanced_braces_fail_like_a_message_format_error() {
        assert_eq!(fill_placeholders("a {b", &[("b", "x")]), None);
        assert_eq!(fill_placeholders("a } b", &[]), None);
        assert_eq!(
            fill_placeholders("{ name }!", &[("name", "x")]).as_deref(),
            Some("x!")
        );
    }
}
