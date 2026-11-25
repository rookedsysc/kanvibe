//! ganttDb의 `compileTasks`: 작업마다 시작·끝 시각을 정한다. `after`·`until`이 뒤쪽 작업을 가리킬 수 있어
//! 모든 작업이 정해질 때까지 최대 11번 다시 훑는다. `excludes`가 있으면 제외한 날만큼 끝을 늦춘다.

use super::calendar::{
    CivilTime, MILLIS_PER_DAY, MILLIS_PER_HOUR, MILLIS_PER_MINUTE, MILLIS_PER_SECOND,
    WEEKDAY_NAMES, add_months, now_millis, start_of_day,
};
use super::date_format::{format, parse_strict};
use super::parser::{GanttDocument, RawStart, RawTask, TaskTags};
use crate::DiagramError;

/// ganttDb `getTasks`의 최대 재시도 횟수(첫 번과 별도로 10번)
const MAX_COMPILE_PASSES: usize = 11;

#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledTask {
    pub name: String,
    pub section: String,
    pub id: String,
    pub start: i64,
    pub end: i64,
    /// 제외한 날 때문에 끝이 늦춰졌을 때 막대를 그릴 끝
    pub render_end: Option<i64>,
    pub tags: TaskTags,
    /// 정의에 나온 순서(막대가 놓이는 줄)
    pub order: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct TaskTimes {
    start: Option<i64>,
    end: Option<i64>,
    render_end: Option<i64>,
}

pub fn schedule(document: &GanttDocument) -> Result<Vec<ScheduledTask>, DiagramError> {
    let mut times = vec![TaskTimes::default(); document.tasks.len()];
    for _ in 0..MAX_COMPILE_PASSES {
        let mut is_complete = true;
        for index in 0..document.tasks.len() {
            times[index] = compile_task(document, &times, index)?;
            is_complete &= times[index].end.is_some();
        }
        if is_complete {
            break;
        }
    }
    document
        .tasks
        .iter()
        .zip(&times)
        .enumerate()
        .map(|(order, (task, times))| scheduled_task(task, times, order))
        .collect()
}

fn scheduled_task(
    task: &RawTask,
    times: &TaskTimes,
    order: usize,
) -> Result<ScheduledTask, DiagramError> {
    let (Some(start), Some(end)) = (times.start, times.end) else {
        return Err(DiagramError::Syntax(format!(
            "gantt 작업 시각: {}",
            task.id
        )));
    };
    Ok(ScheduledTask {
        name: task.name.clone(),
        section: task.section.clone(),
        id: task.id.clone(),
        start,
        end,
        render_end: times.render_end,
        tags: task.tags,
        order,
    })
}

fn compile_task(
    document: &GanttDocument,
    times: &[TaskTimes],
    index: usize,
) -> Result<TaskTimes, DiagramError> {
    let task = &document.tasks[index];
    let start = match &task.start {
        RawStart::PreviousTaskEnd(Some(previous)) => times[*previous].end,
        RawStart::PreviousTaskEnd(None) => {
            return Err(DiagramError::Syntax(format!(
                "gantt 첫 작업 시작: {}",
                task.name
            )));
        }
        RawStart::Expression(expression) => start_date(document, times, expression)?,
    };
    let Some(start) = start else {
        return Ok(times[index]);
    };
    let Some(end) = end_date(document, times, start, &task.end) else {
        return Ok(TaskTimes {
            start: Some(start),
            ..times[index]
        });
    };
    let is_manual_end = parse_strict(task.end.trim(), "YYYY-MM-DD").is_some();
    let (end, render_end) = if document.excludes.is_empty() || is_manual_end {
        (end, None)
    } else {
        skip_excluded_days(document, start + MILLIS_PER_DAY, end)
    };
    Ok(TaskTimes {
        start: Some(start),
        end: Some(end),
        render_end,
    })
}

/// `after`·`until` 뒤의 작업 id들
fn referenced_ids<'a>(expression: &'a str, keyword: &str) -> Option<Vec<&'a str>> {
    let rest = expression.strip_prefix(keyword)?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let ids = rest.trim_start();
    let end = ids
        .find(|character: char| !(character.is_alphanumeric() || "_- ".contains(character)))
        .unwrap_or(ids.len());
    Some(ids[..end].split(' ').collect())
}

fn find_task(document: &GanttDocument, id: &str) -> Option<usize> {
    document.tasks.iter().rposition(|task| task.id == id)
}

fn today_start() -> i64 {
    start_of_day(now_millis())
}

/// ganttDb `getStartDate`. 아직 정해지지 않은 작업을 가리키면 `Ok(None)`
fn start_date(
    document: &GanttDocument,
    times: &[TaskTimes],
    expression: &str,
) -> Result<Option<i64>, DiagramError> {
    let expression = expression.trim();
    let is_timestamp_format = matches!(document.date_format.as_str(), "x" | "X");
    if is_timestamp_format && expression.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(expression.parse().ok());
    }
    if let Some(ids) = referenced_ids(expression, "after") {
        let found = ids.iter().filter_map(|id| find_task(document, id));
        let latest = found.reduce(|latest, index| {
            let is_later =
                matches!((times[index].end, times[latest].end), (Some(a), Some(b)) if a > b);
            if is_later { index } else { latest }
        });
        return Ok(latest.map_or(Some(today_start()), |index| times[index].end));
    }
    parse_strict(expression, &document.date_format)
        .map(Some)
        .ok_or_else(|| DiagramError::Syntax(format!("gantt 날짜: {expression}")))
}

