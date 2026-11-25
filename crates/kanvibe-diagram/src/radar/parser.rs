//! radar-beta 문법(@mermaid-js/parser radar.langium)과 radar/db.ts의 값 정리.
//!
//! `title`·`accTitle`·`accDescr`는 줄 단위로 걷어 내고, 나머지는 낱말로 쪼개 읽는다. 축은
//! `axis 이름["라벨"], …`, 곡선은 `curve 이름["라벨"]{값, …}` 또는 `{축: 값, …}`이고, 선택지는
//! `showLegend`·`ticks`·`max`·`min`·`graticule`이다. 곡선 값 묶음은 여러 줄에 걸칠 수 있다.

use crate::DiagramError;
use crate::directive::diagram_body;
use crate::sanitize::sanitize_text;

/// 눈금 모양
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Graticule {
    Circle,
    Polygon,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadarAxis {
    pub name: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadarCurve {
    pub label: String,
    /// 축 순서에 맞춘 값
    pub entries: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadarChart {
    pub title: String,
    pub axes: Vec<RadarAxis>,
    pub curves: Vec<RadarCurve>,
    pub show_legend: bool,
    pub ticks: usize,
    /// 비면 곡선 값의 최댓값
    pub max: Option<f64>,
    pub min: f64,
    pub graticule: Graticule,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Quoted(String),
    Number(f64),
    Symbol(char),
    Newline,
}

/// 곡선 값 하나. 축 이름이 있으면 축 순서로 다시 맞춘다
struct CurveEntry {
    axis: Option<String>,
    value: f64,
}

pub fn parse(definition: &str) -> Result<RadarChart, DiagramError> {
    let body = diagram_body(definition);
    let mut chart = RadarChart {
        title: String::new(),
        axes: Vec::new(),
        curves: Vec::new(),
        show_legend: true,
        ticks: 5,
        max: None,
        min: 0.0,
        graticule: Graticule::Circle,
    };
    let mut statements = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("title")
            && (title.is_empty() || title.starts_with([' ', '\t']))
        {
            chart.title = sanitize_text(title.trim());
            continue;
        }
        if trimmed.starts_with("accTitle") || trimmed.starts_with("accDescr") {
            continue;
        }
        statements.push_str(line);
        statements.push('\n');
    }
    let tokens = tokenize(&statements)?;
    StatementReader {
        tokens: &tokens,
        position: 0,
    }
    .read_all(&mut chart)?;
    Ok(chart)
}

fn tokenize(text: &str) -> Result<Vec<Token>, DiagramError> {
    let mut tokens = Vec::new();
    let characters: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        if character == '%' && characters.get(index + 1) == Some(&'%') {
            while index < characters.len() && characters[index] != '\n' {
                index += 1;
            }
            continue;
        }
        let start = index;
        index += 1;
        match character {
            '\n' => tokens.push(Token::Newline),
            ' ' | '\t' | '\r' => {}
            '"' | '\'' => {
                let end = (index..characters.len())
                    .find(|&position| characters[position] == character)
                    .ok_or_else(|| DiagramError::Syntax("닫히지 않은 따옴표".to_string()))?;
                tokens.push(Token::Quoted(characters[index..end].iter().collect()));
                index = end + 1;
            }
            '0'..='9' => {
                while index < characters.len()
                    && (characters[index].is_ascii_digit() || characters[index] == '.')
                {
                    index += 1;
                }
                let number: String = characters[start..index].iter().collect();
                let value = number
                    .parse()
                    .map_err(|_| DiagramError::Syntax(format!("숫자: {number}")))?;
                tokens.push(Token::Number(value));
            }
            '[' | ']' | '{' | '}' | ',' | ':' => tokens.push(Token::Symbol(character)),
            _ if character.is_alphanumeric() || character == '_' || character == '-' => {
                while index < characters.len()
                    && (characters[index].is_alphanumeric() || "_-".contains(characters[index]))
                {
                    index += 1;
                }
                tokens.push(Token::Word(characters[start..index].iter().collect()));
            }
            other => return Err(DiagramError::Syntax(format!("radar 글자: {other}"))),
        }
    }
    Ok(tokens)
}

struct StatementReader<'a> {
    tokens: &'a [Token],
    position: usize,
}

