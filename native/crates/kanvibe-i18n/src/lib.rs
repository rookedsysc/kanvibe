use std::{
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
}

impl Locale {
    pub const ALL: [Self; 2] = [Self::Ko, Self::En];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Ko => "ko",
            Self::En => "en",
        }
    }

    pub const fn catalog_relative_path(self) -> &'static str {
        match self {
            Self::Ko => "messages/ko.json",
            Self::En => "messages/en.json",
        }
    }

    pub fn parse(code: &str) -> Option<Self> {
        match code {
            "ko" => Some(Self::Ko),
            "en" => Some(Self::En),
            _ => None,
        }
    }
}

pub const DEFAULT_LOCALE: Locale = Locale::Ko;
pub const MESSAGE_CATALOGS: &[(Locale, &str)] = &[
    (Locale::Ko, "messages/ko.json"),
    (Locale::En, "messages/en.json"),
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
    let catalog_path = repo_root.as_ref().join(locale.catalog_relative_path());
    let catalog = fs::read_to_string(&catalog_path)?;
    let json = serde_json::from_str::<Value>(&catalog)?;

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
    fn native_scope_starts_with_ko_and_en_catalogs() {
        let locales = Locale::ALL.map(Locale::code);

        assert_eq!(DEFAULT_LOCALE, Locale::Ko);
        assert_eq!(locales, ["ko", "en"]);
        assert_eq!(MESSAGE_CATALOGS.len(), 2);
    }

    #[test]
    fn board_labels_load_from_existing_message_catalogs() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let english = load_board_labels(&repo_root, Locale::En).expect("English board labels");
        let korean = load_board_labels(&repo_root, Locale::Ko).expect("Korean board labels");

        assert_eq!(english.new_task, "+ New Task");
        assert_eq!(korean.new_task, "+ 새 작업");
        assert_eq!(
            english
                .columns
                .iter()
                .map(|column| column.label.as_str())
                .collect::<Vec<_>>(),
            ["Todo", "Progress", "Pending", "Review", "Done"]
        );
    }
}
