use std::path::Path;

use serde::Deserialize;

/// ledger 한 행. 나머지 필드는 집계에 쓰지 않는다
#[derive(Debug, Clone, Deserialize)]
pub struct LedgerRow {
    pub id: String,
    pub file: String,
    pub title: String,
}

impl LedgerRow {
    /// `TS-<영역>-<파일>-<해시>`의 두 번째 조각
    pub fn area(&self) -> &str {
        self.id.split('-').nth(1).unwrap_or_default()
    }
}

pub fn read_ledger(ledger_path: &Path) -> Result<Vec<LedgerRow>, String> {
    let content = std::fs::read_to_string(ledger_path)
        .map_err(|error| format!("{}을 읽지 못했습니다: {error}", ledger_path.display()))?;
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line)
                .map_err(|error| format!("ledger {}번째 줄: {error}", index + 1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_is_the_second_id_segment() {
        let row = LedgerRow {
            id: "TS-shared-boardSort-d39fb9".into(),
            file: String::new(),
            title: String::new(),
        };

        assert_eq!(row.area(), "shared");
    }
}
