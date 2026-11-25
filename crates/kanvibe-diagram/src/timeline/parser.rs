//! timeline.jison 문법: `title`, `section`, `기간 : 사건 : 사건`, `: 사건`(앞 기간에 이어 붙임).
//! 사건 구분자는 뒤에 공백이 오는 `:`이다. 머리줄 `timeline TD`는 세로 방향이다.

use crate::DiagramError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Period {
    pub name: String,
    pub section: String,
    pub events: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
    pub title: String,
    /// `timeline TD`(세로 렌더러)
    pub is_vertical: bool,
    /// 나온 순서의 구역 이름
    pub sections: Vec<String>,
    pub periods: Vec<Period>,
}

pub fn parse(definition: &str) -> Result<Timeline, DiagramError> {
    let body = crate::directive::diagram_body(definition);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('%') && !line.starts_with('#'));
    let header = lines.next().unwrap_or("");
    let direction = header.strip_prefix("timeline").map(str::trim);
    let is_vertical = match direction {
        Some("") | Some("LR") => false,
        Some("TD") => true,
        _ => return Err(DiagramError::Syntax("timeline 머리줄".to_string())),
    };
    let mut timeline = Timeline {
        is_vertical,
        ..Timeline::default()
    };
    let mut section = String::new();
    let mut is_in_description_block = false;
    for line in lines {
        let opens_description_block = line.starts_with("accDescr") && line.contains('{');
        if is_in_description_block || opens_description_block {
            is_in_description_block = !line.contains('}');
        } else if line.starts_with("accTitle") || line.starts_with("accDescr") {
            continue;
        } else if let Some(title) = keyword_value(line, "title") {
            timeline.title = title.trim().to_string();
        } else if let Some(name) = keyword_value(line, "section") {
            section = name.split(':').next().unwrap_or("").to_string();
            timeline.sections.push(section.clone());
        } else {
            read_period_line(line, &section, &mut timeline)?;
        }
    }
    Ok(timeline)
}

fn keyword_value<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(keyword)?;
    rest.starts_with(char::is_whitespace).then(|| &rest[1..])
}

/// `기간 : 사건 : 사건`. 기간이 비어 있으면 사건을 앞 기간에 붙인다
fn read_period_line(
    line: &str,
    section: &str,
    timeline: &mut Timeline,
) -> Result<(), DiagramError> {
    let mut parts = split_events(line).into_iter();
    let period = parts.next().unwrap_or_default();
    if !period.is_empty() {
        timeline.periods.push(Period {
            name: period,
            section: section.to_string(),
            events: Vec::new(),
        });
    }
    let current = timeline.periods.last_mut().ok_or_else(|| {
        DiagramError::Syntax(format!("timeline 사건 앞에 기간이 없습니다: {line}"))
    })?;
    current.events.extend(parts);
    Ok(())
}

/// 뒤에 공백이 오는 `:`에서 나눈다. 첫 조각은 기간(`#` 앞까지), 나머지는 사건이다
fn split_events(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let bytes = line.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        let is_separator = *byte == b':'
            && bytes
                .get(index + 1)
                .is_none_or(|next| next.is_ascii_whitespace());
        if is_separator {
            parts.push(line[start..index].to_string());
            start = index + 2;
        }
    }
    parts.push(line.get(start..).unwrap_or("").to_string());
    let period_end = parts[0].find('#').unwrap_or(parts[0].len());
    parts[0].truncate(period_end);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 기간마다_사건을_모은다() {
        let timeline =
            parse("timeline\ntitle T\n2004 : Facebook : Google\n     : Orkut\n2005 : YouTube\n")
                .unwrap();
        assert_eq!(timeline.periods.len(), 2);
        assert_eq!(timeline.periods[0].name, "2004 ");
        assert_eq!(
            timeline.periods[0].events,
            vec!["Facebook ", "Google", "Orkut"]
        );
    }

    #[test]
    fn td_머리줄은_세로_방향이다() {
        assert!(parse("timeline TD\n2002 : A\n").unwrap().is_vertical);
        assert!(!parse("timeline LR\n2002 : A\n").unwrap().is_vertical);
        assert!(!parse("timeline\n2002 : A\n").unwrap().is_vertical);
    }
}