/// ganttDb `getEndDate`. 아직 정해지지 않은 작업을 가리키면 `None`
fn end_date(
    document: &GanttDocument,
    times: &[TaskTimes],
    start: i64,
    expression: &str,
) -> Option<i64> {
    let expression = expression.trim();
    if let Some(ids) = referenced_ids(expression, "until") {
        let found = ids.iter().filter_map(|id| find_task(document, id));
        let earliest = found.reduce(|earliest, index| {
            let is_earlier =
                matches!((times[index].start, times[earliest].start), (Some(a), Some(b)) if a < b);
            if is_earlier { index } else { earliest }
        });
        return earliest.map_or(Some(today_start()), |index| times[index].start);
    }
    if let Some(date) = parse_strict(expression, &document.date_format) {
        let inclusive_shift = if document.is_inclusive_end_dates {
            MILLIS_PER_DAY
        } else {
            0
        };
        return Some(date + inclusive_shift);
    }
    Some(parse_duration(expression).map_or(start, |(value, unit)| add_duration(start, value, unit)))
}

/// ganttDb `parseDuration`: `^(\d+(?:\.\d+)?)([Mdhmswy]|ms)$`
fn parse_duration(expression: &str) -> Option<(f64, &str)> {
    let number_end =
        expression.find(|character: char| !(character.is_ascii_digit() || character == '.'))?;
    let (number, unit) = expression.split_at(number_end);
    let is_number = !number.is_empty()
        && !number.starts_with('.')
        && !number.ends_with('.')
        && number.matches('.').count() <= 1;
    let is_unit = matches!(unit, "M" | "d" | "h" | "m" | "s" | "w" | "y" | "ms");
    (is_number && is_unit)
        .then(|| number.parse().ok().map(|value| (value, unit)))
        .flatten()
}

/// dayjs `add(value, unit)`: 날·주는 반올림한 날 수, 달·해는 정수 부분만 달력으로 더한다
fn add_duration(start: i64, value: f64, unit: &str) -> i64 {
    let round_half_up = |number: f64| (number + 0.5).floor() as i64;
    let elapsed = |unit_millis: i64| start + (value * unit_millis as f64).trunc() as i64;
    match unit {
        "ms" => elapsed(1),
        "s" => elapsed(MILLIS_PER_SECOND),
        "m" => elapsed(MILLIS_PER_MINUTE),
        "h" => elapsed(MILLIS_PER_HOUR),
        "d" => start + round_half_up(value) * MILLIS_PER_DAY,
        "w" => start + round_half_up(value * 7.0) * MILLIS_PER_DAY,
        "M" => add_months(start, value.trunc() as i64),
        _ => add_months(start, value.trunc() as i64 * 12),
    }
}

/// ganttDb `fixTaskDates`: 시작 다음 날부터 끝까지 제외한 날마다 끝을 하루씩 늦춘다
fn skip_excluded_days(document: &GanttDocument, mut day: i64, mut end: i64) -> (i64, Option<i64>) {
    let latest_end = end + 10_000 * MILLIS_PER_DAY;
    let mut render_end = None;
    let mut is_excluded = false;
    while day <= end && end <= latest_end {
        if !is_excluded {
            render_end = Some(end);
        }
        is_excluded = is_excluded_date(document, day);
        if is_excluded {
            end += MILLIS_PER_DAY;
        }
        day += MILLIS_PER_DAY;
    }
    (end, render_end)
}

/// ganttDb `isInvalidDate`
pub fn is_excluded_date(document: &GanttDocument, millis: i64) -> bool {
    let formatted = format(millis, &document.date_format);
    let date_only = format(millis, "YYYY-MM-DD");
    let contains = |list: &[String], value: &str| list.iter().any(|item| item == value);
    if contains(&document.includes, &formatted) || contains(&document.includes, &date_only) {
        return false;
    }
    let weekday = CivilTime::from_millis(millis).weekday();
    let iso_weekday = if weekday == 0 { 7 } else { weekday };
    let is_weekend =
        iso_weekday == document.weekend_start || iso_weekday == document.weekend_start + 1;
    if contains(&document.excludes, "weekends") && is_weekend {
        return true;
    }
    let weekday_name = WEEKDAY_NAMES[weekday as usize].to_lowercase();
    contains(&document.excludes, &weekday_name)
        || contains(&document.excludes, &formatted)
        || contains(&document.excludes, &date_only)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gantt::parser::parse;

    fn date(year: i64, month: u32, day: u32) -> i64 {
        CivilTime::date(year, month, day).to_millis()
    }

    #[test]
    fn after와_기간으로_시각을_정한다() {
        let document = parse(
            "gantt\ndateFormat YYYY-MM-DD\nA :a1, 2024-01-01, 10d\nB :after a1, 2w\nC :1.5d\n",
        )
        .unwrap();
        let tasks = schedule(&document).unwrap();
        assert_eq!(tasks[1].start, date(2024, 1, 11));
        assert_eq!(tasks[1].end, date(2024, 1, 25));
        assert_eq!(tasks[2].end, date(2024, 1, 27));
    }

    #[test]
    fn 주말을_빼면_끝이_늦춰진다() {
        let document =
            parse("gantt\ndateFormat YYYY-MM-DD\nexcludes weekends\nA :2024-01-05, 3d\n").unwrap();
        let tasks = schedule(&document).unwrap();
        assert_eq!(tasks[0].end, date(2024, 1, 10));
        assert_eq!(tasks[0].render_end, Some(date(2024, 1, 10)));
    }
}
