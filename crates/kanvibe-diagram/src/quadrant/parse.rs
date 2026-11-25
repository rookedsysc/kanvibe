//! quadrantChart 문법(quadrant.jison)의 줄 단위 해석.
//!
//! `title`, `x-axis`·`y-axis`(`-->`로 양 끝을 나눈다), `quadrant-1..4`, 점 `이름(:::클래스)?: [x, y] 스타일`,
//! `classDef 이름 스타일`을 읽는다. 스타일은 `radius`, `color`, `stroke-color`, `stroke-width`다.

use std::collections::HashMap;

use super::config::ChartConfig;
use crate::DiagramError;
use crate::directive::{diagram_body, read_config};
use crate::sanitize::sanitize_text;
use crate::scene::Color;

/// 축 끝 라벨에 `-->`만 있고 반대쪽이 없을 때 붙는 화살표
const LONE_AXIS_ARROW: &str = " \u{27F6} ";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointStyle {
    pub radius: Option<f64>,
    pub color: Option<Color>,
    pub stroke_color: Option<Color>,
    pub stroke_width: Option<f64>,
}

impl PointStyle {
    /// 점 자신의 값이 클래스 값보다 앞선다
    fn or(&self, fallback: &PointStyle) -> PointStyle {
        PointStyle {
            radius: self.radius.or(fallback.radius),
            color: self.color.or(fallback.color),
            stroke_color: self.stroke_color.or(fallback.stroke_color),
            stroke_width: self.stroke_width.or(fallback.stroke_width),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartPoint {
    pub text: String,
    pub class_name: String,
    pub x: f64,
    pub y: f64,
    pub style: PointStyle,
}

#[derive(Clone, Debug, Default)]
pub struct QuadrantChart {
    pub title: String,
    /// 사분면 1..4 글자
    pub quadrant_texts: [String; 4],
    pub x_axis_left: String,
    pub x_axis_right: String,
    pub y_axis_bottom: String,
    pub y_axis_top: String,
    /// 정의에 적힌 순서
    pub points: Vec<ChartPoint>,
    classes: HashMap<String, PointStyle>,
    pub config: ChartConfig,
}

impl QuadrantChart {
    /// 점 스타일에 `classDef` 스타일을 겹친다
    pub fn resolve_style(&self, point: &ChartPoint) -> PointStyle {
        match self.classes.get(&point.class_name) {
            Some(class_style) => point.style.or(class_style),
            None => point.style.clone(),
        }
    }
}

/// 앞머리 `title`은 `title` 문장이 없을 때 쓴다
pub fn parse(definition: &str) -> Result<QuadrantChart, DiagramError> {
    let config = read_config(definition);
    let body = diagram_body(definition);
    let mut lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines
        .next()
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    if header != "quadrantChart" {
        return Err(DiagramError::Syntax(format!(
            "quadrantChart 머리줄: {header}"
        )));
    }
    let mut chart = QuadrantChart {
        title: config
            .title
            .as_deref()
            .map(sanitize_text)
            .unwrap_or_default(),
        config: ChartConfig::from_config(&config),
        ..QuadrantChart::default()
    };
    for line in lines {
        parse_line(&mut chart, line)?;
    }
    Ok(chart)
}

fn parse_line(chart: &mut QuadrantChart, line: &str) -> Result<(), DiagramError> {
    let (keyword, rest) = line
        .split_once(char::is_whitespace)
        .map_or((line, ""), |(keyword, rest)| (keyword, rest.trim()));
    match keyword {
        "title" => chart.title = sanitize_text(rest),
        "x-axis" => {
            (chart.x_axis_left, chart.x_axis_right) = parse_axis(rest);
        }
        "y-axis" => {
            (chart.y_axis_bottom, chart.y_axis_top) = parse_axis(rest);
        }
        "quadrant-1" | "quadrant-2" | "quadrant-3" | "quadrant-4" => {
            let index = usize::from(keyword.as_bytes()[9] - b'1');
            chart.quadrant_texts[index] = clean_text(rest);
        }
        "classDef" => {
            let (name, styles) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            chart
                .classes
                .insert(name.to_string(), parse_styles(styles)?);
        }
        _ if keyword.starts_with("accTitle") || keyword.starts_with("accDescr") => {}
        _ => chart.points.push(parse_point(line)?),
    }
    Ok(())
}

/// `A --> B`는 양 끝, `A -->`는 A 뒤에 화살표를 붙인 한쪽, `A`는 한쪽만
fn parse_axis(rest: &str) -> (String, String) {
    match rest.split_once("-->") {
        Some((start, end)) if end.trim().is_empty() => (
            format!("{}{LONE_AXIS_ARROW}", clean_text(start)),
            String::new(),
        ),
        Some((start, end)) => (clean_text(start), clean_text(end)),
        None => (clean_text(rest), String::new()),
    }
}

/// 앞뒤 공백과 감싼 큰따옴표(마크다운 문자열 "` `"도)를 걷고 DOMPurify를 거친 모양으로 둔다.
/// Mermaid는 마크다운 글자도 꾸미지 않고 `**`·`<br>`까지 그대로 그린다
fn clean_text(text: &str) -> String {
    let trimmed = text.trim();
    let unquoted = ["\"`", "\""]
        .iter()
        .find_map(|open| {
            let close: String = open.chars().rev().collect();
            trimmed.strip_prefix(open)?.strip_suffix(close.as_str())
        })
        .unwrap_or(trimmed);
    sanitize_text(unquoted.trim())
}

/// `이름(:::클래스)?: [x, y] 스타일, …`
fn parse_point(line: &str) -> Result<ChartPoint, DiagramError> {
    let invalid = || DiagramError::Syntax(format!("quadrantChart 점 형식: {line}"));
    let open = line.find('[').ok_or_else(invalid)?;
    let close = open + line[open..].find(']').ok_or_else(invalid)?;
    let name_part = line[..open].trim().strip_suffix(':').ok_or_else(invalid)?;
    let (text, class_name) = name_part
        .split_once(":::")
        .map_or((name_part, ""), |(text, class_name)| {
            (text, class_name.trim())
        });
    let (x, y) = line[open + 1..close].split_once(',').ok_or_else(invalid)?;
    let coordinate = |value: &str| value.trim().parse::<f64>().map_err(|_| invalid());
    Ok(ChartPoint {
        text: clean_text(text),
        class_name: class_name.to_string(),
        x: coordinate(x)?,
        y: coordinate(y)?,
        style: parse_styles(&line[close + 1..])?,
    })
}

/// quadrantDb parseStyles: 정수 반지름, `#rgb`·`#rrggbb` 색, `Npx` 굵기만 받는다
fn parse_styles(styles: &str) -> Result<PointStyle, DiagramError> {
    let mut style = PointStyle::default();
    for entry in styles
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let invalid = || DiagramError::Syntax(format!("quadrantChart 점 스타일: {entry}"));
        let (key, value) = entry.split_once(':').ok_or_else(invalid)?;
        let value = value.trim();
        match key.trim() {
            "radius" => style.radius = Some(value.parse::<u32>().map_err(|_| invalid())?.into()),
            "color" => style.color = Some(parse_hex_color(value).ok_or_else(invalid)?),
            "stroke-color" => {
                style.stroke_color = Some(parse_hex_color(value).ok_or_else(invalid)?)
            }
            "stroke-width" => {
                let pixels = value.strip_suffix("px").ok_or_else(invalid)?;
                style.stroke_width = Some(pixels.parse::<u32>().map_err(|_| invalid())?.into());
            }
            _ => return Err(invalid()),
        }
    }
    Ok(style)
}

fn parse_hex_color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    let is_valid = matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit());
    is_valid
        .then(|| Color::parse_css(&format!("#{hex}")))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 축과_사분면과_점을_읽는다() {
        let chart = parse(
            "quadrantChart\n title Reach\n x-axis Low Reach --> High Reach\n y-axis Low -->\n quadrant-3 Re-evaluate\n Campaign A: [0.3, 0.6]",
        )
        .unwrap();
        assert_eq!(chart.title, "Reach");
        assert_eq!(chart.x_axis_left, "Low Reach");
        assert_eq!(chart.x_axis_right, "High Reach");
        assert_eq!(chart.y_axis_bottom, "Low \u{27F6} ");
        assert_eq!(chart.quadrant_texts[2], "Re-evaluate");
        assert_eq!(chart.points[0].text, "Campaign A");
        assert_eq!((chart.points[0].x, chart.points[0].y), (0.3, 0.6));
    }

    #[test]
    fn 점_스타일은_클래스보다_앞선다() {
        let chart = parse(
            "quadrantChart\n classDef hot color: #ff0000, radius: 10\n P:::hot: [0.1, 0.2] radius: 4, stroke-width: 2px, stroke-color: #00f",
        )
        .unwrap();
        let style = chart.resolve_style(&chart.points[0]);
        assert_eq!(style.radius, Some(4.0));
        assert_eq!(style.color, Some(Color::rgb(255, 0, 0)));
        assert_eq!(style.stroke_color, Some(Color::rgb(0, 0, 255)));
        assert_eq!(style.stroke_width, Some(2.0));
    }

    #[test]
    fn 모르는_스타일은_오류다() {
        assert!(parse("quadrantChart\n P: [0.1, 0.2] size: 3").is_err());
    }
}
