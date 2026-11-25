//! journey.jison 문법: `title`, `section`, `작업: 점수: 참여자, …`. 접근성 문장은 건너뛴다.

use crate::DiagramError;

#[derive(Clone, Debug, PartialEq)]
pub struct JourneyTask {
    pub name: String,
    pub section: String,
    /// 숫자가 아니면 NaN(얼굴은 일자 입)
    pub score: f64,
    pub people: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Journey {
    pub title: String,
    pub tasks: Vec<JourneyTask>,
}

impl Journey {
    /// 모든 작업의 참여자를 겹치지 않게 모아 정렬한 범례 순서
    pub fn actors(&self) -> Vec<String> {
        let mut actors: Vec<String> = self
            .tasks
            .iter()
            .flat_map(|task| task.people.iter().cloned())
            .collect();
        actors.sort();
        actors.dedup();
        actors
    }
}

/// `#`·`;` 앞까지만 쓴다
fn before_comment(text: &str) -> &str {
    let end = text.find(['#', ';']).unwrap_or(text.len());
    &text[..end]
}

pub fn parse(definition: &str) -> Result<Journey, DiagramError> {
    let body = crate::directive::diagram_body(definition);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('%') && !line.starts_with('#'));
    let header = lines.next().unwrap_or("");
    if !header.starts_with("journey") {
        return Err(DiagramError::Syntax("journey 머리줄".to_string()));
    }
    let mut journey = Journey::default();
    let mut section = String::new();
    let mut is_in_description_block = false;
    for line in lines {
        let opens_description_block = line.starts_with("accDescr") && line.contains('{');
        if is_in_description_block || opens_description_block {
            is_in_description_block = !line.contains('}');
        } else if line.starts_with("accTitle") || line.starts_with("accDescr") {
            continue;
        } else if let Some(title) = keyword_value(line, "title") {
            journey.title = before_comment(title).trim().to_string();
        } else if let Some(name) = keyword_value(line, "section") {
            let end = name.find(['#', ':', ';']).unwrap_or(name.len());
            section = name[..end].to_string();
        } else {
            journey.tasks.push(parse_task(line, &section)?);
        }
    }
    Ok(journey)
}

fn keyword_value<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(keyword)?;
    rest.starts_with(char::is_whitespace).then(|| &rest[1..])
}

/// `이름: 점수: 참여자1, 참여자2`
fn parse_task(line: &str, section: &str) -> Result<JourneyTask, DiagramError> {
    let (name, data) = line
        .split_once(':')
        .ok_or_else(|| DiagramError::Syntax(format!("journey 작업 형식: {line}")))?;
    let mut pieces = before_comment(data).split(':');
    let score = pieces.next().unwrap_or("").trim();
    let score = if score.is_empty() {
        0.0
    } else {
        score.parse().unwrap_or(f64::NAN)
    };
    let people = pieces
        .next()
        .map(|people| {
            people
                .split(',')
                .map(|person| person.trim().to_string())
                .collect()
        })
        .unwrap_or_default();
    Ok(JourneyTask {
        name: name.to_string(),
        section: section.to_string(),
        score,
        people,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 작업과_참여자를_읽는다() {
        let journey =
            parse("journey\ntitle Day\nsection Go\nMake tea: 5: Me\nDo work: 1: Me, Cat\n")
                .unwrap();
        assert_eq!(journey.title, "Day");
        assert_eq!(
            journey.tasks[1].people,
            vec!["Me".to_string(), "Cat".to_string()]
        );
        assert_eq!(journey.tasks[0].section, "Go");
        assert_eq!(journey.actors(), vec!["Cat".to_string(), "Me".to_string()]);
    }
}
