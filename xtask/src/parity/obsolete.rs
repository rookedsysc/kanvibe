//! `parity/obsolete/*.yaml`. Rust로 옮길 의미가 없는 TS 테스트와 그 이유를 적는다.
//! 형식은 `- id: <ID>` 다음 줄 `  reason: <이유>`의 목록 하나로 제한해 YAML 파서 없이 읽는다.

use std::collections::BTreeMap;
use std::path::Path;

/// ID에서 obsolete 이유로
pub type ObsoleteEntries = BTreeMap<String, String>;

fn unquote(value: &str) -> &str {
    let trimmed = value.trim();
    let is_quoted = trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')));
    if is_quoted {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    }
}

pub fn parse_obsolete_yaml(content: &str, source_name: &str) -> Result<ObsoleteEntries, String> {
    let mut entries = ObsoleteEntries::new();
    let mut pending_id: Option<String> = None;
    for (line_index, line) in content.lines().enumerate() {
        let location = format!("{source_name}:{}", line_index + 1);
        if let Some(id) = line.strip_prefix("- id:") {
            if let Some(previous_id) = pending_id.replace(unquote(id).to_string()) {
                return Err(format!("{location}: {previous_id}에 reason이 없습니다"));
            }
        } else if let Some(reason) = line.trim_start().strip_prefix("reason:") {
            let id = pending_id
                .take()
                .ok_or_else(|| format!("{location}: id 없는 reason"))?;
            let reason = unquote(reason);
            if reason.is_empty() {
                return Err(format!("{location}: {id}의 reason이 비어 있습니다"));
            }
            entries.insert(id, reason.to_string());
        }
    }
    match pending_id {
        Some(id) => Err(format!("{source_name}: {id}에 reason이 없습니다")),
        None => Ok(entries),
    }
}

pub fn read_obsolete_entries(directory: &Path) -> Result<ObsoleteEntries, String> {
    let mut entries = ObsoleteEntries::new();
    let Ok(directory_entries) = std::fs::read_dir(directory) else {
        return Ok(entries);
    };
    for directory_entry in directory_entries {
        let path = directory_entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("yaml") {
            continue;
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        entries.extend(parse_obsolete_yaml(&content, &path.display().to_string())?);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_id_and_reason_pairs() {
        let content = "# 주석\n- id: TS-a-b-1\n  reason: \"이유: 콜론 포함\"\n- id: TS-a-b-2\n  reason: 다른 이유\n";

        let entries = parse_obsolete_yaml(content, "test.yaml").unwrap();

        assert_eq!(entries["TS-a-b-1"], "이유: 콜론 포함");
        assert_eq!(entries["TS-a-b-2"], "다른 이유");
    }

    #[test]
    fn rejects_an_entry_without_a_reason() {
        assert!(parse_obsolete_yaml("- id: TS-a-b-1\n", "test.yaml").is_err());
        assert!(parse_obsolete_yaml("- id: TS-a-b-1\n  reason: \"\"\n", "test.yaml").is_err());
    }
}
