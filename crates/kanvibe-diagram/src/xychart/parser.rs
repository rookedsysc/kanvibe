//! xychart 문법(xychart.jison)과 xychartDb.ts의 값 정리.
//!
//! 머리줄 `xychart-beta`(또는 `xychart`) 뒤에 `vertical`·`horizontal`을 둘 수 있다. 문장은 `title`, `x-axis`
//! (제목, `[범주, …]` 또는 `최솟값 --> 최댓값`), `y-axis`(제목, `최솟값 --> 최댓값`), `line`·`bar`(제목,
//! `[값, …]`)다. 따옴표 없는 글자는 Jison 낱말을 공백 없이 이어 붙인 모양이 된다.

use crate::DiagramError;
use crate::directive::diagram_body;
use crate::sanitize::sanitize_text;

#[derive(Clone, Debug, PartialEq)]
pub enum AxisScale {
    Band {
        title: String,
        categories: Vec<String>,
    },
    Linear {
        title: String,
        min: f64,
        max: f64,
    },
}

impl AxisScale {
    pub fn title(&self) -> &str {
        match self {
            AxisScale::Band { title, .. } | AxisScale::Linear { title, .. } => title,
        }
    }

    fn with_title(self, new_title: String) -> Self {
        match self {
            AxisScale::Band { categories, .. } => AxisScale::Band {
                title: new_title,
                categories,
            },
            AxisScale::Linear { min, max, .. } => AxisScale::Linear {
                title: new_title,
                min,
                max,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotKind {
    Line,
    Bar,
}

/// 그릴 계열 하나. `points`는 (범주 글자, 값)이다
#[derive(Clone, Debug, PartialEq)]
pub struct Plot {
    pub kind: PlotKind,
    /// 정의에 나온 순서(색 고르기에 쓴다)
    pub index: usize,
    pub points: Vec<(String, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct XyChart {
    /// 머리줄 방향(`horizontal`이면 참). 적지 않으면 `None`이고 설정 `chartOrientation`을 따른다
    pub header_orientation: Option<bool>,
    pub title: String,
    pub x_axis: AxisScale,
    pub y_axis: AxisScale,
    pub plots: Vec<Plot>,
}

/// 축 값을 정의에서 정했는지(정하지 않았으면 계열 값으로 넓힌다)
struct AxisFlags {
    has_x_axis: bool,
    has_y_axis: bool,
}

pub fn parse(definition: &str) -> Result<XyChart, DiagramError> {
    let body = diagram_body(definition);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines
        .next()
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    let orientation = header
        .strip_prefix("xychart-beta")
        .or_else(|| header.strip_prefix("xychart"))
        .ok_or_else(|| DiagramError::Syntax("xychart 머리줄".to_string()))?;
    let mut chart = XyChart {
        header_orientation: match orientation.trim() {
            "horizontal" => Some(true),
            "vertical" => Some(false),
            _ => None,
        },
        title: String::new(),
        x_axis: AxisScale::Band {
            title: String::new(),
            categories: Vec::new(),
        },
        y_axis: AxisScale::Linear {
            title: String::new(),
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        },
        plots: Vec::new(),
    };
    let mut flags = AxisFlags {
        has_x_axis: false,
        has_y_axis: false,
    };
    for line in lines {
        read_statement(line, &mut chart, &mut flags)?;
    }
    if chart.plots.is_empty() {
        return Err(DiagramError::Syntax("그릴 계열이 없습니다".to_string()));
    }
    Ok(chart)
}

fn read_statement(
    line: &str,
    chart: &mut XyChart,
    flags: &mut AxisFlags,
) -> Result<(), DiagramError> {
    let (keyword, rest) = line
        .split_once(char::is_whitespace)
        .map_or((line, ""), |(keyword, rest)| (keyword, rest.trim()));
    match keyword {
        "title" => chart.title = sanitize_text(text_value(rest).trim()),
        "x-axis" => {
            let axis = read_axis(rest, true)?;
            flags.has_x_axis |= axis_has_values(&axis);
            chart.x_axis = merge_axis(&chart.x_axis, axis);
        }
        "y-axis" => {
            let axis = read_axis(rest, false)?;
            flags.has_y_axis |= axis_has_values(&axis);
            chart.y_axis = merge_axis(&chart.y_axis, axis);
        }
        "line" | "bar" => {
            let values = read_plot_values(rest)?;
            let kind = if keyword == "line" {
                PlotKind::Line
            } else {
                PlotKind::Bar
            };
            add_plot(chart, flags, kind, &values);
        }
        _ if keyword.starts_with("accTitle") || keyword.starts_with("accDescr") => {}
        other => return Err(DiagramError::Syntax(format!("xychart 문장: {other}"))),
    }
    Ok(())
}

/// 따옴표 글자는 안쪽을, 아니면 공백을 지운 낱말 이음을 돌려준다
fn text_value(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(inner) = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        let inner = inner
            .strip_prefix('`')
            .and_then(|rest| rest.strip_suffix('`'))
            .unwrap_or(inner);
        return inner.to_string();
    }
    trimmed.split_whitespace().collect()
}

/// 축 문장 하나. 값(범주·범위)이 없으면 제목만 바꾸는 `Band`로 돌려준다
fn read_axis(rest: &str, can_be_band: bool) -> Result<AxisScale, DiagramError> {
    if can_be_band && let Some(open) = find_unquoted(rest, '[') {
        let close = rest
            .rfind(']')
            .ok_or_else(|| DiagramError::Syntax("닫히지 않은 범주".to_string()))?;
        let categories = split_unquoted(&rest[open + 1..close])
            .iter()
            .map(|category| sanitize_text(text_value(category).trim()))
            .collect();
        return Ok(AxisScale::Band {
            title: axis_title(&rest[..open]),
            categories,
        });
    }
    if let Some((before, max)) = rest.rsplit_once("-->") {
        let before = before.trim_end();
        let split = before
            .rfind(char::is_whitespace)
            .map_or(0, |index| index + 1);
        let min = parse_number(&before[split..])?;
        return Ok(AxisScale::Linear {
            title: axis_title(&before[..split]),
            min,
            max: parse_number(max)?,
        });
    }
    Ok(AxisScale::Band {
        title: axis_title(rest),
        categories: Vec::new(),
    })
}

fn axis_title(text: &str) -> String {
    sanitize_text(text_value(text).trim())
}

fn axis_has_values(axis: &AxisScale) -> bool {
    match axis {
        AxisScale::Band { categories, .. } => !categories.is_empty(),
        AxisScale::Linear { .. } => true,
    }
}

/// 제목만 적은 축 문장은 이전 값을 두고 제목만 바꾼다
fn merge_axis(previous: &AxisScale, next: AxisScale) -> AxisScale {
    if axis_has_values(&next) {
        return next;
    }
    previous.clone().with_title(next.title().to_string())
}

fn parse_number(text: &str) -> Result<f64, DiagramError> {
    text.trim()
        .parse()
        .map_err(|_| DiagramError::Syntax(format!("xychart 숫자: {text}")))
}

fn find_unquoted(text: &str, target: char) -> Option<usize> {
    let mut is_quoted = false;
    text.char_indices().find_map(|(index, character)| {
        if character == '"' {
            is_quoted = !is_quoted;
        }
        (!is_quoted && character == target).then_some(index)
    })
}

/// 따옴표 밖 쉼표로 나눈다
fn split_unquoted(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut is_quoted = false;
    for character in text.chars() {
        if character == '"' {
            is_quoted = !is_quoted;
        }
        if character == ',' && !is_quoted {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(character);
        }
    }
    parts.push(current);
    parts
}

fn read_plot_values(rest: &str) -> Result<Vec<f64>, DiagramError> {
    let open = find_unquoted(rest, '[')
        .ok_or_else(|| DiagramError::Syntax("계열 값이 없습니다".to_string()))?;
    let close = rest
        .rfind(']')
        .ok_or_else(|| DiagramError::Syntax("닫히지 않은 계열 값".to_string()))?;
    rest[open + 1..close].split(',').map(parse_number).collect()
}

/// setLineData·setBarData: 축 값을 정하지 않았으면 계열 값으로 축을 넓히고, 범주와 값을 짝짓는다
fn add_plot(chart: &mut XyChart, flags: &mut AxisFlags, kind: PlotKind, values: &[f64]) {
    let index = chart.plots.len();
    let points = if values.is_empty() {
        Vec::new()
    } else {
        widen_axes(chart, flags, values);
        pair_with_categories(&chart.x_axis, values)
    };
    chart.plots.push(Plot {
        kind,
        index,
        points,
    });
}

fn widen_axes(chart: &mut XyChart, flags: &mut AxisFlags, values: &[f64]) {
    if !flags.has_x_axis {
        let (previous_min, previous_max) = match chart.x_axis {
            AxisScale::Linear { min, max, .. } => (min, max),
            AxisScale::Band { .. } => (f64::INFINITY, f64::NEG_INFINITY),
        };
        chart.x_axis = AxisScale::Linear {
            title: chart.x_axis.title().to_string(),
            min: previous_min.min(1.0),
            max: previous_max.max(values.len() as f64),
        };
        flags.has_x_axis = true;
    }
    if !flags.has_y_axis {
        let (previous_min, previous_max) = match chart.y_axis {
            AxisScale::Linear { min, max, .. } => (min, max),
            AxisScale::Band { .. } => (f64::INFINITY, f64::NEG_INFINITY),
        };
        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
        let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        chart.y_axis = AxisScale::Linear {
            title: chart.y_axis.title().to_string(),
            min: previous_min.min(low),
            max: previous_max.max(high),
        };
    }
}

/// 숫자 축이면 최솟값부터 같은 간격의 범주 글자(`${i}`)를 만든다. JS처럼 간격을 더해 가며 센다
fn pair_with_categories(x_axis: &AxisScale, values: &[f64]) -> Vec<(String, f64)> {
    let categories: Vec<String> = match x_axis {
        AxisScale::Band { categories, .. } => categories.clone(),
        AxisScale::Linear { min, max, .. } => {
            let step = (max - min) / (values.len() as f64 - 1.0);
            let mut labels = Vec::new();
            let mut position = *min;
            while position <= *max && labels.len() < values.len() {
                labels.push(format_number(position));
                position += step;
            }
            labels
        }
    };
    categories.into_iter().zip(values.iter().copied()).collect()
}

/// JS `Number.prototype.toString`과 같은 짧은 십진 표기(정수는 소수점 없이)
pub fn format_number(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e21 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 범주_축과_범위_축을_읽는다() {
        let chart = parse(
            "xychart-beta horizontal\n title \"Sales\"\n x-axis [jan, \"feb x\"]\n y-axis \"Revenue (in $)\" 4000 --> 11000\n bar [1, 2]",
        )
        .unwrap();
        assert_eq!(chart.header_orientation, Some(true));
        assert_eq!(chart.title, "Sales");
        assert_eq!(
            chart.x_axis,
            AxisScale::Band {
                title: String::new(),
                categories: vec!["jan".to_string(), "feb x".to_string()],
            }
        );
        assert_eq!(chart.y_axis.title(), "Revenue (in $)");
        assert_eq!(chart.plots[0].points[1], ("feb x".to_string(), 2.0));
    }

    #[test]
    fn 축_값이_없으면_계열_값으로_범위를_넓힌다() {
        let chart = parse("xychart\n y-axis Tasks\n line [3, 7]\n bar [1, 9, 4]").unwrap();
        assert_eq!(
            chart.y_axis,
            AxisScale::Linear {
                title: "Tasks".to_string(),
                min: 1.0,
                max: 9.0,
            }
        );
        assert_eq!(chart.plots[0].points[1].0, "2");
    }
}
