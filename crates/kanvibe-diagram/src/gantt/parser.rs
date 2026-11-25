//! gantt.jison 문법 중 그림에 드러나는 문장만 읽는다. 클릭·링크(`click`·`href`·`call`)와 접근성 문장은 건너뛴다.

use crate::DiagramError;

/// 작업 머리 태그(ganttDb `tags`)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskTags {
    pub is_active: bool,
    pub is_done: bool,
    pub is_crit: bool,
    pub is_milestone: bool,
}

/// 시작 시각을 아직 계산하지 않은 작업 시작 표현
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawStart {
    /// 바로 앞 작업이 끝나는 때(앞 작업 번호)
    PreviousTaskEnd(Option<usize>),
    /// `after a b`, 날짜 글자, 타임스탬프
    Expression(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawTask {
    pub name: String,
    pub section: String,
    pub id: String,
    pub start: RawStart,
    /// 끝 표현: 기간(`10d`), 날짜, `until a`
    pub end: String,
    pub tags: TaskTags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GanttDocument {
    pub title: String,
    pub date_format: String,
    pub axis_format: Option<String>,
    pub tick_interval: Option<String>,
    /// 빈 글자 = 기본 오늘 선, `off` = 그리지 않음, 그 밖 = 선 스타일
    pub today_marker: String,
    pub excludes: Vec<String>,
    pub includes: Vec<String>,
    pub is_inclusive_end_dates: bool,
    pub has_top_axis: bool,
    /// `tickInterval`의 주 시작 요일(0 = 일요일)
    pub week_start: u32,
    /// 주말 첫날의 ISO 요일(금 5, 토 6)
    pub weekend_start: u32,
    pub tasks: Vec<RawTask>,
}

impl Default for GanttDocument {
    fn default() -> Self {
        Self {
            title: String::new(),
            date_format: String::new(),
            axis_format: None,
            tick_interval: None,
            today_marker: String::new(),
            excludes: Vec::new(),
            includes: Vec::new(),
            is_inclusive_end_dates: false,
            has_top_axis: false,
            week_start: 0,
            weekend_start: 6,
            tasks: Vec::new(),
        }
    }
}

const WEEKDAYS: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

/// 읽는 동안 이어지는 상태: 현재 구역과 자동 작업 번호
#[derive(Default)]
struct ParseState {
    section: String,
    auto_id_count: usize,
    is_in_description_block: bool,
}

pub fn parse(definition: &str) -> Result<GanttDocument, DiagramError> {
    let mut lines = definition
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines.next().unwrap_or("");
    if !header.to_ascii_lowercase().starts_with("gantt") {
        return Err(DiagramError::Syntax("gantt 머리줄".to_string()));
    }
    let mut document = GanttDocument::default();
    let mut state = ParseState::default();
    for line in lines {
        read_line(line, &mut document, &mut state)?;
    }
    Ok(document)
}

fn read_line(
    line: &str,
    document: &mut GanttDocument,
    state: &mut ParseState,
) -> Result<(), DiagramError> {
    if state.is_in_description_block {
        state.is_in_description_block = !line.contains('}');
        return Ok(());
    }
    if read_setting(line, document) || is_ignored_statement(line, state) {
        return Ok(());
    }
    if let Some(section) = keyword_value(line, "section") {
        state.section = section.to_string();
        return Ok(());
    }
    let task = parse_task(line, document, state)?;
    document.tasks.push(task);
    Ok(())
}

/// 대소문자를 가리지 않는 `<키워드><공백><값>`의 값
fn keyword_value<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let head = line.get(..keyword.len())?;
    let rest = &line[keyword.len()..];
    let is_separated = rest.starts_with(char::is_whitespace);
    (head.eq_ignore_ascii_case(keyword) && is_separated).then_some(rest)
}

/// `#`·`;` 앞까지만 값으로 쓰는 설정 문장
fn setting_value<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let value = keyword_value(line, keyword)?;
    let end = value.find(['#', ';']).unwrap_or(value.len());
    Some(&value[..end])
}

fn read_setting(line: &str, document: &mut GanttDocument) -> bool {
    let word_list = |value: &str| -> Vec<String> {
        value
            .to_lowercase()
            .split(|character: char| character.is_whitespace() || character == ',')
            .filter(|word| !word.is_empty())
            .map(str::to_string)
            .collect()
    };
    if let Some(value) = setting_value(line, "dateFormat") {
        document.date_format = value.trim().to_string();
    } else if let Some(value) = setting_value(line, "axisFormat") {
        document.axis_format = Some(value.trim().to_string());
    } else if let Some(value) = setting_value(line, "tickInterval") {
        document.tick_interval = Some(value.trim().to_string());
    } else if let Some(value) = setting_value(line, "excludes") {
        document.excludes = word_list(value.trim());
    } else if let Some(value) = setting_value(line, "includes") {
        document.includes = word_list(value.trim());
    } else if let Some(value) = keyword_value(line, "todayMarker") {
        let end = value.find(';').unwrap_or(value.len());
        document.today_marker = value[..end].trim().to_string();
    } else if let Some(value) = keyword_value(line, "title") {
        document.title = value.trim().to_string();
    } else {
        return read_flag_setting(line, document);
    }
    true
}

fn read_flag_setting(line: &str, document: &mut GanttDocument) -> bool {
    let lower = line.to_ascii_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    match words.as_slice() {
        ["inclusiveenddates", ..] => document.is_inclusive_end_dates = true,
        ["topaxis", ..] => document.has_top_axis = true,
        ["weekday", day, ..] if WEEKDAYS.contains(day) => {
            document.week_start = WEEKDAYS.iter().position(|name| name == day).unwrap_or(0) as u32;
        }
        ["weekend", "friday", ..] => document.weekend_start = 5,
        ["weekend", "saturday", ..] => document.weekend_start = 6,
        _ => return false,
    }
    true
}

fn is_ignored_statement(line: &str, state: &mut ParseState) -> bool {
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("accdescr") && line.contains('{') {
        state.is_in_description_block = !line.contains('}');
        return true;
    }
    ["acctitle", "accdescr", "click ", "href ", "call "]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn parse_task(
    line: &str,
    document: &GanttDocument,
    state: &mut ParseState,
) -> Result<RawTask, DiagramError> {
    let (name, rest) = line
        .split_once(':')
        .ok_or_else(|| DiagramError::Syntax(format!("gantt 작업 형식: {line}")))?;
    let data_end = rest.find(['#', ';']).unwrap_or(rest.len());
    let mut parts: Vec<&str> = rest[..data_end].split(',').collect();
    let tags = take_tags(&mut parts)?;
    let parts: Vec<&str> = parts.iter().map(|part| part.trim()).collect();
    let mut next_auto_id = || {
        state.auto_id_count += 1;
        format!("task{}", state.auto_id_count)
    };
    let previous = document.tasks.len().checked_sub(1);
    let (id, start, end) = match parts.as_slice() {
        [end] => (next_auto_id(), RawStart::PreviousTaskEnd(previous), *end),
        [start, end] => (
            next_auto_id(),
            RawStart::Expression(start.to_string()),
            *end,
        ),
        [id, start, end] => (
            id.to_string(),
            RawStart::Expression(start.to_string()),
            *end,
        ),
        _ => return Err(DiagramError::Syntax(format!("gantt 작업 형식: {line}"))),
    };
    Ok(RawTask {
        name: name.to_string(),
        section: state.section.clone(),
        id,
        start,
        end: end.to_string(),
        tags,
    })
}

/// 앞쪽의 `active`·`done`·`crit`·`milestone` 조각을 떼어 낸다. `vert`는 아직 그리지 못한다
fn take_tags(parts: &mut Vec<&str>) -> Result<TaskTags, DiagramError> {
    let mut tags = TaskTags::default();
    while let Some(first) = parts.first().map(|part| part.trim()) {
        match first {
            "active" => tags.is_active = true,
            "done" => tags.is_done = true,
            "crit" => tags.is_crit = true,
            "milestone" => tags.is_milestone = true,
            "vert" => return Err(DiagramError::Unsupported("gantt vert".to_string())),
            _ => break,
        }
        parts.remove(0);
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 설정과_작업을_읽는다() {
        let document = parse(
            "gantt\n  title Plan\n  dateFormat YYYY-MM-DD\n  excludes weekends\n  section A\n  \
             One :done, a1, 2024-01-01, 3d\n  Two :2d\n",
        )
        .unwrap();
        assert_eq!(document.title, "Plan");
        assert_eq!(document.excludes, vec!["weekends".to_string()]);
        assert_eq!(document.tasks.len(), 2);
        assert!(document.tasks[0].tags.is_done);
        assert_eq!(document.tasks[0].id, "a1");
        assert_eq!(document.tasks[1].id, "task1");
        assert_eq!(document.tasks[1].start, RawStart::PreviousTaskEnd(Some(0)));
    }
}