impl StatementReader<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        self.position += 1;
        token
    }

    fn skip_newlines(&mut self) {
        while self.peek() == Some(&Token::Newline) {
            self.position += 1;
        }
    }

    fn accept(&mut self, symbol: char) -> bool {
        let is_match = self.peek() == Some(&Token::Symbol(symbol));
        if is_match {
            self.position += 1;
        }
        is_match
    }

    fn expect(&mut self, symbol: char) -> Result<(), DiagramError> {
        if self.accept(symbol) {
            Ok(())
        } else {
            Err(DiagramError::Syntax(format!("radar: '{symbol}' 자리")))
        }
    }

    fn word(&mut self) -> Result<String, DiagramError> {
        match self.next() {
            Some(Token::Word(word)) => Ok(word),
            Some(Token::Number(number)) => Ok(number.to_string()),
            other => Err(DiagramError::Syntax(format!("radar 이름 자리: {other:?}"))),
        }
    }

    fn number(&mut self) -> Result<f64, DiagramError> {
        match self.next() {
            Some(Token::Number(number)) => Ok(number),
            other => Err(DiagramError::Syntax(format!("radar 숫자 자리: {other:?}"))),
        }
    }

    /// 머리말(`radar-beta`, `radar-beta:`) 뒤로 문장을 끝까지 읽는다
    fn read_all(&mut self, chart: &mut RadarChart) -> Result<(), DiagramError> {
        self.skip_newlines();
        if self.word()? != "radar-beta" {
            return Err(DiagramError::Syntax("radar-beta 머리줄".to_string()));
        }
        self.accept(':');
        loop {
            self.skip_newlines();
            let Some(token) = self.next() else {
                return Ok(());
            };
            let Token::Word(keyword) = token else {
                return Err(DiagramError::Syntax(format!("radar 문장: {token:?}")));
            };
            self.read_statement(&keyword, chart)?;
        }
    }

    fn read_statement(
        &mut self,
        keyword: &str,
        chart: &mut RadarChart,
    ) -> Result<(), DiagramError> {
        match keyword {
            "axis" => loop {
                let name = self.word()?;
                let label = self.optional_label()?.unwrap_or_else(|| name.clone());
                chart.axes.push(RadarAxis { name, label });
                if !self.accept(',') {
                    return Ok(());
                }
            },
            "curve" => loop {
                let curve = self.curve(&chart.axes)?;
                chart.curves.push(curve);
                if !self.accept(',') {
                    return Ok(());
                }
            },
            _ => {
                self.option(keyword, chart)?;
                if !self.accept(',') {
                    return Ok(());
                }
                // 쉼표 뒤 선택지는 다음 낱말이 이름이다
                let next = self.word()?;
                self.read_statement(&next, chart)
            }
        }
    }

    fn optional_label(&mut self) -> Result<Option<String>, DiagramError> {
        if !self.accept('[') {
            return Ok(None);
        }
        let label = match self.next() {
            Some(Token::Quoted(label)) => label,
            other => return Err(DiagramError::Syntax(format!("radar 라벨: {other:?}"))),
        };
        self.expect(']')?;
        Ok(Some(label))
    }

    fn curve(&mut self, axes: &[RadarAxis]) -> Result<RadarCurve, DiagramError> {
        let name = self.word()?;
        let label = self.optional_label()?.unwrap_or(name);
        self.expect('{')?;
        let mut entries = Vec::new();
        loop {
            self.skip_newlines();
            entries.push(self.curve_entry()?);
            self.skip_newlines();
            if !self.accept(',') {
                break;
            }
        }
        self.expect('}')?;
        Ok(RadarCurve {
            label,
            entries: order_entries(entries, axes)?,
        })
    }

    fn curve_entry(&mut self) -> Result<CurveEntry, DiagramError> {
        if let Some(Token::Word(axis)) = self.peek().cloned() {
            self.position += 1;
            self.accept(':');
            return Ok(CurveEntry {
                axis: Some(axis),
                value: self.number()?,
            });
        }
        Ok(CurveEntry {
            axis: None,
            value: self.number()?,
        })
    }

    fn option(&mut self, name: &str, chart: &mut RadarChart) -> Result<(), DiagramError> {
        match name {
            "showLegend" => chart.show_legend = self.word()? == "true",
            "ticks" => chart.ticks = self.number()? as usize,
            "max" => chart.max = Some(self.number()?),
            "min" => chart.min = self.number()?,
            "graticule" => {
                chart.graticule = match self.word()?.as_str() {
                    "polygon" => Graticule::Polygon,
                    _ => Graticule::Circle,
                }
            }
            other => return Err(DiagramError::Syntax(format!("radar 문장: {other}"))),
        }
        Ok(())
    }
}

/// computeCurveEntries: 첫 값에 축 이름이 없으면 적힌 순서, 있으면 축 순서로 고른다
fn order_entries(entries: Vec<CurveEntry>, axes: &[RadarAxis]) -> Result<Vec<f64>, DiagramError> {
    if entries.first().is_none_or(|entry| entry.axis.is_none()) {
        return Ok(entries.into_iter().map(|entry| entry.value).collect());
    }
    axes.iter()
        .map(|axis| {
            entries
                .iter()
                .find(|entry| entry.axis.as_deref() == Some(axis.name.as_str()))
                .map(|entry| entry.value)
                .ok_or_else(|| DiagramError::Syntax(format!("축 값 없음: {}", axis.label)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 축_곡선_선택지를_읽는다() {
        let chart = parse(
            "radar-beta\n  title Skills\n  axis a[\"Speed\"], b\n  curve c[\"Alice\"]{4, 3}\n  max 5\n  graticule polygon, ticks 3",
        )
        .unwrap();
        assert_eq!(chart.title, "Skills");
        assert_eq!(chart.axes[0].label, "Speed");
        assert_eq!(chart.axes[1].label, "b");
        assert_eq!(chart.curves[0].label, "Alice");
        assert_eq!(chart.curves[0].entries, vec![4.0, 3.0]);
        assert_eq!(chart.max, Some(5.0));
        assert_eq!(chart.graticule, Graticule::Polygon);
        assert_eq!(chart.ticks, 3);
    }

    #[test]
    fn 축_이름으로_적은_값은_축_순서로_맞춘다() {
        let chart =
            parse("radar-beta\naxis a, b\ncurve c{\n b: 2,\n a: 1\n}\nshowLegend false").unwrap();
        assert_eq!(chart.curves[0].entries, vec![1.0, 2.0]);
        assert!(!chart.show_legend);
    }
}
