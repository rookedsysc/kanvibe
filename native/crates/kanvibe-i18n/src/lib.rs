use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{Display, Formatter},
    fs,
    path::Path,
};

use kanvibe_core::TaskStatus;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    Ko,
    En,
    Zh,
}

impl Locale {
    pub const ALL: [Self; 3] = [Self::Ko, Self::En, Self::Zh];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Ko => "ko",
            Self::En => "en",
            Self::Zh => "zh",
        }
    }

    pub const fn catalog_relative_path(self) -> &'static str {
        match self {
            Self::Ko => "messages/ko.json",
            Self::En => "messages/en.json",
            Self::Zh => "messages/zh.json",
        }
    }

    /// Electron `getNotificationLocale`과 동일하게 `zh-CN` 같은 지역 태그도 기본 언어로 축약한다.
    /// 인식하지 못한 코드는 `None`이며, 호출자는 [`DEFAULT_LOCALE`]로 폴백한다.
    pub fn parse(code: &str) -> Option<Self> {
        if code.starts_with("en") {
            return Some(Self::En);
        }
        if code.starts_with("zh") {
            return Some(Self::Zh);
        }
        if code.starts_with("ko") {
            return Some(Self::Ko);
        }

        None
    }
}

pub const DEFAULT_LOCALE: Locale = Locale::Ko;
pub const MESSAGE_CATALOGS: &[(Locale, &str)] = &[
    (Locale::Ko, "messages/ko.json"),
    (Locale::En, "messages/en.json"),
    (Locale::Zh, "messages/zh.json"),
];

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ColumnLabel {
    pub status: TaskStatus,
    pub label: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct BoardLabels {
    pub locale: Locale,
    pub new_task: String,
    pub all_projects: String,
    pub columns: Vec<ColumnLabel>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct MessageCatalog {
    pub locale: Locale,
    pub strings: BTreeMap<String, String>,
}

impl MessageCatalog {
    pub fn text(&self, path: &str) -> Option<&str> {
        self.strings.get(path).map(String::as_str)
    }

    pub fn format(&self, path: &str, values: &[(&str, &str)]) -> Option<String> {
        let mut message = self.text(path)?.to_owned();
        for (key, value) in values {
            message = message.replace(&format!("{{{key}}}"), value);
        }
        Some(message)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct I18nError(String);

impl I18nError {
    fn missing(path: impl Into<String>) -> Self {
        Self(format!("missing i18n value `{}`", path.into()))
    }
}

impl Display for I18nError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for I18nError {}

pub fn load_board_labels(
    repo_root: impl AsRef<Path>,
    locale: Locale,
) -> Result<BoardLabels, Box<dyn Error + Send + Sync>> {
    let json = load_catalog_json(repo_root, locale)?;

    let board = json
        .get("board")
        .ok_or_else(|| I18nError::missing("board"))?;
    let columns = board
        .get("columns")
        .ok_or_else(|| I18nError::missing("board.columns"))?;

    Ok(BoardLabels {
        locale,
        new_task: required_string(board, "newTask")?,
        all_projects: required_string(board, "allProjects")?,
        columns: TaskStatus::ALL
            .into_iter()
            .map(|status| {
                Ok(ColumnLabel {
                    status,
                    label: required_string(columns, status.as_str())?,
                })
            })
            .collect::<Result<Vec<_>, I18nError>>()?,
    })
}

pub fn load_message_catalog(
    repo_root: impl AsRef<Path>,
    locale: Locale,
) -> Result<MessageCatalog, Box<dyn Error + Send + Sync>> {
    let json = load_catalog_json(repo_root, locale)?;
    let mut strings = BTreeMap::new();
    flatten_strings("", &json, &mut strings);
    Ok(MessageCatalog { locale, strings })
}

fn load_catalog_json(
    repo_root: impl AsRef<Path>,
    locale: Locale,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let catalog_path = repo_root.as_ref().join(locale.catalog_relative_path());
    let catalog = fs::read_to_string(catalog_path)?;
    Ok(serde_json::from_str::<Value>(&catalog)?)
}

fn flatten_strings(prefix: &str, value: &Value, strings: &mut BTreeMap<String, String>) {
    match value {
        Value::String(message) => {
            strings.insert(prefix.to_owned(), message.clone());
        }
        Value::Object(values) => {
            for (key, value) in values {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_strings(&path, value, strings);
            }
        }
        _ => {}
    }
}

fn required_string(parent: &Value, key: &str) -> Result<String, I18nError> {
    parent
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| I18nError::missing(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn native_scope_covers_every_electron_supported_locale() {
        let locales = Locale::ALL.map(Locale::code);

        assert_eq!(DEFAULT_LOCALE, Locale::Ko);
        assert_eq!(locales, ["ko", "en", "zh"]);
        assert_eq!(MESSAGE_CATALOGS.len(), 3);
    }

    #[test]
    fn locale_parsing_matches_electron_prefix_rules() {
        assert_eq!(Locale::parse("zh"), Some(Locale::Zh));
        assert_eq!(Locale::parse("zh-CN"), Some(Locale::Zh));
        assert_eq!(Locale::parse("en-US"), Some(Locale::En));
        assert_eq!(Locale::parse("ko-KR"), Some(Locale::Ko));
        assert_eq!(Locale::parse("fr"), None);
    }

    #[test]
    fn board_labels_load_from_existing_message_catalogs() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let english = load_board_labels(&repo_root, Locale::En).expect("English board labels");
        let korean = load_board_labels(&repo_root, Locale::Ko).expect("Korean board labels");
        let chinese = load_board_labels(&repo_root, Locale::Zh).expect("Chinese board labels");

        assert_eq!(english.new_task, "+ New Task");
        assert_eq!(korean.new_task, "+ 새 작업");
        assert_eq!(chinese.new_task, "+ 新任务");
        assert_eq!(chinese.columns.len(), TaskStatus::ALL.len());
        assert_eq!(
            english
                .columns
                .iter()
                .map(|column| column.label.as_str())
                .collect::<Vec<_>>(),
            ["Todo", "Progress", "Pending", "Review", "Done"]
        );
    }

    #[test]
    fn full_message_catalogs_share_keys_and_format_placeholders() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let english = load_message_catalog(&repo_root, Locale::En).expect("English catalog");
        let korean = load_message_catalog(&repo_root, Locale::Ko).expect("Korean catalog");
        let chinese = load_message_catalog(&repo_root, Locale::Zh).expect("Chinese catalog");

        assert_eq!(
            english.strings.keys().collect::<Vec<_>>(),
            korean.strings.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            english.strings.keys().collect::<Vec<_>>(),
            chinese.strings.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            english.format("common.releaseUpdate.title", &[("version", "1.2.0")]),
            Some("KanVibe 1.2.0 is available".to_owned())
        );
        assert_eq!(
            korean.format("common.unreadCount", &[("count", "3")]),
            Some("읽지 않음 3개".to_owned())
        );
    }
}
