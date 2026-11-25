//! `pane_layout_configs` 테이블의 행. 프로젝트가 없고 `is_global`이면 전역 기본값이다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneLayoutType {
    Single,
    #[serde(rename = "horizontal_2")]
    Horizontal2,
    #[serde(rename = "vertical_2")]
    Vertical2,
    LeftRightTb,
    LeftTbRight,
    Quad,
}

impl PaneLayoutType {
    pub const ALL: [PaneLayoutType; 6] = [
        Self::Single,
        Self::Horizontal2,
        Self::Vertical2,
        Self::LeftRightTb,
        Self::LeftTbRight,
        Self::Quad,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Horizontal2 => "horizontal_2",
            Self::Vertical2 => "vertical_2",
            Self::LeftRightTb => "left_right_tb",
            Self::LeftTbRight => "left_tb_right",
            Self::Quad => "quad",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|layout_type| layout_type.as_str() == value)
    }
}

/// pane 하나에서 실행할 명령. `panes` 컬럼에 `[{"position":0,"command":""}]` 모양 JSON으로 저장된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneCommand {
    pub position: i64,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneLayoutConfig {
    pub id: String,
    pub layout_type: PaneLayoutType,
    pub panes: Vec<PaneCommand>,
    pub project_id: Option<String>,
    pub is_global: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_type_names_match_the_stored_values() {
        for layout_type in PaneLayoutType::ALL {
            let json = serde_json::to_string(&layout_type).unwrap();
            assert_eq!(json, format!("\"{}\"", layout_type.as_str()));
            assert_eq!(
                PaneLayoutType::parse(layout_type.as_str()),
                Some(layout_type)
            );
        }
    }

    #[test]
    fn pane_commands_serialize_like_the_typeorm_simple_json_column() {
        let panes = vec![PaneCommand {
            position: 0,
            command: "git status \"quoted\"".into(),
        }];

        assert_eq!(
            serde_json::to_string(&panes).unwrap(),
            r#"[{"position":0,"command":"git status \"quoted\""}]"#
        );
    }
}
